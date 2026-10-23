use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::common::{resize_image_file_to_png, ICON_EDGE};
use crate::util::log_stderr;

pub fn icon_key(exe: &Path) -> Option<String> {
    // Prefer absolute path so desktop/theme lookup stays stable across refreshes.
    let abs = if exe.is_absolute() {
        exe.to_path_buf()
    } else {
        env::current_dir().ok()?.join(exe)
    };
    Some(abs.to_string_lossy().into_owned())
}

pub fn extract_icon(key: &str, out_png: &Path) -> bool {
    let exe = PathBuf::from(key);
    if let Some(icon_file) = resolve_icon_file(&exe) {
        if icon_file
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        {
            if convert_svg(&icon_file, out_png) {
                return true;
            }
        } else if resize_image_file_to_png(&icon_file, out_png) {
            return true;
        }
    }
    false
}

fn resolve_icon_file(exe: &Path) -> Option<PathBuf> {
    let icon_name = desktop_icon_name(exe).or_else(|| {
        exe.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
    })?;

    if Path::new(&icon_name).is_absolute() {
        let path = PathBuf::from(&icon_name);
        if path.is_file() {
            return Some(path);
        }
    }

    // Sidecar icons next to the binary.
    if let Some(parent) = exe.parent() {
        for ext in ["png", "svg", "xpm", "jpg", "jpeg"] {
            let candidate = parent.join(format!("{icon_name}.{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    find_theme_icon(&icon_name)
}

fn desktop_icon_name(exe: &Path) -> Option<String> {
    let basename = exe.file_name()?.to_string_lossy();
    let stem = exe.file_stem()?.to_string_lossy();
    let abs = exe.to_string_lossy();

    for dir in desktop_dirs() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            if let Some(icon) = parse_desktop_icon(&path, &basename, &stem, &abs) {
                return Some(icon);
            }
        }
    }
    None
}

fn desktop_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/share/applications"));
    }
    if let Ok(xdg) = env::var("XDG_DATA_DIRS") {
        for part in xdg.split(':').filter(|s| !s.is_empty()) {
            dirs.push(PathBuf::from(part).join("applications"));
        }
    } else {
        dirs.push(PathBuf::from("/usr/local/share/applications"));
        dirs.push(PathBuf::from("/usr/share/applications"));
    }
    dirs
}

fn parse_desktop_icon(path: &Path, basename: &str, stem: &str, abs: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let mut exec = None::<String>;
    let mut icon = None::<String>;
    let mut hidden = false;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            if in_entry {
                break;
            }
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "Exec" => exec = Some(value.trim().to_string()),
                "Icon" => icon = Some(value.trim().to_string()),
                "NoDisplay" | "Hidden" => {
                    if value.trim().eq_ignore_ascii_case("true") {
                        hidden = true;
                    }
                }
                _ => {}
            }
        }
    }

    if hidden {
        return None;
    }
    let exec = exec?;
    let exec_bin = exec
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches('"');
    let exec_name = Path::new(exec_bin)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(exec_bin);

    let matched = exec_bin == abs
        || exec_name == basename
        || exec_name == stem
        || exec.contains(basename)
        || exec.contains(stem);
    if matched {
        icon.filter(|value| !value.is_empty())
    } else {
        None
    }
}

fn find_theme_icon(name: &str) -> Option<PathBuf> {
    let name = name.trim_end_matches(".png").trim_end_matches(".svg");
    let sizes = [
        format!("{ICON_EDGE}x{ICON_EDGE}"),
        "128x128".into(),
        "96x96".into(),
        "48x48".into(),
        "32x32".into(),
        "scalable".into(),
    ];
    let categories = ["apps", "applications", "legacy", "status", "devices", "mimetypes"];

    for root in icon_roots() {
        // /usr/share/pixmaps/name.png
        if root.ends_with("pixmaps") {
            for ext in ["png", "svg", "xpm"] {
                let candidate = root.join(format!("{name}.{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
            continue;
        }

        // Prefer hicolor, then whatever themes exist.
        let mut themes = vec!["hicolor".into(), "Adwaita".into(), "breeze".into()];
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let theme = entry.file_name().to_string_lossy().into_owned();
                if !themes.iter().any(|t| t == &theme) {
                    themes.push(theme);
                }
            }
        }

        for theme in themes {
            for size in &sizes {
                for category in categories {
                    for ext in ["png", "svg"] {
                        let candidate = root
                            .join(&theme)
                            .join(size)
                            .join(category)
                            .join(format!("{name}.{ext}"));
                        if candidate.is_file() {
                            return Some(candidate);
                        }
                    }
                }
            }
        }
    }
    None
}

fn icon_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".local/share/icons"));
        roots.push(home.join(".icons"));
    }
    if let Ok(xdg) = env::var("XDG_DATA_DIRS") {
        for part in xdg.split(':').filter(|s| !s.is_empty()) {
            roots.push(PathBuf::from(part).join("icons"));
        }
    } else {
        roots.push(PathBuf::from("/usr/local/share/icons"));
        roots.push(PathBuf::from("/usr/share/icons"));
    }
    roots.push(PathBuf::from("/usr/share/pixmaps"));
    roots
}

fn convert_svg(input: &Path, out_png: &Path) -> bool {
    // Prefer rsvg-convert when present; otherwise try ImageMagick convert.
    let edge = ICON_EDGE.to_string();
    let rsvg = Command::new("rsvg-convert")
        .args(["-w", &edge, "-h", &edge, "-o"])
        .arg(out_png)
        .arg(input)
        .status();
    if matches!(rsvg, Ok(status) if status.success() && out_png.is_file()) {
        return true;
    }

    let convert = Command::new("convert")
        .arg(input)
        .arg("-resize")
        .arg(format!("{ICON_EDGE}x{ICON_EDGE}"))
        .arg(out_png)
        .status();
    match convert {
        Ok(status) if status.success() && out_png.is_file() => true,
        Ok(_) | Err(_) => {
            log_stderr(format!(
                "linux svg icon convert failed for {}",
                input.display()
            ));
            false
        }
    }
}
