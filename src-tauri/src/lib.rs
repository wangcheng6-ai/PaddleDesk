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

fn task_event_name(event: &queue::QueueEvent) -> &'static str {
    match event {
        queue::QueueEvent::Progress { .. } => "task:progress",
        queue::QueueEvent::Done { .. } => "task:done",
        queue::QueueEvent::Failed { .. } => "task:failed",
        queue::QueueEvent::Canceled { .. } => "task:canceled",
    }
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
            let completed = matches!(event, queue::QueueEvent::Done { .. });
            let _ = app_handle.emit(task_event_name(&event), event);
            if completed {
                emit_usage(&app_handle, &store);
            }
        }
    });
}

fn emit_usage(app_handle: &tauri::AppHandle, store: &Arc<Mutex<storage::Store>>) {
    let today_pages = store.lock().ok().and_then(|store| {
        store.usage_since(1).ok().map(|rows| {
            rows.into_iter()
                .fold(0_u32, |sum, row| sum.saturating_add(row.pages))
        })
    });
    if let Some(today_pages) = today_pages {
        let _ = app_handle.emit("usage:updated", UsageUpdated { today_pages });
    }
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
    fn event_names_match_ipc_contract() {
        let events = [
            (
                QueueEvent::Progress {
                    id: "1".into(),
                    stage: "uploading".into(),
                    page: 0,
                    total: 0,
                },
                "task:progress",
            ),
            (QueueEvent::Done { id: "1".into() }, "task:done"),
            (
                QueueEvent::Failed {
                    id: "1".into(),
                    error: OcrError::Auth,
                },
                "task:failed",
            ),
            (QueueEvent::Canceled { id: "1".into() }, "task:canceled"),
        ];

        for (event, expected) in events {
            assert_eq!(task_event_name(&event), expected);
        }
    }
}
