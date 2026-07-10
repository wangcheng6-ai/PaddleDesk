use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager,
};
use tauri_plugin_autostart::ManagerExt;

use crate::{commands, native, AppState};

pub(crate) const SCREENSHOT_SHORTCUT: &str = "Ctrl+Alt+S";
const TRAY_ID: &str = "main-tray";

pub(crate) fn setup_tray(app: &mut App, language: &str) -> tauri::Result<()> {
    let menu = tray_menu(app.handle(), language)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".to_string()))?,
        )
        .tooltip("PaddleDesk")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "capture" => trigger_capture(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub(crate) fn refresh_tray(app: &AppHandle, language: &str) -> Result<(), String> {
    let menu = tray_menu(app, language).map_err(|error| error.to_string())?;
    app.tray_by_id(TRAY_ID)
        .ok_or_else(|| "tray icon is unavailable".to_string())?
        .set_menu(Some(menu))
        .map_err(|error| error.to_string())
}

pub(crate) fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub(crate) fn trigger_capture(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(error) = commands::start_capture_inner(&app, &state).await {
            eprintln!("PaddleDesk screen capture: {error}");
        }
    });
}

pub(crate) fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let current = manager.is_enabled().map_err(|error| error.to_string())?;
    match autostart_change(current, enabled) {
        Some(true) => manager.enable(),
        Some(false) => manager.disable(),
        None => return Ok(()),
    }
    .map_err(|error| error.to_string())
}

pub(crate) fn autostart_enabled(app: &AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

fn autostart_change(current: bool, desired: bool) -> Option<bool> {
    (current != desired).then_some(desired)
}

fn tray_menu(app: &AppHandle, language: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let copy = native::native_copy(native::native_locale(language));
    let show = MenuItem::with_id(app, "show", copy.show, true, None::<&str>)?;
    let capture = MenuItem::with_id(app, "capture", copy.capture, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", copy.quit, true, None::<&str>)?;
    Menu::with_items(app, &[&show, &capture, &quit])
}

#[cfg(test)]
mod tests {
    use super::autostart_change;

    #[test]
    fn disabled_autostart_is_idempotent() {
        assert_eq!(autostart_change(false, false), None);
    }
}
