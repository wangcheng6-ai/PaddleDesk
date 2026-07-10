use std::{
    collections::HashMap,
    sync::{Arc, MutexGuard},
};

use tauri::State;

use crate::{
    api::ParseOptions,
    export,
    model::ServiceId,
    queue::Queue,
    storage::{HistoryRow, NewTask, Store, TaskRow, UsageRow},
    AppState,
};

fn create_tasks_with_queue(
    paths: Vec<String>,
    service: ServiceId,
    options: ParseOptions,
    queue: &Arc<Queue>,
) -> Result<Vec<String>, String> {
    let options_json = serde_json::to_string(&options).map_err(|error| error.to_string())?;
    let mut ids = Vec::with_capacity(paths.len());
    for input_path in paths {
        let id = uuid::Uuid::new_v4().to_string();
        queue.submit(NewTask {
            id: id.clone(),
            service,
            input_path,
            options_json: options_json.clone(),
        });
        ids.push(id);
    }
    Ok(ids)
}

fn validate_setting_keys(settings: &HashMap<String, String>) -> Result<(), String> {
    if let Some(key) = settings.keys().find(|key| is_secret_setting_key(key)) {
        return Err(format!(
            "setting '{key}' must use Windows Credential Manager"
        ));
    }
    for (key, value) in settings {
        let valid = match key.as_str() {
            "language" => matches!(value.as_str(), "system" | "zh-CN" | "en"),
            "theme" => matches!(value.as_str(), "system" | "light" | "dark"),
            "proxy_mode" => matches!(value.as_str(), "system" | "custom" | "direct"),
            "concurrency" => matches!(value.as_str(), "1" | "2" | "3" | "4"),
            "privacy_mode" | "autostart" => matches!(value.as_str(), "0" | "1"),
            "default_service" => {
                matches!(value.as_str(), "vl16" | "pp_ocr_v6" | "structure_v3")
            }
            _ => true,
        };
        if !valid {
            return Err(format!("invalid value for setting '{key}'"));
        }
    }
    Ok(())
}

fn apply_settings(
    settings: HashMap<String, String>,
    store: &Store,
    queue: &Queue,
) -> Result<(), String> {
    validate_setting_keys(&settings)?;
    let persist_results = settings.get("privacy_mode").map(|value| value != "1");
    store
        .set_settings(&settings)
        .map_err(|error| error.to_string())?;
    if let Some(persist_results) = persist_results {
        queue.set_persist_results(persist_results);
    }
    Ok(())
}

fn validate_token_with(
    token: &str,
    save: impl FnOnce(&str) -> Result<(), String>,
) -> Result<bool, String> {
    save(token)?;
    Ok(true)
}

fn is_secret_setting_key(key: &str) -> bool {
    key.to_ascii_lowercase().contains("token")
}

fn lock_store(state: &AppState) -> Result<MutexGuard<'_, Store>, String> {
    state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())
}

#[tauri::command]
pub fn create_tasks(
    paths: Vec<String>,
    service: ServiceId,
    options: ParseOptions,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    create_tasks_with_queue(paths, service, options, &state.queue)
}

