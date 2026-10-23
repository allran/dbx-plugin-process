use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use dbx_plugin_sdk::PluginError;
use russh::client::{self, Handle};
use russh::keys::{
    decode_secret_key, load_secret_key, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate,
};
use russh::{ChannelMsg, Disconnect};
use tokio::runtime::Runtime;
use tokio::sync::Mutex;
use tokio::time::timeout;

use crate::util::log_stderr;

struct ClientHandler;

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // Prefer DBX tunnel (`runtime.host`). When falling back to a raw host we still
        // accept the key — Host currently has no TOFU UI wired for this plugin.
        Ok(true)
    }
}

/// Auth material for an SSH handshake (password and/or private key).
#[derive(Debug, Clone)]
pub struct AuthMaterial {
    pub username: String,
    pub password: String,
    /// Absolute path to a private key on the sidecar host (desktop picker).
    pub private_key_path: Option<String>,
    /// PEM / OpenSSH private key body (browser `content_field` or pasted secret).
    pub private_key_pem: Option<String>,
    /// Passphrase for an encrypted private key (reuses password when unset).
    pub private_key_passphrase: Option<String>,
}

impl AuthMaterial {
    pub fn has_key(&self) -> bool {
        self.private_key_path
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
            || self
                .private_key_pem
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
    }

    pub fn has_password(&self) -> bool {
        !self.password.is_empty()
    }
}

/// Shared SSH client handle for a connected remote host.
pub struct SshHandle {
    inner: Arc<Mutex<Handle<ClientHandler>>>,
}

impl SshHandle {
    pub async fn disconnect(&self) -> Result<(), PluginError> {
        let guard = self.inner.lock().await;
        guard
            .disconnect(Disconnect::ByApplication, "", "en")
            .await
            .map_err(|err| PluginError::new(-32000, format!("SSH disconnect failed: {err}")))
    }
}

fn runtime() -> &'static Runtime {
    static RUNTIME: std::sync::OnceLock<Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("dbx-process-ssh")
            .build()
            .expect("failed to create SSH tokio runtime")
    })
}

pub fn block_on<T>(fut: impl std::future::Future<Output = T>) -> T {
    runtime().block_on(fut)
}

pub fn open_session(
    host: &str,
    port: u16,
    auth: &AuthMaterial,
    timeout_secs: u64,
) -> Result<SshHandle, PluginError> {
    if !auth.has_password() && !auth.has_key() {
        return Err(PluginError::new(
            -32602,
            "Provide an SSH password or private key",
        ));
    }

    block_on(async {
        timeout(Duration::from_secs(timeout_secs), async {
            let config = Arc::new(client::Config {
                inactivity_timeout: Some(Duration::from_secs(
                    timeout_secs.saturating_mul(2).max(60),
                )),
                ..Default::default()
            });
            let mut handle = client::connect(config, (host, port), ClientHandler {})
                .await
                .map_err(|err| {
                    PluginError::new(-32000, format!("SSH connect to {host}:{port} failed: {err}"))
                })?;

            let mut authenticated = false;
            let mut last_error = String::new();

            if auth.has_key() {
                match load_private_key(auth) {
                    Ok(key) => {
                        let hash = handle
                            .best_supported_rsa_hash()
                            .await
                            .map_err(|err| {
                                PluginError::new(-32000, format!("SSH RSA hash probe failed: {err}"))
                            })?
                            .flatten();
                        let auth_res = handle
                            .authenticate_publickey(
                                auth.username.clone(),
                                PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                            )
                            .await
                            .map_err(|err| {
                                PluginError::new(-32000, format!("SSH public-key auth failed: {err}"))
                            })?;
                        if auth_res.success() {
                            authenticated = true;
                            log_stderr(format!(
                                "SSH public-key OK for {}@{host}:{port}",
                                auth.username
                            ));
                        } else {
                            last_error = format!(
                                "public-key rejected for {}@{host}:{port}",
                                auth.username
                            );
                        }
                    }
                    Err(err) => last_error = err.message,
                }
            }

            if !authenticated && auth.has_password() {
                let auth_res = handle
                    .authenticate_password(auth.username.clone(), auth.password.clone())
                    .await
                    .map_err(|err| PluginError::new(-32000, format!("SSH auth failed: {err}")))?;
                if auth_res.success() {
                    authenticated = true;
                    log_stderr(format!(
                        "SSH password OK for {}@{host}:{port}",
                        auth.username
                    ));
                } else {
                    last_error = format!(
                        "password rejected for {}@{host}:{port}",
                        auth.username
                    );
                }
            }

            if !authenticated {
                return Err(PluginError::new(
                    -32000,
                    if last_error.is_empty() {
                        format!("SSH authentication failed for {}@{host}:{port}", auth.username)
                    } else {
                        format!("SSH authentication failed: {last_error}")
                    },
                ));
            }

            Ok(SshHandle {
                inner: Arc::new(Mutex::new(handle)),
            })
        })
        .await
        .map_err(|_| {
            PluginError::new(
                -32000,
                format!("SSH connect to {host}:{port} timed out after {timeout_secs}s"),
            )
        })?
    })
}

fn load_private_key(auth: &AuthMaterial) -> Result<PrivateKey, PluginError> {
    let passphrase = auth
        .private_key_passphrase
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            if auth.password.is_empty() {
                None
            } else {
                Some(auth.password.as_str())
            }
        });

    if let Some(pem) = auth
        .private_key_pem
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return decode_secret_key(pem, passphrase).map_err(|err| {
            PluginError::new(-32000, format!("Invalid private key content: {err}"))
        });
    }

    if let Some(path) = auth
        .private_key_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !Path::new(path).is_file() {
            return Err(PluginError::new(
                -32602,
                format!("Private key file not found: {path}"),
            ));
        }
        return load_secret_key(path, passphrase).map_err(|err| {
            PluginError::new(-32000, format!("Failed to load private key {path}: {err}"))
        });
    }

    Err(PluginError::new(-32602, "Missing private key"))
}

pub fn exec_checked(handle: &SshHandle, command: &str) -> Result<String, PluginError> {
    block_on(async {
        let guard = handle.inner.lock().await;
        let mut channel = guard.channel_open_session().await.map_err(|err| {
            PluginError::new(-32000, format!("SSH channel open failed: {err}"))
        })?;
        // Drop the session lock while the channel runs so list/kill can overlap.
        drop(guard);

        channel
            .exec(true, command)
            .await
            .map_err(|err| PluginError::new(-32000, format!("SSH exec failed: {err}")))?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_status = None;
        loop {
            let Some(msg) = channel.wait().await else {
                break;
            };
            match msg {
                ChannelMsg::Data { ref data } => stdout.extend_from_slice(data),
                ChannelMsg::ExtendedData { ref data, .. } => stderr.extend_from_slice(data),
                ChannelMsg::ExitStatus { exit_status: code } => exit_status = Some(code),
                _ => {}
            }
        }

        let out = String::from_utf8_lossy(&stdout).into_owned();
        if let Some(code) = exit_status {
            if code != 0 && out.trim().is_empty() {
                let err = String::from_utf8_lossy(&stderr);
                return Err(PluginError::new(
                    -32000,
                    format!(
                        "Remote command failed (exit {code}): {}",
                        err.trim().chars().take(240).collect::<String>()
                    ),
                ));
            }
        }
        Ok(out)
    })
}
