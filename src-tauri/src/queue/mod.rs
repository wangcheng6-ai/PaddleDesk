use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use chrono::Local;
use serde::Serialize;
use tokio::sync::{mpsc, Semaphore};

use crate::{
    api::{InputDoc, OcrService, ParseOptions, ProgressFn},
    model::{OcrError, RecognitionResult, ServiceId},
    storage::{AdmittedTask, NewTask, Store},
};

const MAX_RETRIES: u32 = 3;

#[cfg(test)]
type TestProbe = (std::sync::mpsc::SyncSender<()>, Arc<std::sync::Barrier>);

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
    persist_results: AtomicBool,
    active: Mutex<HashSet<String>>,
    #[cfg(test)]
    event_probe: Arc<Mutex<Option<TestProbe>>>,
    #[cfg(test)]
    claim_probe: Mutex<Option<TestProbe>>,
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
            active: Mutex::new(HashSet::new()),
            #[cfg(test)]
            event_probe: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            claim_probe: Mutex::new(None),
        })
    }

    pub fn submit(self: &Arc<Self>, task: NewTask) {
        let persist_result = self.persist_results.load(Ordering::Acquire);
        let inserted = self.lock_store().and_then(|store| {
            store
                .insert_task(&task, persist_result)
                .map_err(storage_error)
        });
        if let Err(error) = inserted {
            self.emit_failed(&task.id, error);
            return;
        }
        self.spawn(AdmittedTask {
            task,
            persist_result,
        });
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
            .filter(|task| active.insert(task.task.id.clone()))
            .collect())
    }

    pub fn cancel(&self, id: &str) {
        let error = match self.lock_store() {
            Ok(store) => match store.cancel_task(id) {
                Ok(true) => {
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
        let task = {
            let store = self.lock_store()?;
            store.retry_task(id).map_err(storage_error)?
        }
        .ok_or_else(|| OcrError::Parse("task is not failed or does not exist".into()))?;
        self.spawn(task);
        Ok(())
    }

    fn spawn(self: &Arc<Self>, task: AdmittedTask) {
        let registered = self
            .active
            .lock()
            .map(|mut active| active.insert(task.task.id.clone()));
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
        tokio::spawn(async move { queue.run(task).await });
    }

    async fn run(self: Arc<Self>, task: AdmittedTask) {
        let terminal = self.work(&task).await;
        if let Ok(mut active) = self.active.lock() {
            active.remove(&task.task.id);
        }
        if let Some(event) = terminal {
            self.emit(event);
        }
    }

    async fn work(&self, task: &AdmittedTask) -> Option<QueueEvent> {
        let Ok(_permit) = self.semaphore.acquire().await else {
            return self.fail(&task.task.id, OcrError::Parse("queue stopped".into()));
        };
        match self.progress(&task.task.id, "uploading", 0, 0) {
            Ok(true) => {}
            Ok(false) => return None,
            Err(error) => return self.fail(&task.task.id, error),
        }
        match self.parse_with_retry(&task.task).await {
            Ok(result) => self.finish(task, &result),
            Err(error) => self.fail(&task.task.id, error),
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

    fn finish(&self, task: &AdmittedTask, result: &RecognitionResult) -> Option<QueueEvent> {
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
                    result,
                    &today,
                    task.task.service,
                    task.persist_result,
                ) {
                    Ok(true) => {
                        return Some(QueueEvent::Done {
                            id: task.task.id.clone(),
                        })
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
        let error = match self.lock_store() {
            Ok(store) => match store.update_status_if_active(id, "failed", None, Some(&error)) {
                Ok(true) => {
                    return Some(QueueEvent::Failed {
                        id: id.into(),
                        error,
                    })
                }
                Ok(false) => return None,
                Err(error) => storage_error(error),
            },
            Err(error) => error,
        };
        Some(QueueEvent::Failed {
            id: id.into(),
            error,
        })
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

    fn emit_failed(&self, id: &str, error: OcrError) {
        self.emit(QueueEvent::Failed {
            id: id.into(),
            error,
        });
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
mod tests;
