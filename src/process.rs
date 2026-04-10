use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, mpsc};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::types::{AcpProvider, AppConfig, AppEvent};

pub type ProcessInput = Compat<ChildStdin>;
pub type ProcessOutput = Compat<ChildStdout>;

pub struct ProcessTransport {
    pub stdin: ProcessInput,
    pub stdout: ProcessOutput,
}

pub struct AcpProcess {
    child: Arc<Mutex<Child>>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    provider: AcpProvider,
}

impl AcpProcess {
    pub async fn spawn(
        config: &AppConfig,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<(Self, ProcessTransport)> {
        let (program, args, label) = match &config.provider {
            AcpProvider::OpenCode => (
                "opencode".to_string(),
                vec![
                    "acp".to_string(),
                    "--cwd".to_string(),
                    config.cwd.display().to_string(),
                ],
                "opencode acp",
            ),
            AcpProvider::Codex => (
                "npx".to_string(),
                vec!["-y".to_string(), "@zed-industries/codex-acp".to_string()],
                "codex-acp",
            ),
            AcpProvider::Claude => (
                "npx".to_string(),
                vec![
                    "-y".to_string(),
                    "@agentclientprotocol/claude-agent-acp".to_string(),
                ],
                "claude-acp",
            ),
            AcpProvider::Pi => (
                "npx".to_string(),
                vec!["-y".to_string(), "pi-acp".to_string()],
                "pi-acp",
            ),
            AcpProvider::Gemini => (
                "gemini".to_string(),
                vec!["--acp".to_string()],
                "gemini-acp",
            ),
            AcpProvider::Cursor => (
                "agent".to_string(),
                vec!["acp".to_string()],
                "cursor-acp",
            ),
            AcpProvider::Amp => (
                "npx".to_string(),
                vec!["-y".to_string(), "amp-acp".to_string()],
                "amp-acp",
            ),
            AcpProvider::Copilot => (
                "copilot".to_string(),
                vec!["--acp".to_string(), "--stdio".to_string()],
                "copilot-acp",
            ),
        };

        let mut command = Command::new(&program);
        for arg in &args {
            command.arg(arg);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(&config.cwd)
            .kill_on_drop(true);

        let mut child = command.spawn().map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => anyhow!(
                "`{program}` was not found in PATH.\nMake sure `{program}` is installed and available in your shell."
            ),
            _ => anyhow!(error).context(format!("failed to start `{label}`")),
        })?;

        let stdin = child
            .stdin
            .take()
            .context(format!("failed to capture stdin for `{label}`"))?
            .compat_write();
        let stdout = child
            .stdout
            .take()
            .context(format!("failed to capture stdout for `{label}`"))?
            .compat();

        let stderr = child
            .stderr
            .take()
            .context(format!("failed to capture stderr for `{label}`"))?;
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(32)));
        let stderr_tail_task = Arc::clone(&stderr_tail);
        let label_owned = label.to_string();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        {
                            let mut tail = stderr_tail_task.lock().await;
                            if tail.len() >= 32 {
                                tail.pop_front();
                            }
                            tail.push_back(line.clone());
                        }
                        let _ = event_tx.send(AppEvent::ProcessStderr(line));
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = event_tx.send(AppEvent::Warning(format!(
                            "failed to read {label_owned} stderr: {error}"
                        )));
                        break;
                    }
                }
            }
        });

        Ok((
            Self {
                child: Arc::new(Mutex::new(child)),
                stderr_tail,
                provider: config.provider.clone(),
            },
            ProcessTransport { stdin, stdout },
        ))
    }

    pub async fn shutdown(&self) {
        let mut child = self.child.lock().await;
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }

        let _ = child.start_kill();
        let _ = child.wait().await;
    }

    pub async fn try_wait(&self) -> Result<Option<std::process::ExitStatus>> {
        let mut child = self.child.lock().await;
        child.try_wait().context("failed to poll ACP process")
    }

    pub async fn startup_context(&self, cwd: &Path) -> String {
        let label = match self.provider {
            AcpProvider::OpenCode => "opencode acp",
            AcpProvider::Codex => "codex-acp",
            AcpProvider::Claude => "claude-acp",
            AcpProvider::Pi => "pi-acp",
            AcpProvider::Gemini => "gemini-acp",
            AcpProvider::Cursor => "cursor-acp",
            AcpProvider::Amp => "amp-acp",
            AcpProvider::Copilot => "copilot-acp",
        };
        let tail = self.stderr_tail.lock().await;
        if tail.is_empty() {
            format!(
                "`{label}` exited unexpectedly while starting for {}.",
                cwd.display()
            )
        } else {
            format!(
                "`{label}` exited unexpectedly while starting for {}.\nstderr:\n{}",
                cwd.display(),
                tail.iter().cloned().collect::<Vec<_>>().join("\n")
            )
        }
    }
}
