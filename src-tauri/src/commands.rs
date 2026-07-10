use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::{
    api::{
        credentials,
        paddle::{PaddleOcr, BASE_URL},
        ParseOptions,
    },
    capture, export,
    model::{OcrError, ServiceId},
    queue::Queue,
    storage::{HistoryRow, NewTask, ResultSummary, Store, TaskRow, UsageRow},
    AppState,
};

#[derive(Debug, Clone, Serialize)]
pub struct CreatedBatch {
    pub batch_id: String,
    pub task_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CredentialStatus {
    pub configured: bool,
    pub last_four: Option<String>,
}

fn create_tasks_with_queue(
    paths: Vec<String>,
    service: ServiceId,
    options: ParseOptions,
    queue: &Arc<Queue>,
) -> Result<CreatedBatch, String> {
    let options_json = serde_json::to_string(&options).map_err(|error| error.to_string())?;
    let batch_id = uuid::Uuid::new_v4().to_string();
    let mut ids = Vec::with_capacity(paths.len());
    for input_path in paths {
        let id = uuid::Uuid::new_v4().to_string();
        // Admission failures are emitted as terminal queue events; continue the batch so
        // later paths are still submitted and every generated ID keeps its original meaning.
        let _admission = queue.submit_in_batch(
            NewTask {
                id: id.clone(),
                service,
                input_path,
                options_json: options_json.clone(),
            },
            Some(&batch_id),
        );
        ids.push(id);
    }
    Ok(CreatedBatch {
        batch_id,
        task_ids: ids,
    })
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
            "save_history" | "autostart" | "onboarding_complete" => {
                matches!(value.as_str(), "0" | "1")
            }
            "current_service" => {
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
    let persist_results = settings.get("save_history").map(|value| value == "1");
    store
        .set_settings(&settings)
        .map_err(|error| error.to_string())?;
    if let Some(persist_results) = persist_results {
        queue.set_persist_results(persist_results);
    }
    Ok(())
}

async fn validate_token_with(
    token: &str,
    probe: impl Future<Output = Result<bool, OcrError>>,
    save: impl FnOnce(&str) -> Result<(), String>,
) -> Result<bool, String> {
    let valid = probe.await.map_err(|error| error.to_string())?;
    if valid {
        save(token)?;
    }
    Ok(valid)
}

fn is_secret_setting_key(key: &str) -> bool {
    key.to_ascii_lowercase().contains("token")
}

fn ensure_token_configured() -> Result<(), String> {
    credentials::load_token()
        .map_err(|error| error.to_string())
        .and_then(|token| {
            if token.trim().is_empty() {
                Err(OcrError::Auth.to_string())
            } else {
                Ok(())
            }
        })
}

fn normalize_shortcut(shortcut: &str) -> Result<String, String> {
    let parts = shortcut
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() == 1 {
        let key = parts[0].to_ascii_uppercase();
        if is_function_key(&key) {
            return Ok(key);
        }
        return Err("single-key shortcuts must be F1-F12".into());
    }
    if !(2..=5).contains(&parts.len()) {
        return Err("shortcut must contain 2 to 5 keys".into());
    }

    let mut modifiers = HashSet::new();
    let mut main_key = None;
    for part in parts {
        let upper = part.to_ascii_uppercase();
        let modifier = match upper.as_str() {
            "CTRL" | "CONTROL" => Some("Ctrl"),
            "ALT" => Some("Alt"),
            "SHIFT" => Some("Shift"),
            "WIN" | "WINDOWS" | "SUPER" | "META" => Some("Win"),
            _ => None,
        };
        if let Some(modifier) = modifier {
            if !modifiers.insert(modifier) {
                return Err("shortcut contains a duplicate modifier".into());
            }
            continue;
        }
        let valid_main = (upper.len() == 1
            && upper
                .chars()
                .all(|character| character.is_ascii_alphanumeric()))
            || is_function_key(&upper);
        if !valid_main || main_key.replace(upper).is_some() {
            return Err("shortcut must contain exactly one main key".into());
        }
    }
    let main_key = main_key.ok_or_else(|| "shortcut is missing a main key".to_string())?;
    if modifiers.is_empty() {
        return Err("shortcut must contain a modifier".into());
    }
    let mut normalized = ["Ctrl", "Alt", "Win", "Shift"]
        .into_iter()
        .filter(|modifier| modifiers.contains(modifier))
        .collect::<Vec<_>>();
    normalized.push(&main_key);
    Ok(normalized.join("+"))
}

fn is_function_key(key: &str) -> bool {
    key.strip_prefix('F')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=12).contains(&number))
}

