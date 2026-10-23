use dbx_plugin_sdk::{PluginEmitter, PluginError, PluginHandler, RequestContext};
use serde_json::{json, Value};

use crate::icon;
use crate::local;
use crate::model::Scope;
use crate::remote::SessionStore;
use crate::reveal;
use crate::util::log_stderr;

pub struct Plugin {
    sessions: SessionStore,
}

impl Plugin {
    pub fn new() -> Self {
        Self {
            sessions: SessionStore::new(),
        }
    }
}

impl PluginHandler for Plugin {
    fn handle(
        &self,
        _context: RequestContext,
        method: &str,
        params: Value,
        _emitter: &PluginEmitter,
    ) -> Result<Value, PluginError> {
        match method {
            "connection/test" => Ok(self.sessions.test(&params)),
            "connection/connect" => self.sessions.connect(&params),
            "connection/disconnect" => {
                let connection_id = connection_id(&params)?;
                self.sessions.disconnect(connection_id)
            }
            "process/summary" => {
                let scope = Scope::from_params(&params);
                let summary = match scope {
                    Scope::Local => local::summary(),
                    Scope::Remote => {
                        let connection_id = require_connection_id(&params)?;
                        self.sessions.summary(connection_id)?
                    }
                };
                Ok(json!(summary))
            }
            "process/list" => {
                let scope = Scope::from_params(&params);
                let query = params.get("query").and_then(Value::as_str);
                let limit = params
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(500)
                    .min(5_000) as usize;
                let items = match scope {
                    Scope::Local => local::list_processes(query, limit),
                    Scope::Remote => {
                        let connection_id = require_connection_id(&params)?;
                        self.sessions.list_processes(connection_id, query, limit)?
                    }
                };
                Ok(json!({
                    "scope": if scope == Scope::Local { "local" } else { "remote" },
                    "items": items,
                    "total": items.len()
                }))
            }
            "process/icon" => {
                let key = params
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| PluginError::new(-32602, "Missing key"))?;
                let data_url = icon::icon_data_url(key)?;
                Ok(json!({ "key": key, "dataUrl": data_url }))
            }
            "process/reveal-path" => {
                if Scope::from_params(&params) == Scope::Remote {
                    return Err(PluginError::new(
                        -32000,
                        "Reveal is only available for local processes",
                    ));
                }
                let path = params
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| PluginError::new(-32602, "Missing path"))?;
                reveal::reveal_path(path)?;
                Ok(json!({ "success": true }))
            }
            "process/related" => {
                let scope = Scope::from_params(&params);
                let pid = params
                    .get("pid")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| PluginError::new(-32602, "Missing pid"))?
                    as u32;
                let child_limit = params
                    .get("childLimit")
                    .and_then(Value::as_u64)
                    .unwrap_or(100) as usize;
                let (parent, children) = match scope {
                    Scope::Local => local::related_processes(pid, child_limit)?,
                    Scope::Remote => {
                        let connection_id = require_connection_id(&params)?;
                        self.sessions
                            .related_processes(connection_id, pid, child_limit)?
                    }
                };
                Ok(json!({
                    "pid": pid,
                    "parent": parent,
                    "children": children,
                    "childCount": children.len()
                }))
            }
            "process/kill" => {
                let scope = Scope::from_params(&params);
                let pid = params
                    .get("pid")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| PluginError::new(-32602, "Missing pid"))?
                    as u32;
                let signal = params
                    .get("signal")
                    .and_then(Value::as_str)
                    .unwrap_or("TERM");
                match scope {
                    Scope::Local => local::kill_process(pid, signal)?,
                    Scope::Remote => {
                        let connection_id = require_connection_id(&params)?;
                        self.sessions.kill_process(connection_id, pid, signal)?;
                    }
                }
                Ok(json!({ "success": true, "pid": pid, "signal": signal }))
            }
            "process/ping" => Ok(json!({
                "ok": true,
                "plugin": "io.dbx.process",
                "language": "rust",
                "scope": match Scope::from_params(&params) {
                    Scope::Local => "local",
                    Scope::Remote => "remote",
                },
                "connectionId": params.get("connectionId").cloned().unwrap_or(Value::Null)
            })),
            _ => {
                log_stderr(format!("unknown method: {method}"));
                Err(PluginError::method_not_found(method))
            }
        }
    }
}

fn connection_id(params: &Value) -> Result<&str, PluginError> {
    params
        .get("connection")
        .and_then(|connection| connection.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::new(-32602, "Missing connection id"))
}

fn require_connection_id(params: &Value) -> Result<&str, PluginError> {
    params
        .get("connectionId")
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::new(-32602, "Missing connectionId"))
}
