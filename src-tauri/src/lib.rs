use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use tauri::{Emitter, Manager};
use tokio::sync::mpsc;

pub mod api;
pub mod commands;
pub mod export;
pub mod model;
pub mod queue;
pub mod storage;

pub struct AppState {
    pub(crate) store: Arc<Mutex<storage::Store>>,
    pub(crate) queue: Arc<queue::Queue>,
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
        queue::QueueEvent::Done { id } => ("task:done", serde_json::json!({"id": id})),
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

fn mock_services() -> HashMap<model::ServiceId, Arc<dyn api::OcrService>> {
    [
        model::ServiceId::Vl16,
        model::ServiceId::PpOcrV6,
        model::ServiceId::StructureV3,
    ]
    .into_iter()
    .map(|id| {
        (
            id,
            Arc::new(api::mock::MockOcr::new()) as Arc<dyn api::OcrService>,
        )
    })
    .collect()
}

fn spawn_event_bridge(
    app_handle: tauri::AppHandle,
    store: Arc<Mutex<storage::Store>>,
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
        }
    });
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let store = Arc::new(Mutex::new(storage::Store::open(
        &data_dir.join("paddledesk.db"),
    )?));
    let persist_results = store
        .lock()
        .map_err(|_| std::io::Error::other("store lock poisoned"))?
        .get_setting("privacy_mode")?
        .as_deref()
        != Some("1");
    let (event_sender, event_receiver) = mpsc::unbounded_channel();
    let queue = queue::Queue::new(
        store.clone(),
        mock_services(),
        2,
        event_sender,
        Duration::from_secs(1),
        persist_results,
    );
    app.manage(AppState {
        store: store.clone(),
        queue: queue.clone(),
    });
    spawn_event_bridge(app.handle().clone(), store, event_receiver);
    queue.resume();
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            commands::create_tasks,
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
    use super::*;
    use crate::{model::OcrError, queue::QueueEvent};

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
                QueueEvent::Done { id: "1".into() },
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
            &QueueEvent::Done { id: "task".into() },
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
                &QueueEvent::Done { id: "task".into() },
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
}
