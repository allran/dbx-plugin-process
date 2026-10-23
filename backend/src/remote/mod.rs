mod collect;
mod ssh;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dbx_plugin_sdk::PluginError;
use serde_json::{json, Value};

use crate::local;
use crate::model::{MemorySummary, ProcessInfo};
use crate::protected::{is_protected, protected_message};
use crate::util::log_stderr;

use collect::{parse_memory_summary, parse_process_table, parse_remote_ports, RemoteOs};
use ssh::{exec_checked, open_session, AuthMaterial, SshHandle};

/// Reuse a fresh process snapshot across list + related within the same UI action.
const SNAPSHOT_TTL: Duration = Duration::from_millis(900);

/// Live remote SSH sessions keyed by `connection.id`.
#[derive(Default)]
pub struct SessionStore {
    inner: Mutex<HashMap<String, Arc<RemoteSession>>>,
}

struct RemoteSession {
    handle: SshHandle,
    os: RemoteOs,
    snapshot_cache: Mutex<Option<(Instant, Vec<ProcessInfo>)>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn test(&self, params: &Value) -> Value {
        match self.open_ephemeral(params) {
            Ok((host, port, os)) => json!({
                "success": true,
                "message": format!("SSH OK ({os:?}) via {host}:{port}")
            }),
            Err(err) => json!({
                "success": false,
                "message": err.message
            }),
        }
    }

    pub fn connect(&self, params: &Value) -> Result<Value, PluginError> {
        let connection = params
            .get("connection")
            .ok_or_else(|| PluginError::new(-32602, "Missing connection"))?;
        let connection_id = connection
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| PluginError::new(-32602, "Missing connection id"))?
            .to_string();

        if let Some(existing) = self
            .inner
            .lock()
            .map_err(|_| PluginError::new(-32000, "Connection registry is poisoned"))?
            .get(&connection_id)
            .cloned()
        {
            // Idempotent connect: keep the session only if it still answers.
            if exec_checked(&existing.handle, "echo ok").is_ok() {
                return Ok(json!({ "success": true }));
            }
            let _ = self.disconnect(&connection_id);
        }

