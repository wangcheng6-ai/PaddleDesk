use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::Local;
use serde::Serialize;
use tokio::sync::{mpsc, Semaphore};

use crate::{
    api::{InputDoc, OcrService, ParseOptions, ProgressFn},
    model::{OcrError, RecognitionResult, ServiceId},
    storage::{NewTask, Store},
};

const MAX_RETRIES: u32 = 3;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueueEvent {
    Progress {
        id: String,
        stage: String,
        page: u32,
        total: u32,
    },
    Done {
        id: String,
    },
    Failed {
        id: String,
        error: OcrError,
    },
    Canceled {
        id: String,
    },
}

pub struct Queue {
    store: Arc<Mutex<Store>>,
    services: HashMap<ServiceId, Arc<dyn OcrService>>,
    semaphore: Arc<Semaphore>,
    events: mpsc::UnboundedSender<QueueEvent>,
    retry_base: Duration,
    active: Mutex<HashSet<String>>,
}

impl Queue {
    pub fn new(
        store: Arc<Mutex<Store>>,
        services: HashMap<ServiceId, Arc<dyn OcrService>>,
        concurrency: usize,
        events: mpsc::UnboundedSender<QueueEvent>,
        retry_base: Duration,
    ) -> Arc<Queue> {
        Arc::new(Self {
            store,
            services,
            semaphore: Arc::new(Semaphore::new(concurrency)),
            events,
            retry_base,
            active: Mutex::new(HashSet::new()),
        })
    }

    pub fn submit(self: &Arc<Self>, task: NewTask) {
        let inserted = self
            .lock_store()
            .and_then(|store| store.insert_task(&task).map_err(storage_error));
        if let Err(error) = inserted {
            self.emit_failed(&task.id, error);
            return;
        }
        self.spawn(task);
    }

    pub fn resume(self: &Arc<Self>) {
        let tasks = self
            .lock_store()
            .and_then(|store| store.unfinished_tasks().map_err(storage_error));
        let Ok(tasks) = tasks else {
            return;
        };
        for task in tasks {
            self.spawn(NewTask {
                id: task.id,
                service: task.service,
                input_path: task.input_path,
                options_json: task.options_json,
            });
        }
    }

    pub fn cancel(&self, id: &str) {
        let canceled = self
            .lock_store()
            .and_then(|store| store.cancel_task(id).map_err(storage_error));
        match canceled {
            Ok(true) => {
                let _ = self.events.send(QueueEvent::Canceled { id: id.into() });
            }
            Ok(false) => {}
            Err(error) => self.emit_failed(id, error),
        }
    }

    fn spawn(self: &Arc<Self>, task: NewTask) {
        let registered = self
            .active
            .lock()
            .map(|mut active| active.insert(task.id.clone()));
        match registered {
            Ok(true) => {}
            Ok(false) => return,
            Err(_) => {
                self.emit_failed(&task.id, storage_error("queue state lock poisoned"));
                return;
            }
        }
        let queue = Arc::clone(self);
        tokio::spawn(async move { queue.run(task).await });
    }

    async fn run(self: Arc<Self>, task: NewTask) {
        self.work(&task).await;
        if let Ok(mut active) = self.active.lock() {
            active.remove(&task.id);
        }
    }

