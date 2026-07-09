use std::{
    collections::HashMap,
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
}

pub struct Queue {
    store: Arc<Mutex<Store>>,
    services: HashMap<ServiceId, Arc<dyn OcrService>>,
    semaphore: Arc<Semaphore>,
    events: mpsc::UnboundedSender<QueueEvent>,
    retry_base: Duration,
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
        })
    }

    pub fn submit(self: &Arc<Self>, task: NewTask) {
        self.store
            .lock()
            .unwrap()
            .insert_task(&task)
            .expect("failed to insert queue task");
        self.spawn(task);
    }

    pub fn resume(self: &Arc<Self>) {
        let tasks = self
            .store
            .lock()
            .unwrap()
            .unfinished_tasks()
            .expect("failed to load unfinished tasks");
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
        self.store
            .lock()
            .unwrap()
            .update_status(id, "canceled", None, None)
            .expect("failed to cancel queue task");
    }

    fn spawn(self: &Arc<Self>, task: NewTask) {
        let queue = Arc::clone(self);
        tokio::spawn(async move { queue.run(task).await });
    }

    async fn run(self: Arc<Self>, task: NewTask) {
        let Ok(_permit) = Arc::clone(&self.semaphore).acquire_owned().await else {
            return;
        };
        if self.is_canceled(&task.id) {
            return;
        }
        self.progress(&task.id, "uploading", 0, 0);
        let result = self.parse_with_retry(&task).await;
        match result {
            Ok(result) => self.finish(&task, &result),
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
            let result = service
                .parse(&input, &options, self.progress_callback(task.id.clone()))
                .await;
            match result {
                Ok(result) => return Ok(result),
                Err(error) if !is_retryable(&error) || attempt == MAX_RETRIES => return Err(error),
                Err(_) => tokio::time::sleep(self.retry_base.saturating_mul(1 << attempt)).await,
            }
        }
        unreachable!()
    }

    fn progress_callback(&self, id: String) -> ProgressFn {
        let store = Arc::clone(&self.store);
        let events = self.events.clone();
        Box::new(move |page, total| {
            store
                .lock()
                .unwrap()
                .update_status(&id, "processing", Some((page, total)), None)
                .expect("failed to persist queue progress");
            let _ = events.send(QueueEvent::Progress {
                id: id.clone(),
                stage: "processing".into(),
                page,
                total,
            });
        })
    }

    fn progress(&self, id: &str, stage: &str, page: u32, total: u32) {
        self.store
            .lock()
            .unwrap()
            .update_status(id, stage, Some((page, total)), None)
            .expect("failed to persist queue status");
        let _ = self.events.send(QueueEvent::Progress {
            id: id.into(),
            stage: stage.into(),
            page,
            total,
        });
    }

    fn finish(&self, task: &NewTask, result: &RecognitionResult) {
        let file_name = Path::new(&task.input_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&task.input_path);
        let today = Local::now().date_naive().to_string();
        let store = self.store.lock().unwrap();
        store
            .save_result(&task.id, file_name, result)
            .expect("failed to save OCR result");
        store
            .add_usage(&today, task.service, result.page_count)
            .expect("failed to save OCR usage");
        store
            .update_status(&task.id, "done", None, None)
            .expect("failed to complete queue task");
        drop(store);
        let _ = self.events.send(QueueEvent::Done {
            id: task.id.clone(),
        });
    }

    fn fail(&self, id: &str, error: OcrError) {
        self.store
            .lock()
            .unwrap()
            .update_status(id, "failed", None, Some(&error))
            .expect("failed to persist queue failure");
        let _ = self.events.send(QueueEvent::Failed {
            id: id.into(),
            error,
        });
    }

    fn is_canceled(&self, id: &str) -> bool {
        self.store
            .lock()
            .unwrap()
            .list_tasks(None)
            .expect("failed to inspect queue task")
            .into_iter()
            .any(|task| task.id == id && task.status == "canceled")
    }
}

fn is_retryable(error: &OcrError) -> bool {
    !matches!(error, OcrError::Auth | OcrError::Quota)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use tokio::sync::mpsc;

    use super::*;
    use crate::{
        api::{mock::MockOcr, OcrService},
        model::{OcrError, ServiceId},
        storage::{NewTask, Store},
    };

    async fn setup(
        svc: MockOcr,
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
        m.insert(ServiceId::Vl16, Arc::new(svc));
        (
            Queue::new(store.clone(), m, conc, tx, Duration::from_millis(1)),
            store,
            rx,
        )
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
        loop {
            if let Some(QueueEvent::Done { id }) = rx.recv().await {
                assert_eq!(id, "t1");
                break;
            }
        }
        assert!(store.lock().unwrap().get_result("t1").unwrap().is_some());
    }

    #[tokio::test]
    async fn auth_error_fails_immediately_no_retry() {
        let (q, _s, mut rx) = setup(MockOcr::failing(99, OcrError::Auth), 1).await;
        q.submit(NewTask {
            id: "t1".into(),
            service: ServiceId::Vl16,
            input_path: "a.png".into(),
            options_json: "{}".into(),
        });
        loop {
            match rx.recv().await.unwrap() {
                QueueEvent::Failed {
                    error: OcrError::Auth,
                    ..
                } => break,
                QueueEvent::Done { .. } => panic!("should fail"),
                _ => {}
            }
        }
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
        loop {
            if let Some(QueueEvent::Done { id }) = rx.recv().await {
                assert_eq!(id, "old");
                break;
            }
        }
    }
}