        let (handle, os, host, port) = self.handshake(params)?;
        let session = Arc::new(RemoteSession {
            handle,
            os,
            snapshot_cache: Mutex::new(None),
        });
        if let Some(previous) = self
            .inner
            .lock()
            .map_err(|_| PluginError::new(-32000, "Connection registry is poisoned"))?
            .insert(connection_id.clone(), session)
        {
            let _ = ssh::block_on(previous.handle.disconnect());
        }
        log_stderr(format!(
            "connected {connection_id} via SSH {host}:{port} ({os:?})"
        ));
        Ok(json!({ "success": true }))
    }

    pub fn disconnect(&self, connection_id: &str) -> Result<Value, PluginError> {
        let removed = self
            .inner
            .lock()
            .map_err(|_| PluginError::new(-32000, "Connection registry is poisoned"))?
            .remove(connection_id);
        if let Some(session) = removed {
            let _ = ssh::block_on(session.handle.disconnect());
            log_stderr(format!("disconnected {connection_id}"));
        }
        Ok(json!({ "success": true }))
    }

    pub fn summary(&self, connection_id: &str) -> Result<MemorySummary, PluginError> {
        let session = self.require(connection_id)?;
        let script = match session.os {
            RemoteOs::Linux => {
                "grep -E '^(MemTotal|MemAvailable|MemFree):' /proc/meminfo 2>/dev/null; echo '---'; ps -e --no-headers 2>/dev/null | wc -l"
            }
            RemoteOs::Darwin => {
                "echo MEMSIZE=$(sysctl -n hw.memsize 2>/dev/null); echo PAGESIZE=$(pagesize 2>/dev/null || sysctl -n hw.pagesize 2>/dev/null); vm_stat 2>/dev/null; echo '---'; ps -ax 2>/dev/null | tail -n +2 | wc -l"
            }
            RemoteOs::Unknown => {
                return Err(PluginError::new(
                    -32000,
                    "Unsupported remote OS for memory summary",
                ))
            }
        };
        let output = exec_checked(&session.handle, script)?;
        parse_memory_summary(session.os, &output)
    }

    pub fn list_processes(
        &self,
        connection_id: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ProcessInfo>, PluginError> {
        let session = self.require(connection_id)?;
        let items = self.snapshot(&session)?;
        Ok(local::filter_sort_limit(items, query, limit))
    }

    pub fn related_processes(
        &self,
        connection_id: &str,
        pid: u32,
        child_limit: usize,
    ) -> Result<(Option<ProcessInfo>, Vec<ProcessInfo>), PluginError> {
        let session = self.require(connection_id)?;
        let all = self.snapshot(&session)?;
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
        let limit = if child_limit == 0 {
            100
        } else {
            child_limit.min(500)
        };
        if children.len() > limit {
            children.truncate(limit);
        }
        Ok((parent, children))
    }

    pub fn kill_process(
        &self,
        connection_id: &str,
        pid: u32,
        signal: &str,
    ) -> Result<(), PluginError> {
        let session = self.require(connection_id)?;
        let name = exec_checked(
            &session.handle,
            &format!("ps -p {pid} -o comm= 2>/dev/null || true"),
        )?
        .trim()
        .to_string();
        if name.is_empty() {
            return Err(PluginError::new(
                -32602,
                format!("Process {pid} not found"),
            ));
        }
        if is_protected(pid, &name) {
            return Err(PluginError::new(-32000, protected_message(pid, &name)));
        }
        let sig = match signal.to_ascii_uppercase().as_str() {
            "" | "TERM" | "SIGTERM" => "TERM",
            "KILL" | "SIGKILL" => "KILL",
            "INT" | "SIGINT" => "INT",
            other => {
                return Err(PluginError::new(
                    -32602,
                    format!("Unsupported signal: {other}"),
                ))
            }
        };
        let output = exec_checked(
            &session.handle,
            &format!("kill -{sig} {pid} 2>&1; echo EXIT:$?"),
        )?;
        if let Ok(mut guard) = session.snapshot_cache.lock() {
            *guard = None;
        }
        // Match a whole line — `contains("EXIT:0")` would also match `EXIT:10`.
        if output.lines().any(|line| line.trim() == "EXIT:0") {
            Ok(())
        } else {
            let detail = output
                .lines()
                .find(|line| !line.trim().starts_with("EXIT:"))
                .unwrap_or("kill failed")
                .trim();
            Err(PluginError::new(
                -32000,
                format!("Failed to signal process {pid}: {detail}"),
            ))
        }
    }

    fn require(&self, connection_id: &str) -> Result<Arc<RemoteSession>, PluginError> {
        self.inner
            .lock()
            .map_err(|_| PluginError::new(-32000, "Connection registry is poisoned"))?
            .get(connection_id)
            .cloned()
            .ok_or_else(|| PluginError::new(-32000, "Connection is not active"))
    }

    fn snapshot(&self, session: &RemoteSession) -> Result<Vec<ProcessInfo>, PluginError> {
        if let Ok(guard) = session.snapshot_cache.lock() {
            if let Some((at, items)) = guard.as_ref() {
                if at.elapsed() <= SNAPSHOT_TTL {
                    return Ok(items.clone());
                }
            }
        }

        let ps_cmd = match session.os {
            RemoteOs::Linux => {
                "LC_ALL=C ps -eo pid=,ppid=,user=,%cpu=,rss=,nlwp=,state=,etimes=,comm=,args= --no-headers"
            }
            RemoteOs::Darwin => {
                // thcount is available on modern macOS; fall back parser tolerates missing column.
                "LC_ALL=C ps -axo pid=,ppid=,user=,%cpu=,rss=,thcount=,state=,etime=,comm=,command="
            }
            RemoteOs::Unknown => {
                return Err(PluginError::new(
                    -32000,
                    "Unsupported remote OS for process list",
                ))
            }
        };
        let ports_cmd = match session.os {
            RemoteOs::Linux => {
                "{ ss -lntp 2>/dev/null; ss -lnup 2>/dev/null; } || { lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null; lsof -nP -iUDP 2>/dev/null; }"
            }
            RemoteOs::Darwin => {
                "{ lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null; lsof -nP -iUDP 2>/dev/null; }"
            }
            RemoteOs::Unknown => "true",
        };
        let combined = format!(
            "echo '__DBX_PS__'; {ps_cmd}; echo '__DBX_PORTS__'; {ports_cmd}; true"
        );
        let output = exec_checked(&session.handle, &combined)?;
        let (ps_text, ports_text) = split_sections(&output);
        let ports = parse_remote_ports(session.os, ports_text);
        let items = parse_process_table(session.os, ps_text, &ports);
        if let Ok(mut guard) = session.snapshot_cache.lock() {
            *guard = Some((Instant::now(), items.clone()));
        }
        Ok(items)
    }

    fn open_ephemeral(&self, params: &Value) -> Result<(String, u16, RemoteOs), PluginError> {
        let (handle, os, host, port) = self.handshake(params)?;
        let _ = ssh::block_on(handle.disconnect());
        Ok((host, port, os))
    }

    fn handshake(
        &self,
        params: &Value,
    ) -> Result<(SshHandle, RemoteOs, String, u16), PluginError> {
        let connection = params.get("connection").unwrap_or(&Value::Null);
        let runtime = params.get("runtime").unwrap_or(&Value::Null);

        let via_tunnel = runtime.get("host").and_then(value_as_string).is_some();
        let host = runtime
            .get("host")
            .and_then(value_as_string)
            .or_else(|| connection.get("host").and_then(value_as_string))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| PluginError::new(-32602, "Missing SSH host"))?;
        let port = runtime
            .get("port")
            .and_then(value_as_u16)
            .or_else(|| connection.get("port").and_then(value_as_u16))
            .unwrap_or(22);
        if !via_tunnel {
            log_stderr(format!(
                "SSH runtime.host missing — dialing connection host {host}:{port} directly"
            ));
        }

        let username = connection_str(connection, &["username", "user"]).ok_or_else(|| {
            PluginError::new(-32602, "Missing username for SSH connection")
        })?;
        let password = connection_str(connection, &["password"]).unwrap_or_default();
        let private_key_path = connection_str(
            connection,
            &["private_key_path", "privateKeyPath", "identity_file"],
        );
        let private_key_pem =
            connection_str(connection, &["private_key", "privateKey", "private_key_pem"]);
        let private_key_passphrase =
            connection_str(connection, &["private_key_passphrase", "passphrase"]);
        let auth = AuthMaterial {
            username,
            password,
            private_key_path,
            private_key_pem,
            private_key_passphrase,
        };
        if !auth.has_password() && !auth.has_key() {
            return Err(PluginError::new(
                -32602,
                "Provide an SSH password or private key",
            ));
        }
        let timeout_secs = connection_u64(connection, "connect_timeout_secs")
            .unwrap_or(30)
            .clamp(5, 120);

        let handle = open_session(&host, port, &auth, timeout_secs)?;
        let uname = exec_checked(&handle, "uname -s 2>/dev/null || echo Unknown")?;
        let os = RemoteOs::from_uname(uname.trim());
        if matches!(os, RemoteOs::Unknown) {
            let _ = ssh::block_on(handle.disconnect());
            return Err(PluginError::new(
                -32000,
                format!(
                    "Unsupported remote OS '{}' at {host}:{port} (need Linux or macOS)",
                    uname.trim()
                ),
            ));
        }
        Ok((handle, os, host, port))
    }
}

