mod common;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use std::fs;
use std::path::Path;

use dbx_plugin_sdk::PluginError;

use common::{cache_path_for_key, icon_cache_dir, read_png_data_url};

/// Resolve a stable icon key for a process executable.
pub fn icon_key_for_exe(exe: Option<&Path>) -> Option<String> {
    let exe = exe?;
    if !exe.exists() {
        return None;
    }

    #[cfg(target_os = "macos")]
    {
        return macos::icon_key(exe);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::icon_key(exe);
    }
    #[cfg(target_os = "linux")]
    {
        return linux::icon_key(exe);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = exe;
        None
    }
}

pub fn icon_data_url(key: &str) -> Result<String, PluginError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(PluginError::new(-32602, "Missing icon key"));
    }

    let cache = cache_path_for_key(key);
    if cache.is_file() {
        return read_png_data_url(&cache);
    }

    fs::create_dir_all(icon_cache_dir())
        .map_err(|err| PluginError::new(-32000, format!("Failed to create icon cache: {err}")))?;

    let extracted = {
        #[cfg(target_os = "macos")]
        {
            macos::extract_icon(key, &cache)
        }
        #[cfg(target_os = "windows")]
        {
            windows::extract_icon(key, &cache)
        }
        #[cfg(target_os = "linux")]
        {
            linux::extract_icon(key, &cache)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let _ = (key, &cache);
            false
        }
    };

    if extracted && cache.is_file() {
        read_png_data_url(&cache)
    } else {
        Err(PluginError::new(-32000, "No application icon available"))
    }
}
