use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, mpsc};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::types::{AppConfig, AppEvent};

pub type ProcessInput = Compat<ChildStdin>;
pub type ProcessOutput = Compat<ChildStdout>;

pub struct ProcessTransport {
    pub stdin: ProcessInput,
    pub stdout: ProcessOutput,
}

pub struct OpenCodeProcess {
    child: Arc<Mutex<Child>>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
}

impl OpenCodeProcess {
    pub async fn spawn(
        config: &AppConfig,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<(Self, ProcessTransport)> {
        let mut command = Command::new("opencode");
        command
            .arg("acp")
            .arg("--cwd")
            .arg(&config.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(&config.cwd)
            .kill_on_drop(true);

        let mut child = command.spawn().map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => anyhow!(
                "`opencode` was not found in PATH.\nInstall OpenCode first, then make sure `opencode` is available in your shell."
            ),
            _ => anyhow!(error).context("failed to start `opencode acp`"),
        })?;

        let stdin = child
            .stdin
            .take()
            .context("failed to capture stdin for `opencode acp`")?
            .compat_write();
        let stdout = child
            .stdout
            .take()
            .context("failed to capture stdout for `opencode acp`")?
            .compat();

        let stderr = child
            .stderr
            .take()
            .context("failed to capture stderr for `opencode acp`")?;
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(32)));
        let stderr_tail_task = Arc::clone(&stderr_tail);
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
                            "failed to read opencode stderr: {error}"
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
        child.try_wait().context("failed to poll opencode process")
    }

    pub async fn startup_context(&self, cwd: &Path) -> String {
        let tail = self.stderr_tail.lock().await;
        if tail.is_empty() {
            format!(
                "`opencode acp` exited unexpectedly while starting for {}.",
                cwd.display()
            )
        } else {
            format!(
                "`opencode acp` exited unexpectedly while starting for {}.\nstderr:\n{}",
                cwd.display(),
                tail.iter().cloned().collect::<Vec<_>>().join("\n")
            )
        }
    }
}
