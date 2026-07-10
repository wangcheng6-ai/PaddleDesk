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
    validate_setting_keys(&map)?;
    let store = lock_store(&state)?;
    for (key, value) in map {
        store
            .set_setting(&key, &value)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
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

    #[tokio::test]
    async fn create_tasks_submits_every_path() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(
            Store::open(&directory.path().join("commands.db")).unwrap(),
        ));
        let mut services: HashMap<ServiceId, Arc<dyn OcrService>> = HashMap::new();
        services.insert(ServiceId::Vl16, Arc::new(MockOcr::new()));
        let (sender, mut events) = mpsc::unbounded_channel();
        let queue = Queue::new(
            store.clone(),
            services,
            1,
            sender,
            Duration::from_millis(1),
            true,
        );

        let ids = create_tasks_with_queue(
            vec!["one.png".into(), "two.png".into()],
            ServiceId::Vl16,
            ParseOptions::default(),
            &queue,
        )
        .unwrap();

        for _ in 0..2 {
            timeout(Duration::from_secs(1), async {
                loop {
                    if matches!(events.recv().await, Some(QueueEvent::Done { .. })) {
                        break;
                    }
                }
            })
            .await
            .unwrap();
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
