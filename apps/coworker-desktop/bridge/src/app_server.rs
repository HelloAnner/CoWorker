use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, mpsc, oneshot},
    task::JoinHandle,
    time::{Duration, timeout},
};
use tracing::{debug, info, warn};

use crate::{
    command_resolver::{ResolvedCommand, resolve_command},
    error::{BridgeError, Result},
    logging::log_subprocess_line,
};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
const APP_SERVER_STOP_GRACE: Duration = Duration::from_secs(3);

pub struct AppServerRequest {
    pub id: Value,
    pub method: String,
    pub params: serde_json::Map<String, Value>,
    pub response: oneshot::Sender<std::result::Result<Value, String>>,
}

type PendingSender = oneshot::Sender<std::result::Result<Value, String>>;

#[derive(Clone)]
pub struct CodexAppServerClient {
    inner: Arc<AppServerInner>,
}

struct AppServerInner {
    command: String,
    args: Vec<String>,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    stderr_task: Mutex<Option<JoinHandle<()>>>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, PendingSender>>,
    notification_tx: mpsc::Sender<Value>,
    server_request_tx: mpsc::Sender<AppServerRequest>,
}

impl CodexAppServerClient {
    pub fn new(
        command: String,
        args: Vec<String>,
    ) -> (
        Self,
        mpsc::Receiver<Value>,
        mpsc::Receiver<AppServerRequest>,
    ) {
        let (notification_tx, notification_rx) = mpsc::channel(256);
        let (server_request_tx, server_request_rx) = mpsc::channel(64);
        (
            Self {
                inner: Arc::new(AppServerInner {
                    command,
                    args,
                    child: Mutex::new(None),
                    stdin: Mutex::new(None),
                    stderr_task: Mutex::new(None),
                    next_id: AtomicU64::new(1),
                    pending: Mutex::new(HashMap::new()),
                    notification_tx,
                    server_request_tx,
                }),
            },
            notification_rx,
            server_request_rx,
        )
    }

    pub async fn start(&self) -> Result<()> {
        if self.inner.child.lock().await.is_some() {
            debug!("Codex app-server already started");
            return Ok(());
        }

        info!(
            command = %self.inner.command,
            args = ?self.inner.args,
            "Starting Codex app-server"
        );
        let resolved = resolve_command(&self.inner.command).map_err(|error| {
            BridgeError::startup(format!(
                "Unable to start Codex app-server: configured command {:?} could not be resolved: {error}",
                self.inner.command
            ))
        })?;
        info!(
            executable = %resolved.executable().display(),
            display_path = %resolved.display_path().display(),
            "Resolved Codex app-server command"
        );
        let mut command = resolved_command(&resolved);
        command
            .args(&self.inner.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        suppress_console_window(&mut command);

        let mut child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                BridgeError::startup(format!(
                    "Unable to start Codex app-server: resolved command {:?} for configured command {:?} was not found",
                    resolved.executable(),
                    self.inner.command
                ))
            } else if error.kind() == std::io::ErrorKind::PermissionDenied {
                BridgeError::startup(format!(
                    "Unable to start Codex app-server: permission denied running resolved command {:?} for configured command {:?}",
                    resolved.executable(),
                    self.inner.command
                ))
            } else {
                BridgeError::Io(error)
            }
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BridgeError::startup("Codex app-server stdout was not piped"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| BridgeError::startup("Codex app-server stdin was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| BridgeError::startup("Codex app-server stderr was not piped"))?;

        *self.inner.stdin.lock().await = Some(stdin);
        *self.inner.child.lock().await = Some(child);
        *self.inner.stderr_task.lock().await = Some(tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log_subprocess_line("codex_app_server", &line);
            }
        }));

        let client = self.clone();
        tokio::spawn(async move {
            if let Err(error) = client.read_loop(stdout).await {
                warn!(%error, "Codex app-server read loop ended");
            }
        });

