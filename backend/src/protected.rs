/// Critical system processes that must not be terminated from this plugin.
pub fn is_protected(pid: u32, name: &str) -> bool {
    if pid == 0 || pid == 1 {
        return true;
    }

    #[cfg(target_os = "windows")]
    {
        // Windows "System" process.
        if pid == 4 {
            return true;
        }
    }

    let name = name.trim();
    if name.is_empty() {
        return false;
    }

    // Kernel threads (Linux) show as [kworker/…]. Apply on every host OS so a
    // macOS/Windows sidecar managing a remote Linux box still blocks them.
    if name.starts_with('[') && name.ends_with(']') {
        return true;
    }

    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim_end_matches(".exe")
        .trim_end_matches(".EXE")
        .to_ascii_lowercase();

    matches!(
        base.as_str(),
        "launchd"
            | "kernel_task"
            | "init"
            | "systemd"
            | "kthreadd"
            | "system"
            | "registry"
            | "smss"
            | "csrss"
            | "wininit"
            | "services"
            | "lsass"
            | "winlogon"
            | "fontdrvhost"
    )
}

pub fn protected_message(pid: u32, name: &str) -> String {
    format!(
        "Refusing to signal protected system process {} (pid {})",
        if name.is_empty() { "unknown" } else { name },
        pid
    )
}
