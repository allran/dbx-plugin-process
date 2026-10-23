use std::collections::HashMap;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::util::log_stderr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortProtocol {
    Tcp,
    Udp,
}

impl PortProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            PortProtocol::Tcp => "tcp",
            PortProtocol::Udp => "udp",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocketPort {
    pub protocol: PortProtocol,
    pub port: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct PortQuery {
    pub port: u16,
    pub protocol: Option<PortProtocol>,
}

/// Map PID → bound/listening sockets (deduped, sorted).
pub fn sockets_by_pid() -> HashMap<u32, Vec<SocketPort>> {
    #[cfg(windows)]
    {
        parse_netstat_windows()
    }
    #[cfg(not(windows))]
    {
        parse_lsof_unix()
    }
}

fn push_socket(map: &mut HashMap<u32, Vec<SocketPort>>, pid: u32, protocol: PortProtocol, port: u16) {
    let sockets = map.entry(pid).or_default();
    let socket = SocketPort { protocol, port };
    if !sockets.contains(&socket) {
        sockets.push(socket);
    }
}

fn finalize(map: &mut HashMap<u32, Vec<SocketPort>>) {
    for sockets in map.values_mut() {
        sockets.sort_unstable();
    }
}

#[cfg(not(windows))]
fn parse_lsof_unix() -> HashMap<u32, Vec<SocketPort>> {
    let mut map = HashMap::new();
    merge_lsof(
        &mut map,
        &["-nP", "-iTCP", "-sTCP:LISTEN"],
        PortProtocol::Tcp,
    );
    merge_lsof(&mut map, &["-nP", "-iUDP"], PortProtocol::Udp);
    finalize(&mut map);
    map
}

#[cfg(not(windows))]
fn merge_lsof(map: &mut HashMap<u32, Vec<SocketPort>>, args: &[&str], protocol: PortProtocol) {
    let output = match Command::new("lsof").args(args).output() {
        Ok(output) if output.status.success() => output.stdout,
        Ok(output) => {
            log_stderr(format!(
                "lsof {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            return;
        }
        Err(err) => {
            log_stderr(format!("lsof unavailable: {err}"));
            return;
        }
    };

    let text = String::from_utf8_lossy(&output);
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 9 {
            continue;
        }
        let Ok(pid) = cols[1].parse::<u32>() else {
            continue;
        };
        let Some(port) = parse_endpoint_port(cols[8]) else {
            continue;
        };
        push_socket(map, pid, protocol, port);
    }
}

#[cfg(windows)]
fn parse_netstat_windows() -> HashMap<u32, Vec<SocketPort>> {
    let mut map = HashMap::new();
    merge_netstat_tcp(&mut map);
    merge_netstat_udp(&mut map);
    finalize(&mut map);
    map
}

#[cfg(windows)]
fn merge_netstat_tcp(map: &mut HashMap<u32, Vec<SocketPort>>) {
    let output = match Command::new("netstat").args(["-ano", "-p", "TCP"]).output() {
        Ok(output) if output.status.success() => output.stdout,
        Ok(output) => {
            log_stderr(format!(
                "netstat TCP failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            return;
        }
        Err(err) => {
            log_stderr(format!("netstat unavailable: {err}"));
            return;
        }
    };

    let text = String::from_utf8_lossy(&output);
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // Proto LocalAddress ForeignAddress State PID
        if cols.len() < 5 || !cols[0].eq_ignore_ascii_case("TCP") {
            continue;
        }
        if !cols[3].eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        let Ok(pid) = cols[4].parse::<u32>() else {
            continue;
        };
        let Some(port) = parse_endpoint_port(cols[1]) else {
            continue;
        };
        push_socket(map, pid, PortProtocol::Tcp, port);
    }
}

#[cfg(windows)]
fn merge_netstat_udp(map: &mut HashMap<u32, Vec<SocketPort>>) {
    let output = match Command::new("netstat").args(["-ano", "-p", "UDP"]).output() {
        Ok(output) if output.status.success() => output.stdout,
        Ok(output) => {
            log_stderr(format!(
                "netstat UDP failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            return;
        }
        Err(_) => return,
    };

    let text = String::from_utf8_lossy(&output);
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // Proto LocalAddress ForeignAddress PID
        if cols.len() < 4 || !cols[0].eq_ignore_ascii_case("UDP") {
            continue;
        }
        let Ok(pid) = cols[3].parse::<u32>() else {
            continue;
        };
        let Some(port) = parse_endpoint_port(cols[1]) else {
            continue;
        };
        push_socket(map, pid, PortProtocol::Udp, port);
    }
}

pub(crate) fn parse_endpoint_port(endpoint: &str) -> Option<u16> {
    // Examples: *:3000  127.0.0.1:8080  [::1]:5432  0.0.0.0:22
    // UDP lsof may look like: 192.168.1.1:5353->1.1.1.1:53 — use the local side.
    let endpoint = endpoint.trim();
    let local = endpoint.split_once("->").map(|(left, _)| left).unwrap_or(endpoint);
    if let Some(idx) = local.rfind(':') {
        let port = &local[idx + 1..];
        return port
            .trim_end_matches(|c: char| !c.is_ascii_digit())
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0);
    }
    None
}

/// Parse a search needle that looks like a port: `8080`, `:8080`, `tcp:8080`, `udp 53`.
pub fn parse_port_query(query: &str) -> Option<PortQuery> {
    let raw = query.trim().to_lowercase();
    if raw.is_empty() {
        return None;
    }

    let (protocol, rest) = if let Some(rest) = raw.strip_prefix("tcp:") {
        (Some(PortProtocol::Tcp), rest.trim())
    } else if let Some(rest) = raw.strip_prefix("tcp ") {
        (Some(PortProtocol::Tcp), rest.trim())
    } else if let Some(rest) = raw.strip_prefix("udp:") {
        (Some(PortProtocol::Udp), rest.trim())
    } else if let Some(rest) = raw.strip_prefix("udp ") {
        (Some(PortProtocol::Udp), rest.trim())
    } else if let Some(rest) = raw.strip_prefix("port:") {
        (None, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("port ") {
        (None, rest.trim())
    } else if let Some(rest) = raw.strip_prefix(':') {
        (None, rest.trim())
    } else {
        (None, raw.as_str())
    };

    if !rest.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let port = rest.parse::<u16>().ok().filter(|p| *p > 0)?;
    Some(PortQuery { port, protocol })
}

pub fn socket_matches(socket: &SocketPort, query: PortQuery) -> bool {
    if socket.port != query.port {
        return false;
    }
    match query.protocol {
        Some(protocol) => socket.protocol == protocol,
        None => true,
    }
}
