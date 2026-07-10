use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use tauri::{Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::mpsc;

pub mod api;
pub mod capture;
pub mod commands;
pub mod export;
pub mod model;
pub mod native;
pub mod queue;
pub mod storage;

pub struct AppState {
    pub(crate) store: Arc<Mutex<storage::Store>>,
    pub(crate) queue: Arc<queue::Queue>,
    pub(crate) proxy: api::paddle::ProxyProvider,
    pub(crate) capture_tasks: Arc<Mutex<HashSet<String>>>,
}

#[derive(Clone, Serialize)]
struct UsageUpdated {
    today_pages: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgeDiagnostic {
    TaskEmit,
    UsageLock,
    UsageQuery,
    UsageEmit,
}

impl BridgeDiagnostic {
    fn message(self) -> &'static str {
        match self {
            Self::TaskEmit => "task event emit failed",
            Self::UsageLock => "usage store lock failed",
            Self::UsageQuery => "usage query failed",
            Self::UsageEmit => "usage event emit failed",
        }
    }
}

fn task_ipc_event(event: &queue::QueueEvent) -> (&'static str, serde_json::Value) {
    match event {
        queue::QueueEvent::Progress {
            id,
            stage,
            page,
            total,
        } => (
            "task:progress",
            serde_json::json!({"id": id, "stage": stage, "page": page, "total": total}),
        ),
        queue::QueueEvent::Done { id, .. } => ("task:done", serde_json::json!({"id": id})),
        queue::QueueEvent::Failed { id, error } => (
            "task:failed",
            serde_json::json!({
                "id": id,
                "kind": ipc_error_kind(error),
                "message": ipc_error_message(error),
            }),
        ),
        queue::QueueEvent::Canceled { id } => ("task:canceled", serde_json::json!({"id": id})),
    }
}

fn ipc_error_kind(error: &model::OcrError) -> &'static str {
    match error {
        model::OcrError::Auth => "Auth",
        model::OcrError::Quota => "Quota",
        model::OcrError::RateLimited(_) => "RateLimited",
        model::OcrError::InvalidInput(_) => "InvalidInput",
        model::OcrError::Network(_) => "Network",
        model::OcrError::Server(_) => "Server",
        model::OcrError::Parse(_) => "Parse",
    }
}

fn ipc_error_message(error: &model::OcrError) -> &str {
    match error {
        model::OcrError::Auth | model::OcrError::Quota => "",
        model::OcrError::RateLimited(message)
        | model::OcrError::InvalidInput(message)
        | model::OcrError::Network(message)
        | model::OcrError::Server(message)
        | model::OcrError::Parse(message) => message,
    }
}

fn forward_event(
    event: &queue::QueueEvent,
    load_usage: impl FnOnce() -> Result<u32, BridgeDiagnostic>,
    mut emit: impl FnMut(&str, serde_json::Value) -> Result<(), ()>,
    mut diagnose: impl FnMut(BridgeDiagnostic),
) {
    let (name, payload) = task_ipc_event(event);
    if emit(name, payload).is_err() {
        diagnose(BridgeDiagnostic::TaskEmit);
    }
    if matches!(event, queue::QueueEvent::Done { .. }) {
        match load_usage() {
            Ok(today_pages) => {
                let payload = serde_json::json!(UsageUpdated { today_pages });
                if emit("usage:updated", payload).is_err() {
                    diagnose(BridgeDiagnostic::UsageEmit);
                }
            }
            Err(error) => diagnose(error),
        }
    }
}

fn load_today_pages(store: &Arc<Mutex<storage::Store>>) -> Result<u32, BridgeDiagnostic> {
    let store = store.lock().map_err(|_| BridgeDiagnostic::UsageLock)?;
    let rows = store
        .usage_since(1)
        .map_err(|_| BridgeDiagnostic::UsageQuery)?;
    Ok(rows
        .into_iter()
        .fold(0_u32, |sum, row| sum.saturating_add(row.pages)))
}

fn report_bridge_diagnostic(diagnostic: BridgeDiagnostic) {
    eprintln!("PaddleDesk event bridge: {}", diagnostic.message());
}

