use std::collections::HashMap;

use dbx_plugin_sdk::PluginError;

use crate::model::{MemorySummary, ProcessInfo};
use crate::ports::{parse_endpoint_port, PortProtocol, SocketPort};
use crate::protected::is_protected;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteOs {
    Linux,
    Darwin,
    Unknown,
}

impl RemoteOs {
    pub fn from_uname(uname: &str) -> Self {
        let value = uname.trim().to_ascii_lowercase();
        if value.contains("linux") {
            RemoteOs::Linux
        } else if value.contains("darwin") {
            RemoteOs::Darwin
        } else {
            RemoteOs::Unknown
        }
    }
}

pub fn parse_process_table(
    os: RemoteOs,
    text: &str,
    ports: &HashMap<u32, Vec<SocketPort>>,
) -> Vec<ProcessInfo> {
    text.lines()
        .filter_map(|line| parse_ps_line(os, line, ports))
        .collect()
}

fn parse_ps_line(
    os: RemoteOs,
    line: &str,
    ports: &HashMap<u32, Vec<SocketPort>>,
) -> Option<ProcessInfo> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let mut parts = line.split_whitespace();

    let pid: u32 = parts.next()?.parse().ok()?;
    let parent_pid = parts.next()?.parse::<u32>().ok();
    let user = parts.next()?.to_string();
    let cpu: f32 = parts.next()?.parse().unwrap_or(0.0);
    let rss_kb: u64 = parts.next()?.parse().unwrap_or(0);
    let (threads, status, start_time, name, cmd) = match os {
        RemoteOs::Linux => {
            let threads = parts.next()?.parse::<u32>().unwrap_or(0);
            let status = parts.next()?.to_string();
            let etimes = parts.next()?.parse::<u64>().ok();
            let name = parts.next()?.to_string();
            let cmd = rest_join(parts);
            let start_time = etimes.map(|elapsed| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                now.saturating_sub(elapsed)
            });
            (threads, status, start_time, name, cmd)
        }
        RemoteOs::Darwin => {
            // Prefer `thcount` when present: pid ppid user cpu rss thcount state etime comm command
            let next = parts.next()?.to_string();
            let (threads, status) = if next.chars().all(|c| c.is_ascii_digit()) {
                (next.parse::<u32>().unwrap_or(0), parts.next()?.to_string())
            } else {
                (0, next)
            };
            let _etime = parts.next()?;
            let name = parts.next()?.to_string();
            let cmd = rest_join(parts);
            (threads, status, None, name, cmd)
        }
        RemoteOs::Unknown => return None,
    };

    let name = if name.is_empty() {
        cmd.split_whitespace().next().unwrap_or("process").to_string()
    } else {
        name
    };

    Some(ProcessInfo {
        pid,
        name: name.clone(),
        cmd,
        user,
        cpu,
        memory_bytes: rss_kb.saturating_mul(1024),
        threads,
        status,
        parent_pid,
        exe: None,
        cwd: None,
        start_time,
        ports: ports.get(&pid).cloned().unwrap_or_default(),
        icon_key: None,
        protected: is_protected(pid, &name),
    })
}

fn rest_join<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    parts.collect::<Vec<_>>().join(" ")
}

pub fn parse_remote_ports(os: RemoteOs, text: &str) -> HashMap<u32, Vec<SocketPort>> {
    let mut map = HashMap::new();
    if text.trim().is_empty() {
        return map;
    }

    let looks_like_ss = text.lines().any(|line| {
        let upper = line.trim_start().to_ascii_uppercase();
        upper.starts_with("LISTEN") || upper.starts_with("UNCONN") || upper.starts_with("UDP")
    }) || text.contains("users:((");

    if looks_like_ss || os == RemoteOs::Linux {
        parse_ss_text(text, &mut map);
    }
    if map.is_empty() {
        parse_lsof_text(text, &mut map);
    }

    for sockets in map.values_mut() {
        sockets.sort_unstable();
    }
    map
}

fn parse_ss_text(text: &str, map: &mut HashMap<u32, Vec<SocketPort>>) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(protocol) = ss_line_protocol(line) else {
            continue;
        };
        let Some(pid) = extract_ss_pid(line) else {
            continue;
        };
        let Some(port) = extract_ss_local_port(line) else {
            continue;
        };
        push_socket(map, pid, protocol, port);
    }
}