fn connection_str(connection: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        for container in [
            connection,
            connection.get("connection_secrets").unwrap_or(&Value::Null),
            connection.get("secrets").unwrap_or(&Value::Null),
            connection.get("external_config").unwrap_or(&Value::Null),
            connection.get("config").unwrap_or(&Value::Null),
        ] {
            if let Some(value) = container
                .get(key)
                .and_then(value_as_string)
                .filter(|s| !s.is_empty())
            {
                return Some(value);
            }
        }
    }
    None
}

fn connection_u64(connection: &Value, key: &str) -> Option<u64> {
    for container in [
        connection,
        connection.get("external_config").unwrap_or(&Value::Null),
        connection.get("config").unwrap_or(&Value::Null),
    ] {
        if let Some(value) = container.get(key).and_then(Value::as_u64) {
            return Some(value);
        }
        if let Some(value) = container
            .get(key)
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
        {
            return Some(value);
        }
    }
    None
}

fn value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn value_as_u16(value: &Value) -> Option<u16> {
    match value {
        Value::Number(n) => n.as_u64().and_then(|v| u16::try_from(v).ok()).filter(|p| *p > 0),
        Value::String(s) => s.trim().parse::<u16>().ok().filter(|p| *p > 0),
        _ => None,
    }
}

fn split_sections(output: &str) -> (&str, &str) {
    let Some(ports_at) = output.find("__DBX_PORTS__") else {
        let ps = output
            .strip_prefix("__DBX_PS__")
            .unwrap_or(output)
            .trim_start_matches(|c: char| c == '\n' || c == '\r');
        return (ps, "");
    };
    let ps = output[..ports_at]
        .trim_start_matches("__DBX_PS__")
        .trim_start_matches(|c: char| c == '\n' || c == '\r');
    let ports = output[ports_at + "__DBX_PORTS__".len()..]
        .trim_start_matches(|c: char| c == '\n' || c == '\r');
    (ps, ports)
}