fn registration_shortcut(shortcut: &str) -> String {
    shortcut.replace("Win", "Super")
}

fn lock_store(state: &AppState) -> Result<MutexGuard<'_, Store>, String> {
    state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())
}

fn task_source_path(store: &Store, task_id: &str) -> Result<String, String> {
    store
        .task_input_path(task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "task not found".to_string())
}

fn submit_image_task_with_cleanup(
    path: PathBuf,
    service: ServiceId,
    queue: &Arc<Queue>,
    capture_tasks: &Mutex<HashSet<String>>,
    copy_result: bool,
    cleanup: impl FnOnce(&Path) -> Result<bool, String>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let options_json = match serde_json::to_string(&ParseOptions::default()) {
        Ok(options_json) => options_json,
        Err(error) => {
            return Err(cleanup_submission_failure(
                &path,
                error.to_string(),
                cleanup,
            ))
        }
    };
    if copy_result {
        match capture_tasks.lock() {
            Ok(mut tasks) => {
                tasks.insert(id.clone());
            }
            Err(_) => {
                return Err(cleanup_submission_failure(
                    &path,
                    "capture state lock poisoned".into(),
                    cleanup,
                ));
            }
        }
    }
    let result = queue.submit(NewTask {
        id: id.clone(),
        service,
        input_path: path.to_string_lossy().into_owned(),
        options_json,
    });
    match result {
        Ok(()) => Ok(id),
        Err(error) => Err(cleanup_submission_failure(
            &path,
            error.to_string(),
            cleanup,
        )),
    }
}

fn cleanup_submission_failure(
    path: &Path,
    mut message: String,
    cleanup: impl FnOnce(&Path) -> Result<bool, String>,
) -> String {
    if let Err(error) = cleanup(path) {
        message.push_str("; capture cleanup failed: ");
        message.push_str(&error);
    }
    message
}

fn submit_image_task(
    app: &AppHandle,
    path: PathBuf,
    service: ServiceId,
    state: &AppState,
    copy_result: bool,
) -> Result<String, String> {
    submit_image_task_with_cleanup(
        path,
        service,
        &state.queue,
        &state.capture_tasks,
        copy_result,
        |path| capture::remove_managed(app, path),
    )
}

fn current_service(state: &AppState) -> Result<ServiceId, String> {
    let value = lock_store(state)?
        .get_setting("current_service")
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| "vl16".into());
    match value.as_str() {
        "vl16" => Ok(ServiceId::Vl16),
        "pp_ocr_v6" => Ok(ServiceId::PpOcrV6),
        "structure_v3" => Ok(ServiceId::StructureV3),
        _ => Err("invalid stored current service".into()),
    }
}

pub(crate) async fn start_capture_inner(
    app: &AppHandle,
    state: &AppState,
) -> Result<String, String> {
    ensure_token_configured()?;
    let service = current_service(state)?;
    let path = capture::select_region(app).await?;
    submit_image_task(app, path, service, state, true)
}