fn ss_line_protocol(line: &str) -> Option<PortProtocol> {
    let lower = line.to_ascii_lowercase();
    let first = lower.split_whitespace().next().unwrap_or("");
    // Prefer netid / state column — never sniff process names like `udp-proxy`.
    if first == "tcp" || first.starts_with("tcp") {
        return Some(PortProtocol::Tcp);
    }
    if first == "udp" || first.starts_with("udp") {
        return Some(PortProtocol::Udp);
    }
    if first == "listen" || first == "estab" || first == "time-wait" || first == "close-wait" {
        return Some(PortProtocol::Tcp);
    }
    if first == "unconn" || first == "unconnected" {
        return Some(PortProtocol::Udp);
    }
    None
}

fn extract_ss_pid(line: &str) -> Option<u32> {
    let idx = line.find("pid=")?;
    let rest = &line[idx + 4..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn extract_ss_local_port(line: &str) -> Option<u16> {
    // Columns vary; find first token that looks like host:port / *:port / [::]:port
    for token in line.split_whitespace() {
        if let Some(port) = parse_endpoint_port(token) {
            return Some(port);
        }
    }
    None
}

fn parse_lsof_text(text: &str, map: &mut HashMap<u32, Vec<SocketPort>>) {
    for line in text.lines().skip(0) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 9 {
            continue;
        }
        // Skip header
        if cols[0].eq_ignore_ascii_case("COMMAND") {
            continue;
        }
        let Ok(pid) = cols[1].parse::<u32>() else {
            continue;
        };
        let name = cols.get(8).copied().unwrap_or("");
        let Some(port) = parse_endpoint_port(name) else {
            continue;
        };
        let protocol = if cols.iter().any(|c| c.eq_ignore_ascii_case("TCP")) {
            PortProtocol::Tcp
        } else if cols.iter().any(|c| c.eq_ignore_ascii_case("UDP")) {
            PortProtocol::Udp
        } else if name.to_ascii_uppercase().contains("UDP") {
            PortProtocol::Udp
        } else {
            PortProtocol::Tcp
        };
        push_socket(map, pid, protocol, port);
    }
}

fn push_socket(map: &mut HashMap<u32, Vec<SocketPort>>, pid: u32, protocol: PortProtocol, port: u16) {
    let sockets = map.entry(pid).or_default();
    let socket = SocketPort { protocol, port };
    if !sockets.contains(&socket) {
        sockets.push(socket);
    }
}

pub fn parse_memory_summary(os: RemoteOs, text: &str) -> Result<MemorySummary, PluginError> {
    let (head, count_part) = text
        .split_once("---")
        .map(|(a, b)| (a, b))
        .unwrap_or((text, ""));
    let process_count = count_part
        .lines()
        .rev()
        .find_map(|line| line.trim().parse::<usize>().ok())
        .unwrap_or(0);

    match os {
        RemoteOs::Linux => {
            let mut total_kb = 0u64;
            let mut available_kb = 0u64;
            let mut free_kb = 0u64;
            for line in head.lines() {
                let mut parts = line.split_whitespace();
                let key = parts.next().unwrap_or("");
                let value = parts.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
                match key {
                    "MemTotal:" => total_kb = value,
                    "MemAvailable:" => available_kb = value,
                    "MemFree:" => free_kb = value,
                    _ => {}
                }
            }
            if available_kb == 0 {
                available_kb = free_kb;
            }
            let total_bytes = total_kb.saturating_mul(1024);
            let available_bytes = available_kb.saturating_mul(1024);
            Ok(MemorySummary {
                total_bytes,
                used_bytes: total_bytes.saturating_sub(available_bytes),
                available_bytes,
                process_count,
            })
        }
        RemoteOs::Darwin => {
            let mut total_bytes = 0u64;
            let mut page_size = 4096u64;
            let mut free_pages = 0u64;
            let mut inactive_pages = 0u64;
            for line in head.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("MEMSIZE=") {
                    total_bytes = rest.trim().parse().unwrap_or(0);
                } else if let Some(rest) = line.strip_prefix("PAGESIZE=") {
                    page_size = rest.trim().parse().unwrap_or(4096);
                } else if let Some(rest) = line.strip_prefix("Pages free:") {
                    free_pages = parse_vm_stat_number(rest);
                } else if let Some(rest) = line.strip_prefix("Pages inactive:") {
                    inactive_pages = parse_vm_stat_number(rest);
                }
            }
            let available_bytes = (free_pages + inactive_pages).saturating_mul(page_size);
            Ok(MemorySummary {
                total_bytes,
                used_bytes: total_bytes.saturating_sub(available_bytes),
                available_bytes,
                process_count,
            })
        }
        RemoteOs::Unknown => Err(PluginError::new(-32000, "Unsupported remote OS")),
    }
}

fn parse_vm_stat_number(raw: &str) -> u64 {
    raw.chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}