fn forward_capture_event(
    app: &tauri::AppHandle,
    store: &Arc<Mutex<storage::Store>>,
    capture_tasks: &Arc<Mutex<HashSet<String>>>,
    event: &queue::QueueEvent,
) {
    let id = match event {
        queue::QueueEvent::Done { id, .. }
        | queue::QueueEvent::Failed { id, .. }
        | queue::QueueEvent::Canceled { id } => id,
        queue::QueueEvent::Progress { .. } => return,
    };
    let captured = capture_tasks
        .lock()
        .map(|mut tasks| tasks.remove(id))
        .unwrap_or(false);
    if !captured {
        return;
    }
    let language = store
        .lock()
        .ok()
        .and_then(|store| store.get_setting("language").ok().flatten())
        .unwrap_or_else(|| "system".into());
    let copy = native::native_copy(native::native_locale(&language));
    let copied = match event {
        queue::QueueEvent::Done { result, .. } => app
            .clipboard()
            .write_text(result.markdown.clone())
            .map(|_| true)
            .unwrap_or_else(|error| {
                eprintln!("PaddleDesk capture clipboard: {error}");
                false
            }),
        _ => false,
    };
    let body = if copied {
        copy.capture_done
    } else {
        copy.capture_failed
    };
    if let Err(error) = app
        .notification()
        .builder()
        .title(copy.notification_title)
        .body(body)
        .show()
    {
        eprintln!("PaddleDesk capture notification: {error}");
    }
    if copied {
        let _ = app.emit("capture:done", serde_json::json!({"task_id": id}));
    }
}

fn real_services(
    token: api::paddle::TokenProvider,
    proxy: api::paddle::ProxyProvider,
) -> HashMap<model::ServiceId, Arc<dyn api::OcrService>> {
    [
        model::ServiceId::Vl16,
        model::ServiceId::PpOcrV6,
        model::ServiceId::StructureV3,
    ]
    .into_iter()
    .map(|id| {
        (
            id,
            Arc::new(api::paddle::PaddleOcr::new(
                id,
                token.clone(),
                proxy.clone(),
            )) as Arc<dyn api::OcrService>,
        )
    })
    .collect()
}

fn proxy_provider(store: Arc<Mutex<storage::Store>>) -> api::paddle::ProxyProvider {
    Arc::new(move || {
        let store = store
            .lock()
            .map_err(|_| model::OcrError::Parse("store lock poisoned".into()))?;
        let mode = store
            .get_setting("proxy_mode")
            .map_err(|error| model::OcrError::Parse(error.to_string()))?
            .unwrap_or_else(|| "system".into());
        match mode.as_str() {
            "system" => Ok(api::paddle::ProxyConfig::System),
            "direct" => Ok(api::paddle::ProxyConfig::Direct),
            "custom" => store
                .get_setting("proxy_address")
                .map_err(|error| model::OcrError::Parse(error.to_string()))?
                .filter(|address| !address.trim().is_empty())
                .map(api::paddle::ProxyConfig::Custom)
                .ok_or_else(|| {
                    model::OcrError::InvalidInput("custom proxy address is empty".into())
                }),
            _ => Err(model::OcrError::InvalidInput(
                "invalid stored proxy mode".into(),
            )),
        }
    })
}

fn startup_concurrency(store: &storage::Store) -> anyhow::Result<usize> {
    Ok(store
        .get_setting("concurrency")?
        .and_then(|value| value.parse().ok())
        .unwrap_or(2)
        .clamp(1, 4))
}