#[tauri::command]
pub fn create_tasks(
    paths: Vec<String>,
    service: ServiceId,
    options: ParseOptions,
    state: State<'_, AppState>,
) -> Result<CreatedBatch, String> {
    ensure_token_configured()?;
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
    state
        .queue
        .get_result(&task_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_task_source(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<tauri::ipc::Response, String> {
    let path = {
        let store = lock_store(&state)?;
        task_source_path(&store, &task_id)?
    };
    std::fs::read(path)
        .map(tauri::ipc::Response::new)
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
    let result = state
        .queue
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
pub fn list_results(
    service: ServiceId,
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<ResultSummary>, String> {
    let session_results = state
        .queue
        .session_results()
        .map_err(|error| error.to_string())?;
    let store = lock_store(&state)?;
    let mut results = store
        .list_results(service, &query)
        .map_err(|error| error.to_string())?;
    let needle = query.trim().to_lowercase();
    for (task_id, result) in session_results {
        let Some(task) = store.task(&task_id).map_err(|error| error.to_string())? else {
            continue;
        };
        if task.service != service {
            continue;
        }
        let file_name = Path::new(&task.input_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&task.input_path)
            .to_string();
        if !needle.is_empty()
            && !file_name.to_lowercase().contains(&needle)
            && !result.markdown.to_lowercase().contains(&needle)
        {
            continue;
        }
        results.push(ResultSummary {
            task_id,
            service,
            file_name,
            snippet: result.markdown.chars().take(160).collect(),
            created_at: task.created_at,
            temporary: true,
        });
    }
    results.sort_by_key(|result| std::cmp::Reverse(result.created_at));
    Ok(results)
}

#[tauri::command]
pub fn delete_result(
    task_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let path = lock_store(&state)?
        .delete_result_task(&task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "recognition result not found".to_string())?;
    state
        .queue
        .remove_session_result(&task_id)
        .map_err(|error| error.to_string())?;
    capture::remove_managed(&app, Path::new(&path))?;
    Ok(())
}

#[tauri::command]
pub fn clear_results(
    service: ServiceId,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let session_ids = list_results(service, String::new(), state.clone())?
        .into_iter()
        .filter(|result| result.temporary)
        .map(|result| result.task_id)
        .collect::<Vec<_>>();
    let paths = lock_store(&state)?
        .clear_results(service)
        .map_err(|error| error.to_string())?;
    for id in session_ids {
        state
            .queue
            .remove_session_result(&id)
            .map_err(|error| error.to_string())?;
    }
    for path in paths {
        capture::remove_managed(&app, Path::new(&path))?;
    }
    Ok(())
}

#[tauri::command]
pub fn dismiss_failed_task(
    task_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let path = lock_store(&state)?
        .dismiss_failed_task(&task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "failed task not found".to_string())?;
    capture::remove_managed(&app, Path::new(&path))?;
    Ok(())
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
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    validate_setting_keys(&map)?;
    let previous_autostart = map
        .get("autostart")
        .map(|_| capture::desktop::autostart_enabled(&app))
        .transpose()?;
    if let Some(value) = map.get("autostart") {
        capture::desktop::set_autostart(&app, value == "1")?;
    }
    let result = {
        let store = lock_store(&state)?;
        apply_settings(map.clone(), &store, &state.queue)
    };
    if let Err(error) = result {
        if let Some(previous) = previous_autostart {
            let _ = capture::desktop::set_autostart(&app, previous);
        }
        return Err(error);
    }
    if let Some(language) = map.get("language") {
        capture::desktop::refresh_tray(&app, language)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn create_task_from_clipboard(
    service: ServiceId,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    ensure_token_configured()?;
    let path = capture::read_image(&app).await?;
    submit_image_task(&app, path, service, &state, false)
}

#[tauri::command]
pub async fn start_capture(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    start_capture_inner(&app, &state).await
}

#[tauri::command]
pub async fn validate_token(token: String, state: State<'_, AppState>) -> Result<bool, String> {
    let proxy = (state.proxy)().map_err(|error| error.to_string())?;
    validate_token_with(
        &token,
        PaddleOcr::probe_token(BASE_URL, &token, proxy),
        |token| credentials::save_token(token).map_err(|error| error.to_string()),
    )
    .await
}

#[tauri::command]
pub fn get_credential_status() -> Result<CredentialStatus, String> {
    match credentials::load_token() {
        Ok(token) if !token.trim().is_empty() => Ok(CredentialStatus {
            configured: true,
            last_four: Some(
                token
                    .chars()
                    .rev()
                    .take(4)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect(),
            ),
        }),
        Ok(_) | Err(OcrError::Auth) => Ok(CredentialStatus {
            configured: false,
            last_four: None,
        }),
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
pub fn reveal_token() -> Result<String, String> {
    credentials::load_token().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_token() -> Result<(), String> {
    match credentials::delete_token() {
        Ok(()) | Err(OcrError::Auth) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
pub fn get_screenshot_hotkey(state: State<'_, AppState>) -> Result<String, String> {
    lock_store(&state)?
        .get_setting("screenshot_hotkey")
        .map_err(|error| error.to_string())
        .map(|shortcut| shortcut.unwrap_or_else(|| capture::desktop::SCREENSHOT_SHORTCUT.into()))
}

#[tauri::command]
pub fn set_screenshot_hotkey(
    shortcut: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let shortcut = normalize_shortcut(&shortcut)?;
    let previous = get_screenshot_hotkey(state.clone())?;
    let registered = registration_shortcut(&shortcut);
    let previous_available = lock_store(&state)?
        .get_setting("screenshot_hotkey_available")
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| "1".into());
    if shortcut == previous && app.global_shortcut().is_registered(registered.as_str()) {
        lock_store(&state)?
            .set_setting("screenshot_hotkey_available", "1")
            .map_err(|error| error.to_string())?;
        return Ok(shortcut);
    }
    app.global_shortcut()
        .register(registered.as_str())
        .map_err(|error| error.to_string())?;
    let updated = HashMap::from([
        ("screenshot_hotkey".into(), shortcut.clone()),
        ("screenshot_hotkey_available".into(), "1".into()),
    ]);
    if let Err(error) = lock_store(&state)?
        .set_settings(&updated)
        .map_err(|error| error.to_string())
    {
        let _ = app.global_shortcut().unregister(registered.as_str());
        return Err(error);
    }
    if shortcut == previous {
        return Ok(shortcut);
    }
    let previous_registered = registration_shortcut(&previous);
    if app
        .global_shortcut()
        .is_registered(previous_registered.as_str())
    {
        if let Err(error) = app
            .global_shortcut()
            .unregister(previous_registered.as_str())
        {
            let rollback = HashMap::from([
                ("screenshot_hotkey".into(), previous),
                ("screenshot_hotkey_available".into(), previous_available),
            ]);
            let _ = lock_store(&state)?.set_settings(&rollback);
            let _ = app.global_shortcut().unregister(registered.as_str());
            return Err(error.to_string());
        }
    }
    Ok(shortcut)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
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
                if let Some(QueueEvent::Done { id, .. }) = events.recv().await {
                    return id;
                }
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn validate_token_writes_only_after_a_valid_probe() {
        let mut stored = None;

        let valid = validate_token_with("test-secret", async { Ok(true) }, |token| {
            stored = Some(token.to_string());
            Ok(())
        })
        .await
        .unwrap();

        assert!(valid);
        assert_eq!(stored.as_deref(), Some("test-secret"));
    }

    #[tokio::test]
    async fn invalid_token_is_not_saved() {
        let mut saved = false;
        let valid = validate_token_with("test-secret", async { Ok(false) }, |_| {
            saved = true;
            Ok(())
        })
        .await
        .unwrap();

        assert!(!valid);
        assert!(!saved);
    }

    #[tokio::test]
    async fn validate_token_propagates_credential_store_failure() {
        let result = validate_token_with("test-secret", async { Ok(true) }, |_| {
            Err("store unavailable".into())
        })
        .await;

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
            HashMap::from([("onboarding_complete".into(), "yes".into())]),
        ] {
            assert!(validate_setting_keys(&invalid).is_err());
        }
    }

    #[test]
    fn screenshot_shortcuts_require_a_real_key_and_normalize_modifiers() {
        assert_eq!(normalize_shortcut("f8").unwrap(), "F8");
        assert_eq!(normalize_shortcut("win+shift+x").unwrap(), "Win+Shift+X");
        assert_eq!(normalize_shortcut("alt+ctrl+s").unwrap(), "Ctrl+Alt+S");
        assert_eq!(
            normalize_shortcut("ctrl+alt+win+shift+s").unwrap(),
            "Ctrl+Alt+Win+Shift+S"
        );
        for invalid in ["S", "Ctrl", "Ctrl+Alt", "F13", "Ctrl+S+X"] {
            assert!(normalize_shortcut(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn task_source_is_resolved_from_sqlite_instead_of_a_frontend_path() {
        let (directory, store, _queue, _events) = test_queue(true);
        let source = directory.path().join("known.pdf");
        std::fs::write(&source, b"%PDF-test").unwrap();
        store
            .lock()
            .unwrap()
            .insert_task(
                &NewTask {
                    id: "known".into(),
                    service: ServiceId::Vl16,
                    input_path: source.to_string_lossy().into_owned(),
                    options_json: "{}".into(),
                },
                true,
            )
            .unwrap();

        let path = task_source_path(&store.lock().unwrap(), "known").unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"%PDF-test");
        assert!(task_source_path(&store.lock().unwrap(), "unknown").is_err());
    }

    #[tokio::test]
    async fn save_history_off_keeps_result_in_session_and_usage_in_database() {
        let (_directory, store, queue, mut events) = test_queue(true);
        apply_settings(
            HashMap::from([("save_history".into(), "0".into())]),
            &store.lock().unwrap(),
            &queue,
        )
        .unwrap();
        queue
            .submit(NewTask {
                id: "private".into(),
                service: ServiceId::Vl16,
                input_path: "private.png".into(),
                options_json: "{}".into(),
            })
            .unwrap();
        assert_eq!(wait_done(&mut events).await, "private");

        let store = store.lock().unwrap();
        assert_eq!(
            store.get_setting("save_history").unwrap().as_deref(),
            Some("0")
        );
        assert_eq!(store.list_tasks(Some("done")).unwrap().len(), 1);
        assert!(store.get_result("private").unwrap().is_none());
        assert!(store.search_history("Mock").unwrap().is_empty());
        assert_eq!(store.usage_since(1).unwrap()[0].pages, 1);
        drop(store);
        assert!(queue.get_result("private").unwrap().is_some());
    }

    #[tokio::test]
    async fn failed_settings_batch_keeps_database_and_queue_policy_aligned() {
        let (directory, store, queue, mut events) = test_queue(false);
        store
            .lock()
            .unwrap()
            .set_setting("save_history", "0")
            .unwrap();
        rusqlite::Connection::open(directory.path().join("commands.db"))
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_save_history_setting BEFORE INSERT ON settings
                 WHEN NEW.key = 'save_history'
                 BEGIN SELECT RAISE(ABORT, 'forced settings failure'); END;",
            )
            .unwrap();
        let result = apply_settings(
            HashMap::from([
                ("theme".into(), "dark".into()),
                ("save_history".into(), "1".into()),
            ]),
            &store.lock().unwrap(),
            &queue,
        );
        assert!(result.is_err());
        assert_eq!(
            store.lock().unwrap().get_settings().unwrap(),
            HashMap::from([("save_history".into(), "0".into())])
        );

        queue
            .submit(NewTask {
                id: "still-private".into(),
                service: ServiceId::Vl16,
                input_path: "private.png".into(),
                options_json: "{}".into(),
            })
            .unwrap();
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

        let batch = create_tasks_with_queue(
            vec!["one.png".into(), "two.png".into()],
            ServiceId::Vl16,
            ParseOptions::default(),
            &queue,
        )
        .unwrap();

        for _ in 0..2 {
            wait_done(&mut events).await;
        }
        assert_eq!(batch.task_ids.len(), 2);
        assert_ne!(batch.task_ids[0], batch.task_ids[1]);
        let tasks = store.lock().unwrap().list_tasks(Some("done")).unwrap();
        assert_eq!(tasks.len(), 2);
        assert!(tasks
            .iter()
            .all(|task| task.batch_id.as_deref() == Some(batch.batch_id.as_str())));
    }

    #[tokio::test]
    async fn failed_image_submission_removes_capture_immediately() {
        let (directory, _store, queue, mut events) = test_queue(true);
        rusqlite::Connection::open(directory.path().join("commands.db"))
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_task_insert BEFORE INSERT ON tasks
                 BEGIN SELECT RAISE(ABORT, 'forced task insert failure'); END;",
            )
            .unwrap();
        let capture = directory.path().join("capture.png");
        std::fs::write(&capture, b"image").unwrap();
        let capture_tasks = Arc::new(Mutex::new(HashSet::new()));

        let result = submit_image_task_with_cleanup(
            capture.clone(),
            ServiceId::Vl16,
            &queue,
            &capture_tasks,
            true,
            |path| {
                std::fs::remove_file(path).map_err(|error| error.to_string())?;
                Ok(true)
            },
        );

        assert!(result.unwrap_err().contains("forced task insert failure"));
        assert!(!capture.exists());
        let failed_id = match events.recv().await {
            Some(QueueEvent::Failed {
                id,
                error: OcrError::Internal(message),
            }) if message.contains("forced task insert failure") => id,
            event => panic!("unexpected event: {event:?}"),
        };
        assert!(capture_tasks.lock().unwrap().contains(&failed_id));
    }

    #[tokio::test]
    async fn create_tasks_continues_after_one_admission_failure() {
        let (directory, store, queue, mut events) = test_queue(true);
        rusqlite::Connection::open(directory.path().join("commands.db"))
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_one_task BEFORE INSERT ON tasks
                 WHEN NEW.input_path = 'bad.png'
                 BEGIN SELECT RAISE(ABORT, 'forced one-task failure'); END;",
            )
            .unwrap();

        let batch = create_tasks_with_queue(
            vec!["bad.png".into(), "good.png".into()],
            ServiceId::Vl16,
            ParseOptions::default(),
            &queue,
        )
        .unwrap();

        assert_eq!(batch.task_ids.len(), 2);
        let mut failed = false;
        let mut done = false;
        while !failed || !done {
            match timeout(Duration::from_secs(1), events.recv())
                .await
                .unwrap()
            {
                Some(QueueEvent::Failed { id, .. }) if id == batch.task_ids[0] => failed = true,
                Some(QueueEvent::Done { id, .. }) if id == batch.task_ids[1] => done = true,
                Some(QueueEvent::Submitted { .. }) | Some(QueueEvent::Progress { .. }) => {}
                event => panic!("unexpected event: {event:?}"),
            }
        }
        let tasks = store.lock().unwrap().list_tasks(None).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].input_path, "good.png");
    }
}