        self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "coworker_desktop",
                    "title": "CoWorker Desktop",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {"experimentalApi": true},
            }),
        )
        .await?;
        self.notify("initialized", json!({})).await?;
        info!("Codex app-server initialized");
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        info!("Stopping Codex app-server");
        *self.inner.stdin.lock().await = None;
        self.fail_pending_requests("Codex app-server is stopping".into())
            .await;

        let child = self.inner.child.lock().await.take();
        if let Some(mut child) = child
            && child.id().is_some()
        {
            match timeout(APP_SERVER_STOP_GRACE, child.wait()).await {
                Ok(Ok(status)) => {
                    info!(%status, "Codex app-server exited after stdin closed");
                }
                Ok(Err(error)) => {
                    warn!(%error, "Failed while waiting for Codex app-server to exit");
                    if let Err(kill_error) = child.kill().await {
                        warn!(%kill_error, "Failed to kill Codex app-server after wait error");
                    }
                }
                Err(_) => {
                    warn!(
                        timeout_seconds = APP_SERVER_STOP_GRACE.as_secs(),
                        "Codex app-server did not exit after stdin closed; killing"
                    );
                    if let Err(error) = child.kill().await {
                        warn!(%error, "Failed to kill Codex app-server");
                    }
                }
            }
        }
        if let Some(task) = self.inner.stderr_task.lock().await.take() {
            task.abort();
        }
        info!("Codex app-server stopped");
        Ok(())
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.inner.pending.lock().await.insert(id, tx);
        if let Err(error) = self
            .write(json!({"id": id, "method": method, "params": params}))
            .await
        {
            self.inner.pending.lock().await.remove(&id);
            return Err(error);
        }
        match rx.await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(BridgeError::AppServer(error)),
            Err(_) => Err(BridgeError::AppServer(
                "app-server response channel closed".into(),
            )),
        }
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.write(json!({"method": method, "params": params}))
            .await
    }

    async fn write(&self, message: Value) -> Result<()> {
        let mut guard = self.inner.stdin.lock().await;
        let stdin = guard
            .as_mut()
            .ok_or_else(|| BridgeError::AppServer("Codex app-server is not started".into()))?;
        let mut line = serde_json::to_vec(&message)?;
        line.push(b'\n');
        stdin.write_all(&line).await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn read_loop(&self, stdout: tokio::process::ChildStdout) -> Result<()> {
        let result = self.read_loop_inner(stdout).await;
        let message = match &result {
            Ok(()) => "Codex app-server read loop ended".to_owned(),
            Err(error) => format!("Codex app-server read loop ended: {error}"),
        };
        self.fail_pending_requests(message).await;
        result
    }

    async fn read_loop_inner(&self, stdout: tokio::process::ChildStdout) -> Result<()> {
        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            let msg: Value = serde_json::from_str(&line)?;
            if !self.handle_incoming_message(msg).await? {
                break;
            }
        }
        Ok(())
    }

    async fn handle_incoming_message(&self, msg: Value) -> Result<bool> {
        if msg.get("id").is_some() && msg.get("method").is_some() {
            let client = self.clone();
            tokio::spawn(async move {
                if let Err(error) = client.handle_server_request(msg).await {
                    warn!(%error, "Failed to handle Codex app-server request");
                }
            });
        } else if let Some(id) = msg.get("id").and_then(Value::as_u64) {
            let sender = self.inner.pending.lock().await.remove(&id);
            if let Some(sender) = sender {
                if let Some(error) = msg.get("error") {
                    let _ = sender.send(Err(error.to_string()));
                } else {
                    let _ =
                        sender.send(Ok(msg.get("result").cloned().unwrap_or_else(|| json!({}))));
                }
            }
        } else if self.inner.notification_tx.send(msg).await.is_err() {
            return Ok(false);
        }
        Ok(true)
    }

    async fn fail_pending_requests(&self, message: String) {
        let pending = self
            .inner
            .pending
            .lock()
            .await
            .drain()
            .map(|(_, sender)| sender)
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return;
        }
        warn!(
            count = pending.len(),
            "Failing pending Codex app-server requests"
        );
        for sender in pending {
            let _ = sender.send(Err(message.clone()));
        }
    }

    async fn handle_server_request(&self, msg: Value) -> Result<()> {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| BridgeError::AppServer("invalid server request method".into()))?
            .to_owned();
        let params = msg
            .get("params")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        debug!(%method, request_id = %id, "Received app-server request");
        let (response_tx, response_rx) = oneshot::channel();
        let request = AppServerRequest {
            id: id.clone(),
            method: method.clone(),
            params,
            response: response_tx,
        };
        if self.inner.server_request_tx.send(request).await.is_err() {
            self.write(json!({
                "id": id,
                "error": {"code": -32603, "message": "No server request handler is registered"},
            }))
            .await?;
            return Ok(());
        }
        match response_rx.await {
            Ok(Ok(result)) => {
                self.write(json!({"id": id, "result": result})).await?;
                debug!(%method, request_id = %id, "Handled app-server request");
            }
            Ok(Err(message)) => {
                self.write(json!({"id": id, "error": {"code": -32603, "message": message}}))
                    .await?;
            }
            Err(_) => {
                self.write(json!({"id": id, "error": {"code": -32603, "message": "server request handler dropped"}}))
                    .await?;
            }
        }
        Ok(())
    }
}