    async fn work(&self, task: &NewTask) {
        let Ok(_permit) = self.semaphore.acquire().await else {
            self.emit_failed(&task.id, OcrError::Parse("queue stopped".into()));
            return;
        };
        match self.progress(&task.id, "uploading", 0, 0) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                self.fail(&task.id, error);
                return;
            }
        }
        match self.parse_with_retry(task).await {
            Ok(result) => self.finish(task, &result),
            Err(error) => self.fail(&task.id, error),
        }
    }

    async fn parse_with_retry(&self, task: &NewTask) -> Result<RecognitionResult, OcrError> {
        let service = self
            .services
            .get(&task.service)
            .cloned()
            .ok_or_else(|| OcrError::Parse("OCR service is not configured".into()))?;
        let options: ParseOptions = serde_json::from_str(&task.options_json)
            .map_err(|error| OcrError::Parse(error.to_string()))?;
        let input = InputDoc {
            path: task.input_path.clone().into(),
        };
        for attempt in 0..=MAX_RETRIES {
            let progress_error = Arc::new(Mutex::new(None));
            let result = service
                .parse(
                    &input,
                    &options,
                    self.progress_callback(task.id.clone(), progress_error.clone()),
                )
                .await;
            if let Some(error) = take_progress_error(&progress_error)? {
                return Err(error);
            }
            match result {
                Ok(result) => return Ok(result),
                Err(error) if !is_retryable(&error) || attempt == MAX_RETRIES => return Err(error),
                Err(_) => tokio::time::sleep(self.retry_base.saturating_mul(1 << attempt)).await,
            }
        }
        unreachable!()
    }

    fn progress_callback(
        &self,
        id: String,
        progress_error: Arc<Mutex<Option<OcrError>>>,
    ) -> ProgressFn {
        let store = Arc::clone(&self.store);
        let events = self.events.clone();
        Box::new(move |page, total| {
            let updated = store
                .lock()
                .map_err(|_| storage_error("store lock poisoned"))
                .and_then(|store| {
                    store
                        .update_status_if_active(&id, "processing", Some((page, total)), None)
                        .map_err(storage_error)
                });
            match updated {
                Ok(true) => {
                    let _ = events.send(QueueEvent::Progress {
                        id: id.clone(),
                        stage: "processing".into(),
                        page,
                        total,
                    });
                }
                Ok(false) => {}
                Err(error) => set_progress_error(&progress_error, error),
            }
        })
    }

    fn progress(&self, id: &str, stage: &str, page: u32, total: u32) -> Result<bool, OcrError> {
        let updated = self
            .lock_store()?
            .update_status_if_active(id, stage, Some((page, total)), None)
            .map_err(storage_error)?;
        if updated {
            let _ = self.events.send(QueueEvent::Progress {
                id: id.into(),
                stage: stage.into(),
                page,
                total,
            });
        }
        Ok(updated)
    }

    fn finish(&self, task: &NewTask, result: &RecognitionResult) {
        let file_name = Path::new(&task.input_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&task.input_path);
        let today = Local::now().date_naive().to_string();
        let completed = self.lock_store().and_then(|store| {
            store
                .complete_task(&task.id, file_name, result, &today, task.service)
                .map_err(storage_error)
        });
        match completed {
            Ok(true) => {
                let _ = self.events.send(QueueEvent::Done {
                    id: task.id.clone(),
                });
            }
            Ok(false) => {}
            Err(error) => self.fail(&task.id, error),
        }
    }

    fn fail(&self, id: &str, error: OcrError) {
        let persisted = self.lock_store().and_then(|store| {
            store
                .update_status_if_active(id, "failed", None, Some(&error))
                .map_err(storage_error)
        });
        match persisted {
            Ok(true) => self.emit_failed(id, error),
            Ok(false) => {}
            Err(storage) => self.emit_failed(id, storage),
        }
    }

    fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, Store>, OcrError> {
        self.store
            .lock()
            .map_err(|_| storage_error("store lock poisoned"))
    }

    fn emit_failed(&self, id: &str, error: OcrError) {
        let _ = self.events.send(QueueEvent::Failed {
            id: id.into(),
            error,
        });
    }
}

fn is_retryable(error: &OcrError) -> bool {
    !matches!(error, OcrError::Auth | OcrError::Quota)
}

fn storage_error(error: impl std::fmt::Display) -> OcrError {
    OcrError::Parse(format!("storage error: {error}"))
}

fn set_progress_error(slot: &Mutex<Option<OcrError>>, error: OcrError) {
    if let Ok(mut slot) = slot.lock() {
        *slot = Some(error);
    }
}

