use std::path::{Path, PathBuf};

use super::common::{resize_image_file_to_png, write_rgba_png};
use crate::util::log_stderr;

pub fn icon_key(exe: &Path) -> Option<String> {
    let ext = exe.extension()?.to_str()?.to_ascii_lowercase();
    if matches!(ext.as_str(), "exe" | "dll" | "ico" | "lnk") {
        Some(exe.to_string_lossy().into_owned())
    } else {
        None
    }
}

pub fn extract_icon(key: &str, out_png: &Path) -> bool {
    let source = PathBuf::from(key);
    if !source.exists() {
        return false;
    }

    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if matches!(ext.as_str(), "ico" | "png" | "jpg" | "jpeg" | "bmp" | "gif") {
        return resize_image_file_to_png(&source, out_png);
    }

    extract_from_pe(&source, out_png)
}

fn extract_from_pe(path: &Path, out_png: &Path) -> bool {
    let path_str = path.to_string_lossy();
    match win_icon_extractor::extract_icon(path_str.as_ref()) {
        Ok(icon) => write_rgba_png(out_png, icon.width, icon.height, &icon.rgba),
        Err(err) => match win_icon_extractor::extract_icon_at(path_str.as_ref(), 0) {
            Ok(icon) => write_rgba_png(out_png, icon.width, icon.height, &icon.rgba),
            Err(err2) => {
                log_stderr(format!(
                    "windows icon extract failed for {}: {err}; fallback: {err2}",
                    path.display()
                ));
                false
            }
        },
    }
}
