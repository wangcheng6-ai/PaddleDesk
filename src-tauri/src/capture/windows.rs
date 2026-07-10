use std::process::Command;

pub(super) fn launch_region_selector() -> Result<(), String> {
    Command::new("explorer.exe")
        .arg("ms-screenclip:")
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}
