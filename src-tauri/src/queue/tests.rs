use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Barrier, Mutex, TryLockError,
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
    setup_service_with_persistence(svc, conc, true).await
}

async fn setup_service_with_persistence(
    svc: Arc<dyn OcrService>,
    conc: usize,
    persist_results: bool,
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
        Queue::new(
            store.clone(),
            m,
            conc,
            tx,
            Duration::from_millis(1),
            persist_results,
        ),
        store,
        rx,
    )
}

fn assert_resume_claim_handshake(q: &Arc<Queue>, store: &Arc<Mutex<Store>>) {
    q.active.lock().unwrap().insert("old".into());
    let snapshot_guard = store.lock().unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let release = Arc::new(Barrier::new(2));
    *q.claim_probe.lock().unwrap() = Some((entered_tx, release.clone()));
    let claim_queue = q.clone();
    let claim = std::thread::spawn(move || claim_queue.resume());
    if let Err(error) = entered_rx.recv_timeout(TEST_TIMEOUT) {
        drop(snapshot_guard);
        entered_rx
            .recv_timeout(TEST_TIMEOUT)
            .expect("claim probe did not fire after Store release");
        release.wait();
        claim.join().unwrap();
        panic!("claim probe did not fire before unfinished snapshot: {error}");
    }
    let active_locked = matches!(q.active.try_lock(), Err(TryLockError::WouldBlock));
    if !active_locked {
        drop(snapshot_guard);
        release.wait();
        claim.join().unwrap();
        panic!("active lock was not held before unfinished snapshot");
    }

    let (cleanup_started_tx, cleanup_started_rx) = std::sync::mpsc::sync_channel(0);
    let cleanup_queue = q.clone();
    let cleanup = std::thread::spawn(move || {
        let active_locked = matches!(
            cleanup_queue.active.try_lock(),
            Err(TryLockError::WouldBlock)
        );
        cleanup_started_tx.send(active_locked).unwrap();
        cleanup_queue.active.lock().unwrap().remove("old");
    });
    let cleanup_waited_for_active = cleanup_started_rx.recv_timeout(TEST_TIMEOUT).unwrap();
    drop(snapshot_guard);
    release.wait();
    claim.join().unwrap();
    cleanup.join().unwrap();
    assert!(
        cleanup_waited_for_active,
        "active lock was released before registration"
    );
}

struct TrackingOcr {
    active: Arc<AtomicU32>,
    max_active: Arc<AtomicU32>,
}

struct CallbackStalledOcr {
    release_callback: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl OcrService for CallbackStalledOcr {
    fn id(&self) -> ServiceId {
        ServiceId::Vl16
    }

    async fn parse(
        &self,
        _input: &InputDoc,
        _options: &ParseOptions,
        progress: ProgressFn,
    ) -> Result<RecognitionResult, OcrError> {
        self.release_callback.notified().await;
        progress(1, 1);
        std::future::pending().await
    }
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
                QueueEvent::Progress { stage, .. } if stage == "processing" => processing = true,
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
async fn disabled_result_persistence_keeps_lifecycle_without_history() {
    let (queue, store, mut events) =
        setup_service_with_persistence(Arc::new(MockOcr::new()), 1, false).await;
    queue.submit(NewTask {
        id: "private".into(),
        service: ServiceId::Vl16,
        input_path: "sensitive.png".into(),
        options_json: "{}".into(),
    });

    let (uploading, processing) = timeout(TEST_TIMEOUT, async {
        let (mut uploading, mut processing) = (false, false);
        loop {
            match events.recv().await.expect("queue event channel closed") {
                QueueEvent::Progress { stage, .. } if stage == "uploading" => uploading = true,
                QueueEvent::Progress { stage, .. } if stage == "processing" => processing = true,
                QueueEvent::Done { id } => {
                    assert_eq!(id, "private");
                    return (uploading, processing);
                }
                QueueEvent::Progress { .. } => {}
                _ => panic!("private task should succeed"),
            }
        }
    })
    .await
    .expect("timed out waiting for private task");

    assert!(uploading && processing);
    let store = store.lock().unwrap();
    assert_eq!(store.list_tasks(Some("done")).unwrap().len(), 1);
    assert!(store.get_result("private").unwrap().is_none());
    assert!(store.search_history("Mock").unwrap().is_empty());
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
async fn manual_retry_only_restarts_requested_failed_task() {
    let (queue, store, mut events) = setup(MockOcr::failing(2, OcrError::Auth), 1).await;
    for id in ["retry-me", "leave-failed"] {
        queue.submit(NewTask {
            id: id.into(),
            service: ServiceId::Vl16,
            input_path: format!("{id}.png"),
            options_json: "{}".into(),
        });
    }
    for _ in 0..2 {
        assert!(matches!(
            terminal(&mut events).await,
            QueueEvent::Failed { .. }
        ));
    }

    queue.retry("retry-me").unwrap();
    assert!(matches!(
        terminal(&mut events).await,
        QueueEvent::Done { id } if id == "retry-me"
    ));

    let store = store.lock().unwrap();
    assert_eq!(store.list_tasks(Some("done")).unwrap()[0].id, "retry-me");
    assert_eq!(
        store.list_tasks(Some("failed")).unwrap()[0].id,
        "leave-failed"
    );
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn canceled_is_last_event_after_accepted_progress() {
    let release_callback = Arc::new(tokio::sync::Notify::new());
    let (q, _store, mut rx) = setup_service(
        Arc::new(CallbackStalledOcr {
            release_callback: release_callback.clone(),
        }),
        1,
    )
    .await;
    q.submit(NewTask {
        id: "t1".into(),
        service: ServiceId::Vl16,
        input_path: "a.png".into(),
        options_json: "{}".into(),
    });
    wait_for_stage(&mut rx, "uploading").await;

    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let release = Arc::new(Barrier::new(2));
    *q.event_probe.lock().unwrap() = Some((entered_tx, release.clone()));
    release_callback.notify_one();
    entered_rx.recv_timeout(TEST_TIMEOUT).unwrap();
    let store_locked = matches!(q.store.try_lock(), Err(TryLockError::WouldBlock));

    let ready = Arc::new(Barrier::new(2));
    let cancel_queue = q.clone();
    let cancel_ready = ready.clone();
    let cancel = std::thread::spawn(move || {
        cancel_ready.wait();
        cancel_queue.cancel("t1");
    });
    ready.wait();
    release.wait();
    cancel.join().unwrap();
    assert!(store_locked, "Store lock was released before event send");

    match terminal(&mut rx).await {
        QueueEvent::Canceled { id } => assert_eq!(id, "t1"),
        _ => panic!("task should be canceled"),
    }
    assert!(rx.try_recv().is_err(), "event arrived after cancellation");
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_claim_waits_for_active_cleanup_before_snapshot() {
    let service = MockOcr::new();
    let service_probe = service.clone();
    let (q, store, mut rx) = setup(service, 1).await;
    store
        .lock()
        .unwrap()
        .insert_task(&NewTask {
            id: "old".into(),
            service: ServiceId::Vl16,
            input_path: "old.png".into(),
            options_json: "{}".into(),
        })
        .unwrap();
    assert_resume_claim_handshake(&q, &store);
    assert_eq!(service_probe.call_count(), 0);
    assert!(rx.try_recv().is_err(), "resume started a duplicate worker");
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
