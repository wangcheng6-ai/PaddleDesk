use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::Duration,
};

use image::{ImageBuffer, Rgba};
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

pub(crate) mod desktop;
#[cfg(windows)]
mod windows;

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const POLL_ATTEMPTS: usize = 120;

struct ClipboardImage {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

impl ClipboardImage {
    fn fingerprint(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.width.hash(&mut hasher);
        self.height.hash(&mut hasher);
        self.rgba.hash(&mut hasher);
        hasher.finish()
    }
}

pub(crate) async fn read_image(app: &AppHandle) -> Result<PathBuf, String> {
    save_image(app, clipboard_image(app)?).await
}

pub(crate) async fn select_region(app: &AppHandle) -> Result<PathBuf, String> {
    let before = clipboard_image(app).ok().map(|image| image.fingerprint());
    launch_region_selector()?;
    for _ in 0..POLL_ATTEMPTS {
        tokio::time::sleep(POLL_INTERVAL).await;
        if let Ok(image) = clipboard_image(app) {
            if Some(image.fingerprint()) != before {
                return save_image(app, image).await;
            }
        }
    }
    Err("screen capture canceled or timed out".into())
}

fn clipboard_image(app: &AppHandle) -> Result<ClipboardImage, String> {
    let image = app
        .clipboard()
        .read_image()
        .map_err(|error| error.to_string())?;
    Ok(ClipboardImage {
        rgba: image.rgba().to_vec(),
        width: image.width(),
        height: image.height(),
    })
}

async fn save_image(app: &AppHandle, image: ClipboardImage) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("captures");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join(format!("{}.png", uuid::Uuid::new_v4()));
    write_png(&image.rgba, image.width, image.height, &path)?;
    Ok(path)
}

fn write_png(rgba: &[u8], width: u32, height: u32, path: &Path) -> Result<(), String> {
    let image = ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "clipboard image buffer has invalid dimensions".to_string())?;
    image.save(path).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn launch_region_selector() -> Result<(), String> {
    windows::launch_region_selector()
}

#[cfg(not(windows))]
fn launch_region_selector() -> Result<(), String> {
    Err("screen capture is not supported on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::write_png;

    #[test]
    fn writes_clipboard_rgba_as_png() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("clipboard.png");

        write_png(&[0x2b, 0x36, 0xe8, 0xff], 1, 1, &path).unwrap();

        assert_eq!(&std::fs::read(path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    }
}
