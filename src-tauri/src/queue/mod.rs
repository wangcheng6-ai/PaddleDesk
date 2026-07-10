use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use chrono::Local;
use serde::Serialize;
use tokio::sync::{mpsc, watch, Semaphore};

use crate::{
    api::{InputDoc, OcrService, ParseCheckpoint, ParseOptions, ProgressFn},
    model::{OcrError, RecognitionResult, ServiceId},
    storage::{AdmittedTask, NewTask, Store, TaskRow},
};

const MAX_RETRIES: u32 = 3;

#[cfg(test)]
type TestProbe = (std::sync::mpsc::SyncSender<()>, Arc<std::sync::Barrier>);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueueEvent {
    Submitted {
        task: TaskRow,
    },
    Progress {
        id: String,
        stage: String,
        page: u32,
        total: u32,
    },
    Done {
        id: String,
        result: RecognitionResult,
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
    persist_results: AtomicBool,
    active: Mutex<HashMap<String, watch::Sender<bool>>>,
    session_results: Mutex<HashMap<String, RecognitionResult>>,
    #[cfg(test)]
    event_probe: Arc<Mutex<Option<TestProbe>>>,
    #[cfg(test)]
    claim_probe: Mutex<Option<TestProbe>>,
    #[cfg(test)]
    submit_probe: Mutex<Option<TestProbe>>,
    #[cfg(test)]
    terminal_probe: Mutex<Option<TestProbe>>,
    #[cfg(test)]
    cancel_probe: Mutex<Option<TestProbe>>,
    #[cfg(test)]
    retry_probe: Mutex<Option<TestProbe>>,
}

impl Queue {
    pub fn new(
        store: Arc<Mutex<Store>>,
        services: HashMap<ServiceId, Arc<dyn OcrService>>,
        concurrency: usize,
        events: mpsc::UnboundedSender<QueueEvent>,
        retry_base: Duration,
        persist_results: bool,
    ) -> Arc<Queue> {
        Arc::new(Self {
            store,
            services,
            semaphore: Arc::new(Semaphore::new(concurrency)),
            events,
            retry_base,
            persist_results: AtomicBool::new(persist_results),
            active: Mutex::new(HashMap::new()),
            session_results: Mutex::new(HashMap::new()),
            #[cfg(test)]
            event_probe: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            claim_probe: Mutex::new(None),
            #[cfg(test)]
            submit_probe: Mutex::new(None),
            #[cfg(test)]
            terminal_probe: Mutex::new(None),
            #[cfg(test)]
            cancel_probe: Mutex::new(None),
            #[cfg(test)]
            retry_probe: Mutex::new(None),
        })
    }

    pub fn submit(self: &Arc<Self>, task: NewTask) -> Result<(), OcrError> {
        self.submit_in_batch(task, None)
    }

    pub fn submit_in_batch(
        self: &Arc<Self>,
        task: NewTask,
        batch_id: Option<&str>,
    ) -> Result<(), OcrError> {
        #[cfg(test)]
        if matches!(
            self.store.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ) {
            hit_probe(&self.submit_probe);
        }
        let persist_result = {
            let store = match self.lock_store() {
                Ok(store) => store,
                Err(error) => {
                    self.emit_failed(&task.id, error.clone());
                    return Err(error);
                }
            };
            let persist_result = self.persist_results.load(Ordering::Acquire);
            if let Err(error) = store.insert_task_in_batch(&task, persist_result, batch_id) {
                let error = storage_error(error);
                self.emit_failed(&task.id, error.clone());
                return Err(error);
            }
            let submitted = store
                .task(&task.id)
                .map_err(storage_error)?
                .ok_or_else(|| storage_error("submitted task missing from storage"))?;
            self.emit(QueueEvent::Submitted { task: submitted });
            persist_result
        };
        self.spawn(AdmittedTask {
            task,
            persist_result,
            upstream_job_ids: Vec::new(),
        });
        Ok(())
    }

    pub fn set_persist_results(&self, persist_results: bool) {
        self.persist_results
            .store(persist_results, Ordering::Release);
    }

    pub fn resume(self: &Arc<Self>) {
        let Ok(tasks) = self.claim_unfinished() else {
            return;
        };
        for task in tasks {
            self.spawn_registered(task);
        }
    }

    fn claim_unfinished(&self) -> Result<Vec<AdmittedTask>, OcrError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| storage_error("queue state lock poisoned"))?;
        #[cfg(test)]
        hit_probe(&self.claim_probe);
        let store = self.lock_store()?;
        let tasks = store.unfinished_tasks().map_err(storage_error)?;
        Ok(tasks
            .into_iter()
            .filter(|task| register(&mut active, &task.task.id))
            .collect())
    }

    pub fn cancel(&self, id: &str) {
        #[cfg(test)]
        if matches!(
            self.active.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ) {
            hit_probe(&self.cancel_probe);
        }
        let active = match self.active.lock() {
            Ok(active) => active,
            Err(_) => {
                self.emit_failed(id, storage_error("queue state lock poisoned"));
                return;
            }
        };
        let error = match self.lock_store() {
            Ok(store) => match store.cancel_task(id) {
                Ok(true) => {
                    if let Some(cancel) = active.get(id) {
                        cancel.send_replace(true);
                    }
                    self.emit(QueueEvent::Canceled { id: id.into() });
                    return;
                }
                Ok(false) => return,
                Err(error) => storage_error(error),
            },
            Err(error) => error,
        };
        self.emit_failed(id, error);
    }

    pub fn retry(self: &Arc<Self>, id: &str) -> Result<(), OcrError> {
        #[cfg(test)]
        if matches!(
            self.active.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ) {
            hit_probe(&self.retry_probe);
        }
        let task = {
            let mut active = self
                .active
                .lock()
                .map_err(|_| storage_error("queue state lock poisoned"))?;
            if !register(&mut active, id) {
                return Err(OcrError::Internal("task is already active".into()));
            }
            let retried = self
                .lock_store()
                .and_then(|store| store.retry_task(id).map_err(storage_error));
            match retried {
                Ok(Some(task)) => task,
                Ok(None) => {
                    active.remove(id);
                    return Err(OcrError::Internal(
                        "task is not failed or does not exist".into(),
                    ));
                }
                Err(error) => {
                    active.remove(id);
                    return Err(error);
                }
            }
        };
        self.spawn_registered(task);
        Ok(())
    }

    fn spawn(self: &Arc<Self>, task: AdmittedTask) {
        let registered = self
            .active
            .lock()
            .map(|mut active| register(&mut active, &task.task.id));
        match registered {
            Ok(true) => {}
            Ok(false) => return,
            Err(_) => {
                self.emit_failed(&task.task.id, storage_error("queue state lock poisoned"));
                return;
            }
        }
        self.spawn_registered(task);
    }

    fn spawn_registered(self: &Arc<Self>, task: AdmittedTask) {
        let queue = Arc::clone(self);
        tauri::async_runtime::spawn(async move { queue.run(task).await });
    }

    async fn run(self: Arc<Self>, task: AdmittedTask) {
        let terminal = match self.cancel_receiver(&task.task.id) {
            Ok(mut cancel) => self.work(&task, &mut cancel).await,
            Err(error) => self.fail(&task.task.id, error),
        };
        self.finalize(&task.task.id, terminal);
    }

    pub fn get_result(&self, id: &str) -> Result<Option<RecognitionResult>, OcrError> {
        if let Some(result) = self.lock_store()?.get_result(id).map_err(storage_error)? {
            return Ok(Some(result));
        }
        self.session_results
            .lock()
            .map_err(|_| storage_error("session result cache lock poisoned"))
            .map(|results| results.get(id).cloned())
    }

    pub fn session_results(&self) -> Result<Vec<(String, RecognitionResult)>, OcrError> {
        self.session_results
            .lock()
            .map_err(|_| storage_error("session result cache lock poisoned"))
            .map(|results| {
                results
                    .iter()
                    .map(|(id, result)| (id.clone(), result.clone()))
                    .collect()
            })
    }

    pub fn remove_session_result(&self, id: &str) -> Result<bool, OcrError> {
        self.session_results
            .lock()
            .map_err(|_| storage_error("session result cache lock poisoned"))
            .map(|mut results| results.remove(id).is_some())
    }

    async fn work(
        &self,
        task: &AdmittedTask,
        cancel: &mut watch::Receiver<bool>,
    ) -> Option<QueueEvent> {
        let permit = tokio::select! {
            _ = canceled(cancel) => return None,
            permit = self.semaphore.acquire() => permit,
        };
        let Ok(_permit) = permit else {
            return self.fail(&task.task.id, OcrError::Internal("queue stopped".into()));
        };
        match self.progress(&task.task.id, "uploading", 0, 0) {
            Ok(true) => {}
            Ok(false) => return None,
            Err(error) => return self.fail(&task.task.id, error),
        }
        let result = tokio::select! {
            _ = canceled(cancel) => return None,
            result = self.parse_with_retry(task) => result,
        };
        match result {
            Ok(result) => self.finish(task, result),
            Err(error) => self.fail(&task.task.id, error),
        }
    }

    async fn parse_with_retry(
        &self,
        admitted: &AdmittedTask,
    ) -> Result<RecognitionResult, OcrError> {
        let task = &admitted.task;
        let service = self
            .services
            .get(&task.service)
            .cloned()
            .ok_or_else(|| OcrError::Internal("OCR service is not configured".into()))?;
        let options: ParseOptions = serde_json::from_str(&task.options_json)
            .map_err(|error| OcrError::Internal(error.to_string()))?;
        let input = InputDoc {
            path: task.input_path.clone().into(),
        };
        let store = Arc::clone(&self.store);
        let task_id = task.id.clone();
        let checkpoint = ParseCheckpoint::new(
            admitted.upstream_job_ids.clone(),
            Arc::new(move |job_ids| {
                let store = store
                    .lock()
                    .map_err(|_| storage_error("store lock poisoned"))?;
                match store
                    .set_upstream_job_ids(&task_id, job_ids)
                    .map_err(storage_error)?
                {
                    true => Ok(()),
                    false => Err(OcrError::Internal("task is no longer active".into())),
                }
            }),
        );
        for attempt in 0..=MAX_RETRIES {
            let progress_error = Arc::new(Mutex::new(None));
            let result = service
                .parse_resumable(
                    &input,
                    &options,
                    self.progress_callback(task.id.clone(), progress_error.clone()),
                    checkpoint.clone(),
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
        #[cfg(test)]
        let event_probe = Arc::clone(&self.event_probe);
        Box::new(move |page, total| {
            let Ok(store) = store.lock() else {
                set_progress_error(&progress_error, storage_error("store lock poisoned"));
                return;
            };
            match store.update_status_if_active(&id, "processing", Some((page, total)), None) {
                Ok(true) => {
                    #[cfg(test)]
                    hit_probe(&event_probe);
                    let _ = events.send(QueueEvent::Progress {
                        id: id.clone(),
                        stage: "processing".into(),
                        page,
                        total,
                    });
                }
                Ok(false) => {}
                Err(error) => set_progress_error(&progress_error, storage_error(error)),
            }
        })
    }

    fn progress(&self, id: &str, stage: &str, page: u32, total: u32) -> Result<bool, OcrError> {
        let store = self.lock_store()?;
        let updated = store
            .update_status_if_active(id, stage, Some((page, total)), None)
            .map_err(storage_error)?;
        if updated {
            self.emit(QueueEvent::Progress {
                id: id.into(),
                stage: stage.into(),
                page,
                total,
            });
        }
        Ok(updated)
    }

    fn finish(&self, task: &AdmittedTask, result: RecognitionResult) -> Option<QueueEvent> {
        let file_name = Path::new(&task.task.input_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&task.task.input_path);
        let today = Local::now().date_naive().to_string();
        let error = match self.lock_store() {
            Ok(store) => {
                match store.complete_task(
                    &task.task.id,
                    file_name,
                    &result,
                    &today,
                    task.task.service,
                    task.persist_result,
                ) {
                    Ok(true) => {
                        if !task.persist_result {
                            let cached = self.session_results.lock().map(|mut results| {
                                results.insert(task.task.id.clone(), result.clone());
                            });
                            if cached.is_err() {
                                return self.fail(
                                    &task.task.id,
                                    storage_error("session result cache lock poisoned"),
                                );
                            }
                        }
                        return Some(QueueEvent::Done {
                            id: task.task.id.clone(),
                            result,
                        });
                    }
                    Ok(false) => return None,
                    Err(error) => storage_error(error),
                }
            }
            Err(error) => error,
        };
        self.fail(&task.task.id, error)
    }

    fn fail(&self, id: &str, error: OcrError) -> Option<QueueEvent> {
        Some(QueueEvent::Failed {
            id: id.into(),
            error,
        })
    }

    fn finalize(&self, task_id: &str, terminal: Option<QueueEvent>) {
        let mut active = match self.active.lock() {
            Ok(active) => active,
            Err(_) => {
                self.emit_failed(task_id, storage_error("queue state lock poisoned"));
                return;
            }
        };
        let terminal = match terminal {
            Some(QueueEvent::Failed { id, error }) => match self.lock_store() {
                Ok(store) => match store.update_status_if_active(&id, "failed", None, Some(&error))
                {
                    Ok(true) => Some(QueueEvent::Failed { id, error }),
                    Ok(false) => None,
                    Err(error) => Some(QueueEvent::Failed {
                        id,
                        error: storage_error(error),
                    }),
                },
                Err(error) => Some(QueueEvent::Failed { id, error }),
            },
            terminal => terminal,
        };
        #[cfg(test)]
        hit_probe(&self.terminal_probe);
        active.remove(task_id);
        if let Some(event) = terminal {
            self.emit(event);
        }
    }

    fn emit(&self, event: QueueEvent) {
        #[cfg(test)]
        hit_probe(&self.event_probe);
        let _ = self.events.send(event);
    }

    fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, Store>, OcrError> {
        self.store
            .lock()
            .map_err(|_| storage_error("store lock poisoned"))
    }

    fn cancel_receiver(&self, id: &str) -> Result<watch::Receiver<bool>, OcrError> {
        self.active
            .lock()
            .map_err(|_| storage_error("queue state lock poisoned"))?
            .get(id)
            .map(watch::Sender::subscribe)
            .ok_or_else(|| OcrError::Internal("task is not active".into()))
    }

    fn emit_failed(&self, id: &str, error: OcrError) {
        self.emit(QueueEvent::Failed {
            id: id.into(),
            error,
        });
    }
}

fn register(active: &mut HashMap<String, watch::Sender<bool>>, id: &str) -> bool {
    if active.contains_key(id) {
        return false;
    }
    let (cancel, _) = watch::channel(false);
    active.insert(id.into(), cancel);
    true
}

async fn canceled(cancel: &mut watch::Receiver<bool>) {
    if *cancel.borrow() {
        return;
    }
    while cancel.changed().await.is_ok() {
        if *cancel.borrow() {
            return;
        }
    }
}

#[cfg(test)]
fn hit_probe(probe: &Mutex<Option<TestProbe>>) {
    if let Some((entered, release)) = probe.lock().unwrap().take() {
        entered.send(()).unwrap();
        release.wait();
    }
}

fn is_retryable(error: &OcrError) -> bool {
    matches!(
        error,
        OcrError::RateLimited(_) | OcrError::Network(_) | OcrError::Server(_)
    )
}

fn storage_error(error: impl std::fmt::Display) -> OcrError {
    OcrError::Internal(format!("storage error: {error}"))
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
mod tests;
