use std::path::PathBuf;
use std::process::Command;

#[cfg(all(unix, not(target_os = "macos")))]
use std::path::Path;

use dbx_plugin_sdk::PluginError;

/// Reveal a local file/folder in the system file manager.
pub fn reveal_path(raw: &str) -> Result<(), PluginError> {
    let path = PathBuf::from(raw);
    if raw.trim().is_empty() {
        return Err(PluginError::new(-32602, "Path is empty"));
    }
    if !path.is_absolute() {
        return Err(PluginError::new(-32602, "Path must be absolute"));
    }
    if !path.exists() {
        return Err(PluginError::new(
            -32000,
            format!("Path does not exist: {}", path.display()),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-R", raw])
            .spawn()
            .map_err(|err| PluginError::new(-32000, format!("Failed to reveal path: {err}")))?;
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(format!("/select,{raw}"))
            .spawn()
            .map_err(|err| PluginError::new(-32000, format!("Failed to reveal path: {err}")))?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let folder = if path.is_dir() {
            path.clone()
        } else {
            path.parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| PluginError::new(-32000, "Path has no parent directory"))?
        };
        Command::new("xdg-open")
            .arg(&folder)
            .spawn()
            .map_err(|err| PluginError::new(-32000, format!("Failed to open folder: {err}")))?;
    }

    Ok(())
}