#[tauri::command]
pub fn list_tasks(
    status: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<TaskRow>, String> {
    lock_store(&state)?
        .list_tasks(status.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn cancel_task(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.queue.cancel(&id);
    Ok(())
}

#[tauri::command]
pub fn retry_task(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.queue.retry(&id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_result(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Option<crate::model::RecognitionResult>, String> {
    lock_store(&state)?
        .get_result(&task_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn export_result(
    task_id: String,
    format: String,
    path: String,
    block_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let result = lock_store(&state)?
        .get_result(&task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "recognition result not found".to_string())?;
    let output = export::export(&result, &format, block_id.as_deref())?;
    std::fs::write(&path, output).map_err(|error| error.to_string())?;
    Ok(path)
}

#[tauri::command]
pub fn search_history(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<HistoryRow>, String> {
    lock_store(&state)?
        .search_history(&query)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_usage(days: u32, state: State<'_, AppState>) -> Result<Vec<UsageRow>, String> {
    lock_store(&state)?
        .usage_since(days)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<HashMap<String, String>, String> {
    let mut settings = lock_store(&state)?
        .get_settings()
        .map_err(|error| error.to_string())?;
    settings.retain(|key, _| !is_secret_setting_key(key));
    Ok(settings)
}

#[tauri::command]
pub fn set_settings(
    map: HashMap<String, String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let store = lock_store(&state)?;
    apply_settings(map, &store, &state.queue)
}

#[tauri::command]
pub fn validate_token(token: String) -> Result<bool, String> {
    validate_token_with(&token, save_token)
}

#[cfg(target_os = "windows")]
fn save_token(token: &str) -> Result<(), String> {
    let entry = keyring::Entry::new("cc.ccwu.paddledesk", "paddleocr_access_token")
        .map_err(|error| format!("credential store unavailable: {error}"))?;
    entry
        .set_password(token)
        .map_err(|error| format!("credential store write failed: {error}"))
}

#[cfg(not(target_os = "windows"))]
fn save_token(_token: &str) -> Result<(), String> {
    Err("Windows Credential Manager is unavailable".into())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use tokio::{sync::mpsc, time::timeout};

    use super::*;
    use crate::{
        api::{mock::MockOcr, OcrService, ParseOptions},
        model::ServiceId,
        queue::{Queue, QueueEvent},
        storage::Store,
    };

    fn test_queue(
        persist_results: bool,
    ) -> (
        tempfile::TempDir,
        Arc<Mutex<Store>>,
        Arc<Queue>,
        mpsc::UnboundedReceiver<QueueEvent>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(
            Store::open(&directory.path().join("commands.db")).unwrap(),
        ));
        let services: HashMap<ServiceId, Arc<dyn OcrService>> = HashMap::from([(
            ServiceId::Vl16,
            Arc::new(MockOcr::new()) as Arc<dyn OcrService>,
        )]);
        let (sender, events) = mpsc::unbounded_channel();
        let queue = Queue::new(
            store.clone(),
            services,
            1,
            sender,
            Duration::from_millis(1),
            persist_results,
        );
        (directory, store, queue, events)
    }

    async fn wait_done(events: &mut mpsc::UnboundedReceiver<QueueEvent>) -> String {
        timeout(Duration::from_secs(1), async {
            loop {
                if let Some(QueueEvent::Done { id }) = events.recv().await {
                    return id;
                }
            }
        })
        .await
        .unwrap()
    }

    #[test]
    fn validate_token_writes_injected_store_before_returning_true() {
        let mut stored = None;

        let valid = validate_token_with("test-secret", |token| {
            stored = Some(token.to_string());
            Ok(())
        })
        .unwrap();

        assert!(valid);
        assert_eq!(stored.as_deref(), Some("test-secret"));
    }

    #[test]
    fn validate_token_propagates_credential_store_failure() {
        let result = validate_token_with("test-secret", |_| Err("store unavailable".into()));

        assert_eq!(result.unwrap_err(), "store unavailable");
    }

    #[test]
    fn settings_reject_token_keys_before_storage() {
        let secret = HashMap::from([("access_token".to_string(), "test-secret".to_string())]);
        let ordinary = HashMap::from([("theme".to_string(), "dark".to_string())]);

        assert!(validate_setting_keys(&secret).is_err());
        assert!(validate_setting_keys(&ordinary).is_ok());
    }

    #[test]
    fn settings_reject_invalid_enum_values() {
        for invalid in [
            HashMap::from([("language".into(), "fr".into())]),
            HashMap::from([("theme".into(), "sepia".into())]),
            HashMap::from([("proxy_mode".into(), "auto".into())]),
            HashMap::from([("concurrency".into(), "8".into())]),
        ] {
            assert!(validate_setting_keys(&invalid).is_err());
        }
    }

    #[tokio::test]
    async fn privacy_setting_disables_results_but_keeps_lifecycle_and_usage() {
        let (_directory, store, queue, mut events) = test_queue(true);
        apply_settings(
            HashMap::from([("privacy_mode".into(), "1".into())]),
            &store.lock().unwrap(),
            &queue,
        )
        .unwrap();
        queue.submit(NewTask {
            id: "private".into(),
            service: ServiceId::Vl16,
            input_path: "private.png".into(),
            options_json: "{}".into(),
        });
        assert_eq!(wait_done(&mut events).await, "private");

        let store = store.lock().unwrap();
        assert_eq!(
            store.get_setting("privacy_mode").unwrap().as_deref(),
            Some("1")
        );
        assert_eq!(store.list_tasks(Some("done")).unwrap().len(), 1);
        assert!(store.get_result("private").unwrap().is_none());
        assert!(store.search_history("Mock").unwrap().is_empty());
        assert_eq!(store.usage_since(1).unwrap()[0].pages, 1);
    }

    #[tokio::test]
    async fn failed_settings_batch_keeps_database_and_queue_policy_aligned() {
        let (directory, store, queue, mut events) = test_queue(false);
        rusqlite::Connection::open(directory.path().join("commands.db"))
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_privacy_setting BEFORE INSERT ON settings
                 WHEN NEW.key = 'privacy_mode'
                 BEGIN SELECT RAISE(ABORT, 'forced settings failure'); END;",
            )
            .unwrap();
        let result = apply_settings(
            HashMap::from([
                ("theme".into(), "dark".into()),
                ("privacy_mode".into(), "0".into()),
            ]),
            &store.lock().unwrap(),
            &queue,
        );
        assert!(result.is_err());
        assert!(store.lock().unwrap().get_settings().unwrap().is_empty());

        queue.submit(NewTask {
            id: "still-private".into(),
            service: ServiceId::Vl16,
            input_path: "private.png".into(),
            options_json: "{}".into(),
        });
        assert_eq!(wait_done(&mut events).await, "still-private");
        assert!(store
            .lock()
            .unwrap()
            .get_result("still-private")
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn create_tasks_submits_every_path() {
        let (_directory, store, queue, mut events) = test_queue(true);

        let ids = create_tasks_with_queue(
            vec!["one.png".into(), "two.png".into()],
            ServiceId::Vl16,
            ParseOptions::default(),
            &queue,
        )
        .unwrap();

        for _ in 0..2 {
            wait_done(&mut events).await;
        }
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        assert_eq!(
            store
                .lock()
                .unwrap()
                .list_tasks(Some("done"))
                .unwrap()
                .len(),
            2
        );
    }
}
