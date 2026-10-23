use serde::{Deserialize, Serialize};

use crate::ports::SocketPort;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cmd: String,
    pub user: String,
    pub cpu: f32,
    pub memory_bytes: u64,
    pub threads: u32,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exe: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Process start time as unix seconds, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time: Option<u64>,
    /// Bound/listening sockets owned by this process (may be empty).
    #[serde(default)]
    pub ports: Vec<SocketPort>,
    /// Stable key for lazy icon loading (e.g. macOS `.app` path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_key: Option<String>,
    /// Critical system process; terminate actions should be blocked.
    #[serde(default)]
    pub protected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySummary {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub process_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Local,
    Remote,
}

impl Scope {
    pub fn from_params(params: &serde_json::Value) -> Self {
        let scope = params
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if scope.eq_ignore_ascii_case("remote") || params.get("connectionId").is_some() {
            Scope::Remote
        } else {
            Scope::Local
        }
    }
}
