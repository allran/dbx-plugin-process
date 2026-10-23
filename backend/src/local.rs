use std::path::Path;
use std::thread;
use std::time::Duration;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, Signal, System, UpdateKind, Users};

use crate::icon::icon_key_for_exe;
use crate::model::{MemorySummary, ProcessInfo};
use crate::ports::{parse_port_query, socket_matches, sockets_by_pid, PortQuery};
use crate::protected::{is_protected, protected_message};
use crate::threads::thread_counts_by_pid;
use dbx_plugin_sdk::PluginError;

fn process_refresh_kind() -> ProcessRefreshKind {
    // sysinfo 0.38's bare `refresh_processes` skips cmd/user/cwd and includes Linux tasks.
    ProcessRefreshKind::everything()
        .without_tasks()
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_cmd(UpdateKind::OnlyIfNotSet)
        .with_cwd(UpdateKind::OnlyIfNotSet)
        .with_user(UpdateKind::OnlyIfNotSet)
}

fn refresh_process_snapshot(system: &mut System, processes_to_update: ProcessesToUpdate<'_>) {
    let kind = process_refresh_kind();
    system.refresh_processes_specifics(processes_to_update, true, kind);
    // CPU% needs two samples; a short pause is enough for a usable delta.
    thread::sleep(Duration::from_millis(120));
    system.refresh_processes_specifics(processes_to_update, true, kind);
}

pub fn summary() -> MemorySummary {
    let mut system = System::new();
    system.refresh_memory();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh_kind());

    MemorySummary {
        total_bytes: system.total_memory(),
        used_bytes: system.used_memory(),
        available_bytes: system.available_memory(),
        process_count: system.processes().len(),
    }
}

pub fn list_processes(query: Option<&str>, limit: usize) -> Vec<ProcessInfo> {
    let mut system = System::new();
    system.refresh_memory();
    refresh_process_snapshot(&mut system, ProcessesToUpdate::All);
    let users = Users::new_with_refreshed_list();
    let ports_by_pid = sockets_by_pid();
    let pids: Vec<u32> = system.processes().keys().map(|pid| pid.as_u32()).collect();
    let threads_by_pid = thread_counts_by_pid(pids);

    let items: Vec<ProcessInfo> = system
        .processes()
        .iter()
        .map(|(pid, process)| {
            let raw_name = process.name().to_string_lossy().into_owned();
            let exe_path = process.exe();
            let name = display_name(&raw_name, exe_path);
            let cmd = process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            let user = process
                .user_id()
                .and_then(|uid| users.get_user_by_id(uid))
                .map(|user| user.name().to_string())
                .unwrap_or_default();
            let pid_u32 = pid.as_u32();
            let icon_key = icon_key_for_exe(exe_path);
            let exe = exe_path.map(|path| path.to_string_lossy().into_owned());
            let cwd = process
                .cwd()
                .map(|path| path.to_string_lossy().into_owned());
            let start_time = {
                let value = process.start_time();
                if value > 0 {
                    Some(value)
                } else {
                    None
                }
            };
            let protected = is_protected(pid_u32, &name);
            ProcessInfo {
                pid: pid_u32,
                name,
                cmd,
                user,
                cpu: process.cpu_usage(),
                memory_bytes: process.memory(),
                threads: threads_by_pid.get(&pid_u32).copied().unwrap_or(0),
                status: format!("{:?}", process.status()),
                parent_pid: process.parent().map(|p| p.as_u32()),
                exe,
                cwd,
                start_time,
                ports: ports_by_pid.get(&pid_u32).cloned().unwrap_or_default(),
                icon_key,
                protected,
            }
        })
        .collect();

    filter_sort_limit(items, query, limit)
}

/// Filter / sort / truncate a process snapshot (shared by local + remote).
pub fn filter_sort_limit(
    mut items: Vec<ProcessInfo>,
    query: Option<&str>,
    limit: usize,
) -> Vec<ProcessInfo> {
    let needle = query
        .map(|q| q.trim().to_lowercase())
        .filter(|q| !q.is_empty());
    let port_query = needle.as_deref().and_then(parse_port_query);
    if needle.is_some() {
        items.retain(|item| matches_query(item, needle.as_deref(), port_query));
    }
    items.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));
    if limit > 0 && items.len() > limit {
        items.truncate(limit);
    }
    items
}

