use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

use crate::types::{AppEvent, SlashCommand, SlashCommandSource};

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcMessage {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

pub struct ExtensionHost {
    child: Child,
    stdin_tx: mpsc::UnboundedSender<String>,
    pending: std::sync::Arc<tokio::sync::Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next_id: AtomicU64,
    commands: std::sync::Arc<tokio::sync::Mutex<Vec<SlashCommand>>>,
}

impl ExtensionHost {
    /// Discover extensions and spawn the host process. Returns None if no extensions found.
    pub async fn discover_and_spawn(
        cwd: &Path,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Option<Self> {
        let paths = discover_extensions(cwd);
        if paths.is_empty() {
            return None;
        }

        let count = paths.len();
        match spawn_host(&paths, event_tx.clone()).await {
            Ok(host) => {
                let names: Vec<_> = paths
                    .iter()
                    .filter_map(|p| p.file_name())
                    .filter_map(|n| n.to_str())
                    .collect();
                let _ = event_tx.send(AppEvent::ExtensionMessage {
                    text: format!("{count} extension(s) loaded: {}", names.join(", ")),
                    level: "info".into(),
                });
                Some(host)
            }
            Err(e) => {
                let _ = event_tx.send(AppEvent::Warning(format!("extension host failed: {e}")));
                None
            }
        }
    }

    /// Send a fire-and-forget notification to the host.
    pub fn notify(&self, method: &str, params: Value) {
        let msg = JsonRpcMessage {
            jsonrpc: "2.0".into(),
            id: None,
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = self.stdin_tx.send(json);
        }
    }

    /// Send a request and wait for a response (with timeout).
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();

        self.pending.lock().await.insert(id, tx);

        let msg = JsonRpcMessage {
            jsonrpc: "2.0".into(),
            id: Some(id),
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        };
        let json = serde_json::to_string(&msg)?;
        let _ = self.stdin_tx.send(json);

        match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => anyhow::bail!("extension host channel closed"),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                anyhow::bail!("extension host request timed out")
            }
        }
    }

    /// Get registered extension commands.
    pub async fn commands(&self) -> Vec<SlashCommand> {
        self.commands.lock().await.clone()
    }

    /// Execute an extension command by name.
    pub async fn execute_command(&self, name: &str, args: &str) -> Result<()> {
        self.request(
            "command/execute",
            serde_json::json!({ "name": name, "args": args }),
        )
        .await?;
        Ok(())
    }

    /// Shut down the extension host.
    pub async fn shutdown(&mut self) {
        self.notify("lifecycle/shutdown", serde_json::json!({}));
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let _ = self.child.kill().await;
    }
}

fn discover_extensions(cwd: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    let dirs = [
        dirs_home().map(|h| h.join(".mythcode").join("extensions")),
        Some(cwd.join(".mythcode").join("extensions")),
    ];

    for dir in dirs.into_iter().flatten() {
        if !dir.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "ts" || e == "js") {
                    paths.push(path);
                } else if path.is_dir() {
                    let index = path.join("index.ts");
                    if index.exists() {
                        paths.push(index);
                    }
                }
            }
        }
    }

    paths
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The host script is embedded at compile time so it works in production builds.
const HOST_SCRIPT: &str = include_str!("../extension-host/host.ts");

/// Write the embedded host script to a temp file and return the path.
fn host_script_path() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("mythcode");
    std::fs::create_dir_all(&dir).context("failed to create temp dir for extension host")?;
    let path = dir.join("host.ts");
    std::fs::write(&path, HOST_SCRIPT).context("failed to write extension host script")?;
    Ok(path)
}

