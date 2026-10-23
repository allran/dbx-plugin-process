use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::common::{icon_cache_dir, ICON_EDGE};
use crate::util::log_stderr;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

pub fn icon_key(exe: &Path) -> Option<String> {
    find_app_bundle(exe).map(|app| app.to_string_lossy().into_owned())
}

pub fn extract_icon(key: &str, out_png: &Path) -> bool {
    let source = PathBuf::from(key);
    if !source.exists() {
        return false;
    }
    extract_macos_icon(&source, out_png)
}

fn is_app_bundle(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
        && path.is_dir()
}

fn is_helper_app(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains("Helper"))
}

/// Prefer the main `.app` over nested `*Helper*.app` so Chrome/Cursor helpers
/// share one cached icon and skip slow qlmanage on helper bundles.
fn find_app_bundle(exe: &Path) -> Option<PathBuf> {
    let mut apps = Vec::new();
    for ancestor in exe.ancestors() {
        if is_app_bundle(ancestor) {
            apps.push(ancestor.to_path_buf());
        }
    }
    apps.iter()
        .find(|path| !is_helper_app(path))
        .or_else(|| apps.first())
        .cloned()
}

fn extract_macos_icon(source: &Path, out_png: &Path) -> bool {
    if is_app_bundle(source) {
        if let Some(icns) = resolve_bundle_icns(source) {
            if run_sips(&icns, out_png) {
                return true;
            }
        }
        // Nested helpers rarely have usable previews; skip the slow qlmanage path.
        if is_helper_app(source) {
            return false;
        }
        return run_qlmanage(source, out_png);
    }

    if source
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "icns" | "png" | "jpg" | "jpeg"))
    {
        return run_sips(source, out_png);
    }

    false
}

fn resolve_bundle_icns(app: &Path) -> Option<PathBuf> {
    let resources = app.join("Contents/Resources");
    let plist = app.join("Contents/Info.plist");
    let icon_name = plutil_string(&plist, "CFBundleIconFile")
        .or_else(|| plutil_string(&plist, "CFBundleIconName"))?;

    let stem = icon_name.trim_end_matches(".icns").to_string();
    let candidates = [
        resources.join(format!("{stem}.icns")),
        resources.join(&icon_name),
        resources.join(format!("{stem}.png")),
        resources.join("AppIcon.icns"),
        resources.join("application.icns"),
    ];
    candidates.into_iter().find(|path| path.is_file())
}

fn plutil_string(plist: &Path, key: &str) -> Option<String> {
    let output = run_command_timeout(
        Command::new("plutil").args([
            "-extract",
            key,
            "raw",
            "-o",
            "-",
            &plist.to_string_lossy(),
        ]),
        Duration::from_millis(800),
    )?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn run_sips(input: &Path, out_png: &Path) -> bool {
    let edge = ICON_EDGE.to_string();
    let mut cmd = Command::new("sips");
    cmd.args(["-z", &edge, &edge, "-s", "format", "png"])
        .arg(input)
        .arg("--out")
        .arg(out_png);
    match run_command_timeout(&mut cmd, COMMAND_TIMEOUT) {
        Some(output) if output.status.success() && out_png.is_file() => true,
        Some(output) => {
            log_stderr(format!(
                "sips failed ({}) for {}",
                output.status,
                input.display()
            ));
            false
        }
        None => {
            log_stderr(format!("sips timed out for {}", input.display()));
            false
        }
    }
}

fn run_qlmanage(app: &Path, out_png: &Path) -> bool {
    let staging = match tempfile_dir() {
        Some(dir) => dir,
        None => return false,
    };
    let edge = ICON_EDGE.to_string();
    let mut cmd = Command::new("qlmanage");
    cmd.args(["-t", "-s", &edge, "-o"]).arg(&staging).arg(app);
    let output = match run_command_timeout(&mut cmd, COMMAND_TIMEOUT) {
        Some(output) => output,
        None => {
            log_stderr(format!("qlmanage timed out for {}", app.display()));
            let _ = fs::remove_dir_all(&staging);
            return false;
        }
    };
    if !output.status.success() {
        log_stderr(format!(
            "qlmanage failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
        let _ = fs::remove_dir_all(&staging);
        return false;
    }

    let file_name = format!(
        "{}.png",
        app.file_name().and_then(|n| n.to_str()).unwrap_or("app")
    );
    let generated = staging.join(file_name);
    let generated = if generated.is_file() {
        generated
    } else {
        match fs::read_dir(&staging).ok().and_then(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .find(|p| p.extension().and_then(|x| x.to_str()) == Some("png"))
        }) {
            Some(path) => path,
            None => {
                let _ = fs::remove_dir_all(&staging);
                return false;
            }
        }
    };
    let ok = fs::copy(&generated, out_png).is_ok() && out_png.is_file();
    let _ = fs::remove_dir_all(&staging);
    ok
}

fn run_command_timeout(cmd: &mut Command, timeout: Duration) -> Option<std::process::Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            log_stderr(format!("failed to spawn {:?}: {err}", cmd.get_program()));
            return None;
        }
    };
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(err) => {
                log_stderr(format!("wait failed for {:?}: {err}", cmd.get_program()));
                let _ = child.kill();
                return None;
            }
        }
    };

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = stdout_pipe.take() {
        let _ = pipe.read_to_end(&mut stdout);
    }
    if let Some(mut pipe) = stderr_pipe.take() {
        let _ = pipe.read_to_end(&mut stderr);
    }
    Some(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn tempfile_dir() -> Option<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = icon_cache_dir().join(format!("tmp-{}-{}", std::process::id(), nanos));
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}