fn spawn_event_bridge(
    app_handle: tauri::AppHandle,
    store: Arc<Mutex<storage::Store>>,
    capture_tasks: Arc<Mutex<HashSet<String>>>,
    mut events: mpsc::UnboundedReceiver<queue::QueueEvent>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            forward_event(
                &event,
                || load_today_pages(&store),
                |name, payload| app_handle.emit(name, payload).map_err(|_| ()),
                report_bridge_diagnostic,
            );
            forward_capture_event(&app_handle, &store, &capture_tasks, &event);
        }
    });
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let store = Arc::new(Mutex::new(storage::Store::open(
        &data_dir.join("paddledesk.db"),
    )?));
    let (persist_results, concurrency, language, autostart) = {
        let store = store
            .lock()
            .map_err(|_| std::io::Error::other("store lock poisoned"))?;
        (
            store.get_setting("privacy_mode")?.as_deref() != Some("1"),
            startup_concurrency(&store)?,
            store
                .get_setting("language")?
                .unwrap_or_else(|| "system".into()),
            store.get_setting("autostart")?.as_deref() == Some("1"),
        )
    };
    let token: api::paddle::TokenProvider = Arc::new(api::credentials::load_token);
    let proxy = proxy_provider(store.clone());
    let (event_sender, event_receiver) = mpsc::unbounded_channel();
    let queue = queue::Queue::new(
        store.clone(),
        real_services(token, proxy.clone()),
        concurrency,
        event_sender,
        Duration::from_secs(1),
        persist_results,
    );
    let capture_tasks = Arc::new(Mutex::new(HashSet::new()));
    app.manage(AppState {
        store: store.clone(),
        queue: queue.clone(),
        proxy,
        capture_tasks: capture_tasks.clone(),
    });
    capture::desktop::setup_tray(app, &language)?;
    capture::desktop::set_autostart(app.handle(), autostart)?;
    app.global_shortcut()
        .register(capture::desktop::SCREENSHOT_SHORTCUT)?;
    spawn_event_bridge(app.handle().clone(), store, capture_tasks, event_receiver);
    queue.resume();
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            capture::desktop::show_main(app);
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state == ShortcutState::Pressed {
                        capture::desktop::trigger_capture(app.clone());
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(setup)
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_tasks,
            commands::create_task_from_clipboard,
            commands::start_capture,
            commands::list_tasks,
            commands::cancel_task,
            commands::retry_task,
            commands::get_result,
            commands::export_result,
            commands::search_history,
            commands::get_usage,
            commands::get_settings,
            commands::set_settings,
            commands::validate_token,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::{
        model::{OcrError, RecognitionResult},
        queue::QueueEvent,
    };

    fn empty_result() -> RecognitionResult {
        RecognitionResult {
            markdown: String::new(),
            page_count: 0,
            pages: Vec::new(),
        }
    }

    #[test]
    fn task_payloads_match_ipc_contract_exactly() {
        let events = [
            (
                QueueEvent::Progress {
                    id: "1".into(),
                    stage: "uploading".into(),
                    page: 0,
                    total: 0,
                },
                "task:progress",
                serde_json::json!({"id": "1", "stage": "uploading", "page": 0, "total": 0}),
            ),
            (
                QueueEvent::Done {
                    id: "1".into(),
                    result: empty_result(),
                },
                "task:done",
                serde_json::json!({"id": "1"}),
            ),
            (
                QueueEvent::Failed {
                    id: "1".into(),
                    error: OcrError::Auth,
                },
                "task:failed",
                serde_json::json!({
                    "id": "1",
                    "kind": "Auth",
                    "message": "",
                }),
            ),
            (
                QueueEvent::Canceled { id: "1".into() },
                "task:canceled",
                serde_json::json!({"id": "1"}),
            ),
        ];

        for (event, expected_name, expected_payload) in events {
            let (name, payload) = task_ipc_event(&event);
            assert_eq!(name, expected_name);
            assert_eq!(payload, expected_payload);
        }
    }

    #[test]
    fn failed_payload_maps_every_error_kind_exactly() {
        let errors = [
            (OcrError::Auth, "Auth", ""),
            (OcrError::Quota, "Quota", ""),
            (
                OcrError::RateLimited("slow down".into()),
                "RateLimited",
                "slow down",
            ),
            (
                OcrError::InvalidInput("bad file".into()),
                "InvalidInput",
                "bad file",
            ),
            (OcrError::Network("offline".into()), "Network", "offline"),
            (OcrError::Server("503".into()), "Server", "503"),
            (OcrError::Parse("bad json".into()), "Parse", "bad json"),
        ];

        for (error, kind, message) in errors {
            let (_, payload) = task_ipc_event(&QueueEvent::Failed {
                id: "task".into(),
                error,
            });
            assert_eq!(
                payload,
                serde_json::json!({"id": "task", "kind": kind, "message": message})
            );
        }
    }

    #[test]
    fn usage_payload_matches_ipc_contract_exactly() {
        assert_eq!(
            serde_json::to_value(UsageUpdated { today_pages: 7 }).unwrap(),
            serde_json::json!({"today_pages": 7})
        );
    }

    #[test]
    fn bridge_reports_task_and_usage_emit_failures() {
        let mut task_diagnostics = Vec::new();
        forward_event(
            &QueueEvent::Progress {
                id: "task".into(),
                stage: "uploading".into(),
                page: 0,
                total: 0,
            },
            || Ok(0),
            |_, _| Err(()),
            |diagnostic| task_diagnostics.push(diagnostic),
        );
        assert_eq!(task_diagnostics, [BridgeDiagnostic::TaskEmit]);

        let mut usage_diagnostics = Vec::new();
        forward_event(
            &QueueEvent::Done {
                id: "task".into(),
                result: empty_result(),
            },
            || Ok(3),
            |name, _| (name != "usage:updated").then_some(()).ok_or(()),
            |diagnostic| usage_diagnostics.push(diagnostic),
        );
        assert_eq!(usage_diagnostics, [BridgeDiagnostic::UsageEmit]);
    }

    #[test]
    fn bridge_reports_usage_loader_failures() {
        for failure in [BridgeDiagnostic::UsageLock, BridgeDiagnostic::UsageQuery] {
            let mut diagnostics = Vec::new();
            forward_event(
                &QueueEvent::Done {
                    id: "task".into(),
                    result: empty_result(),
                },
                || Err(failure),
                |_, _| Ok(()),
                |diagnostic| diagnostics.push(diagnostic),
            );
            assert_eq!(diagnostics, [failure]);
        }
    }

    #[test]
    fn usage_loader_distinguishes_lock_and_query_failures() {
        let directory = tempfile::tempdir().unwrap();
        let lock_store = Arc::new(Mutex::new(
            storage::Store::open(&directory.path().join("lock.db")).unwrap(),
        ));
        let poisoned = lock_store.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoned.lock().unwrap();
            panic!("poison usage lock");
        })
        .join();
        assert_eq!(
            load_today_pages(&lock_store),
            Err(BridgeDiagnostic::UsageLock)
        );

        let query_path = directory.path().join("query.db");
        let query_store = Arc::new(Mutex::new(storage::Store::open(&query_path).unwrap()));
        rusqlite::Connection::open(&query_path)
            .unwrap()
            .execute("DROP TABLE usage", [])
            .unwrap();
        assert_eq!(
            load_today_pages(&query_store),
            Err(BridgeDiagnostic::UsageQuery)
        );
    }

    #[test]
    fn real_services_cover_all_models() {
        let token: api::paddle::TokenProvider = Arc::new(|| Ok("test-token".into()));
        let proxy: api::paddle::ProxyProvider = Arc::new(|| Ok(api::paddle::ProxyConfig::Direct));
        let services = real_services(token, proxy);

        assert_eq!(services.len(), 3);
        for id in [
            model::ServiceId::Vl16,
            model::ServiceId::PpOcrV6,
            model::ServiceId::StructureV3,
        ] {
            assert_eq!(services.get(&id).unwrap().id(), id);
        }
    }

    #[test]
    fn startup_concurrency_defaults_and_clamps() {
        let directory = tempfile::tempdir().unwrap();
        let store = storage::Store::open(&directory.path().join("settings.db")).unwrap();
        assert_eq!(startup_concurrency(&store).unwrap(), 2);

        store
            .set_settings(&HashMap::from([("concurrency".into(), "9".into())]))
            .unwrap();
        assert_eq!(startup_concurrency(&store).unwrap(), 4);
        store
            .set_settings(&HashMap::from([("concurrency".into(), "0".into())]))
            .unwrap();
        assert_eq!(startup_concurrency(&store).unwrap(), 1);
    }

    #[test]
    fn proxy_provider_reads_current_settings() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(
            storage::Store::open(&directory.path().join("proxy.db")).unwrap(),
        ));
        let provider = proxy_provider(store.clone());
        assert_eq!(provider().unwrap(), api::paddle::ProxyConfig::System);

        store
            .lock()
            .unwrap()
            .set_settings(&HashMap::from([
                ("proxy_mode".into(), "custom".into()),
                ("proxy_address".into(), "http://127.0.0.1:7890".into()),
            ]))
            .unwrap();
        assert_eq!(
            provider().unwrap(),
            api::paddle::ProxyConfig::Custom("http://127.0.0.1:7890".into())
        );
    }
}