async fn spawn_host(
    extension_paths: &[PathBuf],
    event_tx: mpsc::UnboundedSender<AppEvent>,
) -> Result<ExtensionHost> {
    let host_script = host_script_path()?;

    let mut args = vec![host_script.to_string_lossy().to_string()];
    for p in extension_paths {
        args.push(p.to_string_lossy().to_string());
    }

    // Try bun first, fall back to npx tsx
    let (program, full_args) = if which_exists("bun") {
        ("bun".to_string(), args)
    } else {
        let mut a = vec!["tsx".to_string()];
        a.extend(args);
        ("npx".to_string(), a)
    };

    let mut child = Command::new(&program)
        .args(&full_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to start extension host via `{program}`"))?;

    let stdout = child.stdout.take().context("no stdout")?;
    let stderr = child.stderr.take().context("no stderr")?;
    let stdin = child.stdin.take().context("no stdin")?;

    let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<String>();

    // Stdin writer task
    tokio::task::spawn_local(async move {
        let mut stdin = stdin;
        while let Some(line) = stdin_rx.recv().await {
            if stdin.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            if stdin.write_all(b"\n").await.is_err() {
                break;
            }
            if stdin.flush().await.is_err() {
                break;
            }
        }
    });

    let pending: std::sync::Arc<tokio::sync::Mutex<HashMap<u64, oneshot::Sender<Value>>>> =
        std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let commands: std::sync::Arc<tokio::sync::Mutex<Vec<SlashCommand>>> =
        std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));

    // Channel to signal when host is ready
    let (ready_tx, ready_rx) = oneshot::channel::<()>();
    let ready_tx = std::sync::Arc::new(tokio::sync::Mutex::new(Some(ready_tx)));

    // Stdout reader task — handles incoming JSON-RPC from the host
    let pending_clone = pending.clone();
    let commands_clone = commands.clone();
    let event_tx_clone = event_tx.clone();
    let ready_tx_clone = ready_tx.clone();
    let reply_tx = stdin_tx.clone();
    tokio::task::spawn_local(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(msg) = serde_json::from_str::<JsonRpcMessage>(&line) else {
                continue;
            };

            // Response to a request we sent
            if let Some(id) = msg.id
                && (msg.result.is_some() || msg.error.is_some())
            {
                let mut pending = pending_clone.lock().await;
                if let Some(tx) = pending.remove(&id) {
                    let _ = tx.send(msg.result.unwrap_or(Value::Null));
                }
                continue;
            }

            let reply_id = msg.id;
            let reply = |result: Value| {
                if let Some(id) = reply_id {
                    let resp = serde_json::json!({"jsonrpc":"2.0","id":id,"result":result});
                    let _ = reply_tx.send(resp.to_string());
                }
            };

            // Incoming notification/request from host
            let method = msg.method.as_deref().unwrap_or("");
            let params = msg.params.as_ref();
            match method {
                "register/command" => {
                    if let Some(p) = params {
                        let name = p["name"].as_str().unwrap_or("").to_string();
                        let desc = p["description"].as_str().unwrap_or("").to_string();
                        let hint = p["hint"].as_str().map(|s| s.to_string());
                        commands_clone.lock().await.push(SlashCommand {
                            name,
                            description: desc,
                            hint,
                            source: SlashCommandSource::Extension,
                        });
                    }
                    reply(Value::Bool(true));
                }
                "register/tool" => {
                    reply(Value::Bool(true));
                }
                "action/showMessage" => {
                    if let Some(p) = params {
                        let text = p["text"].as_str().unwrap_or("").to_string();
                        let level = p["level"].as_str().unwrap_or("info").to_string();
                        let _ = event_tx_clone.send(AppEvent::ExtensionMessage { text, level });
                    }
                }
                "action/setActivity" => {
                    if let Some(p) = params {
                        let text = p["text"].as_str().unwrap_or("").to_string();
                        let _ =
                            event_tx_clone.send(AppEvent::Activity(crate::types::ActivityView {
                                title: text,
                                status: None,
                            }));
                    }
                }
                "action/exit" => {
                    let _ = event_tx_clone.send(AppEvent::ExtensionExit);
                }
                "action/newSession" => {
                    let _ = event_tx_clone.send(AppEvent::ExtensionNewSession);
                    reply(Value::Bool(true));
                }
                "action/getCwd" | "action/getModel" => {
                    // These need state from the TUI — respond via events
                    // For now return empty; will be wired up when TUI handles them
                    reply(Value::Null);
                }
                "action/setModel" => {
                    if let Some(p) = params {
                        let model_id = p["modelId"].as_str().unwrap_or("").to_string();
                        let _ = event_tx_clone.send(AppEvent::ExtensionSetModel(model_id));
                    }
                    reply(Value::Bool(true));
                }
                "action/sendMessage" => {
                    if let Some(p) = params {
                        let text = p["text"].as_str().unwrap_or("").to_string();
                        let _ = event_tx_clone.send(AppEvent::ExtensionSendMessage(text));
                    }
                }
                "action/sendUserMessage" => {
                    if let Some(p) = params {
                        let text = p["text"].as_str().unwrap_or("").to_string();
                        let _ = event_tx_clone.send(AppEvent::ExtensionSendMessage(text));
                    }
                }
                "action/exec" => {
                    if let Some(p) = params {
                        let command = p["command"].as_str().unwrap_or("").to_string();
                        let reply_tx_exec = reply_tx.clone();
                        let id = reply_id;
                        tokio::task::spawn_local(async move {
                            let output = tokio::process::Command::new("sh")
                                .arg("-c")
                                .arg(&command)
                                .output()
                                .await;
                            let result = match output {
                                Ok(out) => serde_json::json!({
                                    "stdout": String::from_utf8_lossy(&out.stdout),
                                    "stderr": String::from_utf8_lossy(&out.stderr),
                                    "exitCode": out.status.code().unwrap_or(-1),
                                }),
                                Err(e) => serde_json::json!({
                                    "stdout": "",
                                    "stderr": e.to_string(),
                                    "exitCode": -1,
                                }),
                            };
                            if let Some(id) = id {
                                let resp =
                                    serde_json::json!({"jsonrpc":"2.0","id":id,"result":result});
                                let _ = reply_tx_exec.send(resp.to_string());
                            }
                        });
                    }
                }
                "action/clearScreen" => {
                    let _ = event_tx_clone.send(AppEvent::ExtensionClearScreen);
                }
                "action/setStatus" => {
                    if let Some(p) = params {
                        let key = p["key"].as_str().unwrap_or("").to_string();
                        let value = p["value"].as_str().map(|s| s.to_string());
                        let _ = event_tx_clone.send(AppEvent::ExtensionSetStatus { key, value });
                    }
                }
                "action/setTheme" => {
                    if let Some(p) = params
                        && let Ok(overrides) =
                            serde_json::from_value::<crate::tui::theme::ThemeOverride>(p.clone())
                    {
                        crate::tui::theme::apply_override(&overrides);
                    }
                    reply(Value::Bool(true));
                }
                "host/ready" => {
                    if let Some(tx) = ready_tx_clone.lock().await.take() {
                        let _ = tx.send(());
                    }
                }
                "host/error" => {
                    if let Some(p) = params {
                        let ext = p["extension"].as_str().unwrap_or("unknown");
                        let err = p["error"].as_str().unwrap_or("unknown error");
                        let _ = event_tx_clone
                            .send(AppEvent::Warning(format!("extension {ext}: {err}")));
                    }
                }
                _ => {}
            }
        }
    });

    // Stderr reader — forward as warnings
    tokio::task::spawn_local(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = event_tx.send(AppEvent::Warning(format!("ext: {line}")));
        }
    });

    // Wait for host to be ready (with timeout)
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), ready_rx).await;

    Ok(ExtensionHost {
        child,
        stdin_tx,
        pending,
        next_id: AtomicU64::new(1),
        commands,
    })
}

fn which_exists(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