fn resolved_command(resolved: &ResolvedCommand) -> Command {
    Command::new(resolved.executable())
}

fn suppress_console_window(command: &mut Command) {
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::time::{Duration, timeout};

    use super::CodexAppServerClient;

    #[tokio::test]
    async fn server_request_does_not_block_later_notifications() {
        let (client, mut notifications, mut server_requests) =
            CodexAppServerClient::new("codex".into(), vec!["app-server".into()]);

        client
            .handle_incoming_message(json!({
                "id": "srv_1",
                "method": "item/tool/requestUserInput",
                "params": {"threadId": "thr_1"}
            }))
            .await
            .expect("server request should be accepted");

        let request = timeout(Duration::from_secs(1), server_requests.recv())
            .await
            .expect("server request should be forwarded")
            .expect("server request channel should stay open");
        assert_eq!(request.method, "item/tool/requestUserInput");

        client
            .handle_incoming_message(json!({
                "method": "turn/started",
                "params": {"threadId": "thr_1"}
            }))
            .await
            .expect("notification should be accepted");

        let notification = timeout(Duration::from_secs(1), notifications.recv())
            .await
            .expect("notification should not wait on server request")
            .expect("notification channel should stay open");
        assert_eq!(notification["method"], "turn/started");
    }

    #[tokio::test]
    async fn read_loop_shutdown_failure_releases_pending_requests() {
        let (client, _notifications, _server_requests) =
            CodexAppServerClient::new("codex".into(), vec!["app-server".into()]);
        let (tx, rx) = tokio::sync::oneshot::channel();
        client.inner.pending.lock().await.insert(7, tx);

        client
            .fail_pending_requests("Codex app-server read loop ended".into())
            .await;

        let result = rx.await.expect("pending sender should send an error");
        assert_eq!(
            result.expect_err("pending request should fail"),
            "Codex app-server read loop ended"
        );
        assert!(client.inner.pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn stop_releases_pending_requests() {
        let (client, _notifications, _server_requests) =
            CodexAppServerClient::new("codex".into(), vec!["app-server".into()]);
        let (tx, rx) = tokio::sync::oneshot::channel();
        client.inner.pending.lock().await.insert(7, tx);

        client.stop().await.expect("stop should succeed");

        let result = rx.await.expect("pending sender should send an error");
        assert_eq!(
            result.expect_err("pending request should fail"),
            "Codex app-server is stopping"
        );
        assert!(client.inner.pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn failed_request_write_removes_pending_request() {
        let (client, _notifications, _server_requests) =
            CodexAppServerClient::new("codex".into(), vec!["app-server".into()]);

        let error = client
            .request("thread/list", json!({}))
            .await
            .expect_err("request should fail when app-server is not started");

        assert!(error.to_string().contains("not started"));
        assert!(client.inner.pending.lock().await.is_empty());
    }
}
