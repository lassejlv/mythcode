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

        match spawn_host(&paths, event_tx).await {
            Ok(host) => Some(host),
            Err(e) => {
                eprintln!("extension host failed to start: {e}");
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
                if path.extension().map_or(false, |e| e == "ts" || e == "js") {
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

/// Resolve the host script path — bundled next to the binary or from SDK.
fn host_script_path() -> Result<PathBuf> {
    // Check next to the binary first
    let exe = std::env::current_exe().context("cannot find executable path")?;
    let alongside = exe.parent().unwrap_or(Path::new(".")).join("extension-host.ts");
    if alongside.exists() {
        return Ok(alongside);
    }

    // Check in the source tree (dev mode)
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("extension-host")
        .join("host.ts");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    anyhow::bail!("extension host script not found")
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
    tokio::spawn(async move {
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

    // Stdout reader task — handles incoming JSON-RPC from the host
    let pending_clone = pending.clone();
    let commands_clone = commands.clone();
    let event_tx_clone = event_tx.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(msg) = serde_json::from_str::<JsonRpcMessage>(&line) else {
                continue;
            };

            // Response to a request we sent
            if let Some(id) = msg.id {
                if msg.result.is_some() || msg.error.is_some() {
                    let mut pending = pending_clone.lock().await;
                    if let Some(tx) = pending.remove(&id) {
                        let _ = tx.send(msg.result.unwrap_or(Value::Null));
                    }
                    continue;
                }
            }

            // Incoming notification/request from host
            let method = msg.method.as_deref().unwrap_or("");
            match method {
                "register/command" => {
                    if let Some(params) = &msg.params {
                        let name = params["name"].as_str().unwrap_or("").to_string();
                        let desc = params["description"].as_str().unwrap_or("").to_string();
                        let hint = params["hint"].as_str().map(|s| s.to_string());
                        commands_clone.lock().await.push(SlashCommand {
                            name,
                            description: desc,
                            hint,
                            source: SlashCommandSource::Extension,
                        });
                    }
                    // Ack the registration
                    if let Some(id) = msg.id {
                        let ack = serde_json::json!({"jsonrpc":"2.0","id":id,"result":true});
                        // We don't have stdin_tx here, so we skip the ack for now
                        let _ = ack; // TODO: send ack back
                    }
                }
                "action/showMessage" => {
                    if let Some(params) = &msg.params {
                        let text = params["text"].as_str().unwrap_or("").to_string();
                        let level = params["level"]
                            .as_str()
                            .unwrap_or("info")
                            .to_string();
                        let _ = event_tx_clone.send(AppEvent::ExtensionMessage { text, level });
                    }
                }
                "action/setActivity" => {
                    if let Some(params) = &msg.params {
                        let text = params["text"].as_str().unwrap_or("").to_string();
                        let _ = event_tx_clone.send(AppEvent::Activity(text));
                    }
                }
                "host/ready" => {
                    // Extensions loaded successfully
                }
                "host/error" => {
                    if let Some(params) = &msg.params {
                        let ext = params["extension"].as_str().unwrap_or("unknown");
                        let err = params["error"].as_str().unwrap_or("unknown error");
                        let _ = event_tx_clone.send(AppEvent::Warning(format!(
                            "extension {ext}: {err}"
                        )));
                    }
                }
                _ => {}
            }
        }
    });

    // Stderr reader — forward as warnings
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = event_tx.send(AppEvent::Warning(format!("ext: {line}")));
        }
    });

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