fn take_progress_error(slot: &Mutex<Option<OcrError>>) -> Result<Option<OcrError>, OcrError> {
    slot.lock()
        .map(|mut slot| slot.take())
        .map_err(|_| storage_error("progress state lock poisoned"))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicU32, Ordering},
            Arc, Mutex,
        },
        time::Duration,
    };

    use tokio::{sync::mpsc, time::timeout};

    use super::*;
    use crate::{
        api::{mock::MockOcr, OcrService},
        model::{OcrError, RecognitionResult, ServiceId},
        storage::{NewTask, Store},
    };

    const TEST_TIMEOUT: Duration = Duration::from_secs(1);

    async fn terminal(rx: &mut mpsc::UnboundedReceiver<QueueEvent>) -> QueueEvent {
        timeout(TEST_TIMEOUT, async {
            loop {
                match rx.recv().await.expect("queue event channel closed") {
                    event @ (QueueEvent::Done { .. }
                    | QueueEvent::Failed { .. }
                    | QueueEvent::Canceled { .. }) => return event,
                    QueueEvent::Progress { .. } => {}
                }
            }
        })
        .await
        .expect("timed out waiting for terminal queue event")
    }

    async fn wait_for_stage(rx: &mut mpsc::UnboundedReceiver<QueueEvent>, wanted: &str) {
        timeout(TEST_TIMEOUT, async {
            loop {
                if let QueueEvent::Progress { stage, .. } =
                    rx.recv().await.expect("queue event channel closed")
                {
                    if stage == wanted {
                        return;
                    }
                }
            }
        })
        .await
        .expect("timed out waiting for queue progress");
    }

    async fn setup(
        svc: MockOcr,
        conc: usize,
    ) -> (
        Arc<Queue>,
        Arc<Mutex<Store>>,
        mpsc::UnboundedReceiver<QueueEvent>,
    ) {
        setup_service(Arc::new(svc), conc).await
    }

    async fn setup_service(
        svc: Arc<dyn OcrService>,
        conc: usize,
    ) -> (
        Arc<Queue>,
        Arc<Mutex<Store>>,
        mpsc::UnboundedReceiver<QueueEvent>,
    ) {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(Store::open(&d.path().join("t.db")).unwrap()));
        std::mem::forget(d);
        let (tx, rx) = mpsc::unbounded_channel();
        let mut m: HashMap<ServiceId, Arc<dyn OcrService>> = HashMap::new();
        m.insert(ServiceId::Vl16, svc);
        (
            Queue::new(store.clone(), m, conc, tx, Duration::from_millis(1)),
            store,
            rx,
        )
    }

    struct TrackingOcr {
        active: Arc<AtomicU32>,
        max_active: Arc<AtomicU32>,
    }

    #[async_trait::async_trait]
    impl OcrService for TrackingOcr {
        fn id(&self) -> ServiceId {
            ServiceId::Vl16
        }

        async fn parse(
            &self,
            _input: &InputDoc,
            _options: &ParseOptions,
            progress: ProgressFn,
        ) -> Result<RecognitionResult, OcrError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(30)).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            progress(1, 1);
            Ok(RecognitionResult {
                markdown: "tracked".into(),
                page_count: 1,
                pages: vec![],
            })
        }
    }

    #[tokio::test]
    async fn retries_then_succeeds() {
        let (q, store, mut rx) = setup(MockOcr::failing(2, OcrError::Network("x".into())), 1).await;
        q.submit(NewTask {
            id: "t1".into(),
            service: ServiceId::Vl16,
            input_path: "a.png".into(),
            options_json: "{}".into(),
        });
        match terminal(&mut rx).await {
            QueueEvent::Done { id } => assert_eq!(id, "t1"),
            _ => panic!("task should succeed"),
        }
        assert!(store.lock().unwrap().get_result("t1").unwrap().is_some());
    }

    #[tokio::test]
    async fn exhausted_network_error_retries_three_times_then_fails() {
        let svc = MockOcr::failing(99, OcrError::Network("offline".into()));
        let probe = svc.clone();
        let (q, store, mut rx) = setup(svc, 1).await;
        q.submit(NewTask {
            id: "network".into(),
            service: ServiceId::Vl16,
            input_path: "a.png".into(),
            options_json: "{}".into(),
        });
        match terminal(&mut rx).await {
            QueueEvent::Failed {
                error: OcrError::Network(message),
                ..
            } => assert_eq!(message, "offline"),
            _ => panic!("task should fail with network error"),
        }
        assert_eq!(probe.call_count(), 4);
        assert_eq!(
            store
                .lock()
                .unwrap()
                .list_tasks(Some("failed"))
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn progress_success_result_and_usage_stay_in_sync() {
        let (q, store, mut rx) = setup(MockOcr::new(), 1).await;
        q.submit(NewTask {
            id: "success".into(),
            service: ServiceId::Vl16,
            input_path: "a.png".into(),
            options_json: "{}".into(),
        });
        let (uploading, processing) = timeout(TEST_TIMEOUT, async {
            let (mut uploading, mut processing) = (false, false);
            loop {
                match rx.recv().await.expect("queue event channel closed") {
                    QueueEvent::Progress { stage, .. } if stage == "uploading" => uploading = true,
                    QueueEvent::Progress { stage, .. } if stage == "processing" => {
                        processing = true
                    }
                    QueueEvent::Done { .. } => return (uploading, processing),
                    QueueEvent::Progress { .. } => {}
                    _ => panic!("task should succeed"),
                }
            }
        })
        .await
        .expect("timed out waiting for successful task");
        assert!(uploading && processing);
        let store = store.lock().unwrap();
        let task = &store.list_tasks(Some("done")).unwrap()[0];
        assert_eq!((task.progress_page, task.total_pages), (1, 1));
        assert!(store.get_result("success").unwrap().is_some());
        assert_eq!(store.usage_since(1).unwrap()[0].pages, 1);
    }

    #[tokio::test]
    async fn auth_error_fails_immediately_no_retry() {
        let svc = MockOcr::failing(99, OcrError::Auth);
        let probe = svc.clone();
        let (q, _s, mut rx) = setup(svc, 1).await;
        q.submit(NewTask {
            id: "t1".into(),
            service: ServiceId::Vl16,
            input_path: "a.png".into(),
            options_json: "{}".into(),
        });
        match terminal(&mut rx).await {
            QueueEvent::Failed {
                error: OcrError::Auth,
                ..
            } => {}
            _ => panic!("task should fail with auth error"),
        }
        assert_eq!(probe.call_count(), 1);
    }

    #[tokio::test]
    async fn quota_error_fails_immediately_no_retry() {
        let svc = MockOcr::failing(99, OcrError::Quota);
        let probe = svc.clone();
        let (q, _s, mut rx) = setup(svc, 1).await;
        q.submit(NewTask {
            id: "t1".into(),
            service: ServiceId::Vl16,
            input_path: "a.png".into(),
            options_json: "{}".into(),
        });
        match terminal(&mut rx).await {
            QueueEvent::Failed {
                error: OcrError::Quota,
                ..
            } => {}
            _ => panic!("task should fail with quota error"),
        }
        assert_eq!(probe.call_count(), 1);
    }

    #[tokio::test]
    async fn cancel_emits_terminal_event_and_persists_status() {
        let (q, store, mut rx) = setup(MockOcr::new(), 0).await;
        q.submit(NewTask {
            id: "t1".into(),
            service: ServiceId::Vl16,
            input_path: "a.png".into(),
            options_json: "{}".into(),
        });
        q.cancel("t1");
        let event = terminal(&mut rx).await;
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({"type": "canceled", "id": "t1"})
        );
        match event {
            QueueEvent::Canceled { id } => assert_eq!(id, "t1"),
            _ => panic!("task should be canceled"),
        }
        let tasks = store.lock().unwrap().list_tasks(Some("canceled")).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "t1");
    }

    #[tokio::test]
    async fn cancellation_cannot_be_overwritten_by_running_worker() {
        let (q, store, mut rx) = setup(MockOcr::new(), 1).await;
        q.submit(NewTask {
            id: "t1".into(),
            service: ServiceId::Vl16,
            input_path: "a.png".into(),
            options_json: "{}".into(),
        });
        wait_for_stage(&mut rx, "uploading").await;
        q.cancel("t1");
        match terminal(&mut rx).await {
            QueueEvent::Canceled { id } => assert_eq!(id, "t1"),
            _ => panic!("task should be canceled"),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        let store = store.lock().unwrap();
        let tasks = store.list_tasks(Some("canceled")).unwrap();
        assert_eq!(tasks.len(), 1);
        assert!(store.get_result("t1").unwrap().is_none());
        assert!(store.usage_since(1).unwrap().is_empty());
    }

    #[tokio::test]
    async fn submit_and_repeated_resume_start_only_one_worker() {
        let svc = MockOcr::new();
        let probe = svc.clone();
        let (q, store, mut rx) = setup(svc, 3).await;
        q.submit(NewTask {
            id: "t1".into(),
            service: ServiceId::Vl16,
            input_path: "a.png".into(),
            options_json: "{}".into(),
        });
        q.resume();
        q.resume();
        match terminal(&mut rx).await {
            QueueEvent::Done { id } => assert_eq!(id, "t1"),
            _ => panic!("task should succeed"),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(probe.call_count(), 1);
        let usage = store.lock().unwrap().usage_since(1).unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].pages, 1);
    }

    #[tokio::test]
    async fn semaphore_limits_parallel_service_calls() {
        let max_active = Arc::new(AtomicU32::new(0));
        let service = TrackingOcr {
            active: Arc::new(AtomicU32::new(0)),
            max_active: max_active.clone(),
        };
        let (q, _store, mut rx) = setup_service(Arc::new(service), 2).await;
        for index in 0..4 {
            q.submit(NewTask {
                id: format!("t{index}"),
                service: ServiceId::Vl16,
                input_path: "a.png".into(),
                options_json: "{}".into(),
            });
        }
        for _ in 0..4 {
            assert!(matches!(terminal(&mut rx).await, QueueEvent::Done { .. }));
        }
        assert_eq!(max_active.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn resume_picks_up_unfinished() {
        let (q, store, mut rx) = setup(MockOcr::new(), 1).await;
        store
            .lock()
            .unwrap()
            .insert_task(&NewTask {
                id: "old".into(),
                service: ServiceId::Vl16,
                input_path: "b.png".into(),
                options_json: "{}".into(),
            })
            .unwrap();
        q.resume();
        match terminal(&mut rx).await {
            QueueEvent::Done { id } => assert_eq!(id, "old"),
            _ => panic!("resumed task should succeed"),
        }
    }
}