fn matches_query(item: &ProcessInfo, needle: Option<&str>, port_query: Option<PortQuery>) -> bool {
    let Some(needle) = needle else {
        return true;
    };

    let port_only = needle.starts_with(':')
        || needle.starts_with("port:")
        || needle.starts_with("port ")
        || needle.starts_with("tcp:")
        || needle.starts_with("tcp ")
        || needle.starts_with("udp:")
        || needle.starts_with("udp ");
    if port_only {
        return port_query
            .map(|query| item.ports.iter().any(|socket| socket_matches(socket, query)))
            .unwrap_or(false);
    }

    // `8080` matches name / PID / bound port (any protocol).
    if let Some(query) = port_query {
        if item.ports.iter().any(|socket| socket_matches(socket, query)) {
            return true;
        }
    }

    // Match display name / PID / ports only — not exe/cmd/path (too noisy).
    let pid_text = item.pid.to_string();
    item.name.to_lowercase().contains(needle)
        || pid_text == *needle
        || (needle.len() >= 2 && pid_text.contains(needle))
        || item.ports.iter().any(|socket| {
            socket.port.to_string().contains(needle)
                || format!("{} {}", socket.protocol.as_str(), socket.port).contains(needle)
                || format!("{}:{}", socket.protocol.as_str(), socket.port).contains(needle)
        })
}

/// Prefer a stable app/binary name over macOS `comm` truncation
/// (e.g. `/Applications/Al` → `AlDente` from the exe path).
fn display_name(raw_name: &str, exe: Option<&Path>) -> String {
    if let Some(from_exe) = exe.and_then(name_from_exe) {
        let raw = raw_name.trim();
        // Truncated path-like comm, empty, or clearly not the real binary name.
        if raw.is_empty()
            || raw.contains('/')
            || raw.contains('\\')
            || (raw.len() >= 15 && !raw.eq_ignore_ascii_case(&from_exe))
        {
            return from_exe;
        }
    }
    raw_name.to_string()
}

fn name_from_exe(exe: &Path) -> Option<String> {
    let file_name = exe.file_name()?.to_str()?.to_string();
    if !file_name.is_empty() {
        return Some(file_name);
    }
    None
}

pub fn related_processes(pid: u32, child_limit: usize) -> Result<(Option<ProcessInfo>, Vec<ProcessInfo>), PluginError> {
    let all = list_processes(None, 0);
    let current = all
        .iter()
        .find(|item| item.pid == pid)
        .cloned()
        .ok_or_else(|| PluginError::new(-32602, format!("Process {pid} not found")))?;

    let parent = current
        .parent_pid
        .and_then(|parent_pid| all.iter().find(|item| item.pid == parent_pid).cloned());

    let mut children: Vec<ProcessInfo> = all
        .into_iter()
        .filter(|item| item.parent_pid == Some(pid))
        .collect();
    children.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));
    let limit = if child_limit == 0 { 100 } else { child_limit.min(500) };
    if children.len() > limit {
        children.truncate(limit);
    }

    Ok((parent, children))
}

pub fn kill_process(pid: u32, signal: &str) -> Result<(), PluginError> {
    let target = sysinfo::Pid::from_u32(pid);
    let mut system = System::new();
    // Only refresh the target PID — a full process scan made kill feel stuck.
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[target]),
        true,
        process_refresh_kind(),
    );

    let Some(process) = system.process(target) else {
        return Err(PluginError::new(-32602, format!("Process {pid} not found")));
    };

    let name = process.name().to_string_lossy();
    if is_protected(pid, &name) {
        return Err(PluginError::new(-32000, protected_message(pid, &name)));
    }

    let signal = match signal.to_ascii_uppercase().as_str() {
        "" | "TERM" | "SIGTERM" => Signal::Term,
        "KILL" | "SIGKILL" => Signal::Kill,
        "INT" | "SIGINT" => Signal::Interrupt,
        other => {
            return Err(PluginError::new(
                -32602,
                format!("Unsupported signal: {other}"),
            ))
        }
    };

    if process.kill_with(signal).unwrap_or(false) {
        Ok(())
    } else {
        Err(PluginError::new(
            -32000,
            format!("Failed to signal process {pid}"),
        ))
    }
}
