use std::collections::HashMap;

/// Best-effort thread counts keyed by PID.
pub fn thread_counts_by_pid(pids: impl IntoIterator<Item = u32>) -> HashMap<u32, u32> {
    #[cfg(target_os = "linux")]
    {
        let _ = pids;
        linux_thread_counts()
    }
    #[cfg(target_os = "macos")]
    {
        macos_thread_counts(pids)
    }
    #[cfg(target_os = "windows")]
    {
        let _ = pids;
        windows_thread_counts()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = pids;
        HashMap::new()
    }
}

#[cfg(target_os = "linux")]
fn linux_thread_counts() -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return map;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let status = entry.path().join("status");
        let Ok(text) = std::fs::read_to_string(status) else {
            continue;
        };
        for line in text.lines() {
            if let Some(value) = line.strip_prefix("Threads:") {
                if let Ok(count) = value.trim().parse::<u32>() {
                    map.insert(pid, count);
                }
                break;
            }
        }
    }
    map
}

#[cfg(target_os = "macos")]
fn macos_thread_counts(pids: impl IntoIterator<Item = u32>) -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    for pid in pids {
        if let Some(count) = macos_thread_count(pid) {
            map.insert(pid, count);
        }
    }
    map
}

#[cfg(target_os = "macos")]
fn macos_thread_count(pid: u32) -> Option<u32> {
    unsafe {
        let mut info: libc::proc_taskinfo = std::mem::zeroed();
        let size = std::mem::size_of::<libc::proc_taskinfo>() as i32;
        let ret = libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTASKINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        );
        if ret == size && info.pti_threadnum >= 0 {
            Some(info.pti_threadnum as u32)
        } else {
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_thread_counts() -> HashMap<u32, u32> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };

    let mut map = HashMap::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else {
            return map;
        };
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        if Thread32First(snapshot, &mut entry).is_ok() {
            loop {
                *map.entry(entry.th32OwnerProcessID).or_insert(0) += 1;
                if Thread32Next(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    map
}
