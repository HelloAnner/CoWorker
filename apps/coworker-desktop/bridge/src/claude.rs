use std::{
    collections::{HashMap, HashSet},
    io::BufRead,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout},
    sync::{Mutex, Notify},
};

use crate::{
    actor::{
        ActorAdapter, ActorConversation, ActorConversationPage, ActorHealth, ActorMessage,
        ActorMessageInput, ActorMessagePage, ActorStreamEvent, publish_actor_stream_event,
    },
    command_resolver::{ResolvedCommand, resolve_command},
    conversation_store::default_conversation_title,
    desktop_protocol::{ActorId, actor_model_message},
    error::{BridgeError, Result},
    logging::log_subprocess_line,
};

const DESKTOP_ALLOWED_MCP_TOOLS: &str =
    "mcp__coworker_desktop__list_coworkers,mcp__coworker_desktop__send_to_coworker";
const CLAUDE_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const CLAUDE_INPUT_ACK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeConfig {
    pub enabled: bool,
    pub command: String,
    pub args: Vec<String>,
    pub permissions_mode: String,
    pub home_dir: PathBuf,
    pub storage_dir: PathBuf,
    pub desktop_config_path: Option<PathBuf>,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            command: "claude".to_owned(),
            args: Vec::new(),
            permissions_mode: "read-only".to_owned(),
            home_dir: default_claude_home(),
            storage_dir: PathBuf::from("data/coworker_desktop"),
            desktop_config_path: None,
        }
    }
}

#[derive(Clone)]
pub struct ClaudeAdapter {
    config: ClaudeConfig,
    active_sessions: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
    sessions: Arc<Mutex<HashMap<String, Arc<ClaudeProcessHandle>>>>,
}

struct ClaudeProcessHandle {
    state: Mutex<ClaudeProcess>,
    stdin: Mutex<ChildStdin>,
    turns: Arc<ClaudeTurnQueue>,
    acknowledged_inputs: Mutex<HashSet<String>>,
}

struct ClaudeProcess {
    child: Child,
    stdout: Lines<BufReader<ChildStdout>>,
    session_id: Option<String>,
    project_path: Option<String>,
    mode: Option<String>,
    idle_generation: u64,
    idle_expired: bool,
    _cleanup: ClaudeRunCleanup,
    stderr_task: tokio::task::JoinHandle<()>,
}

struct ClaudeTurnQueue {
    next: AtomicU64,
    serving: AtomicU64,
    changed: Notify,
}

impl ClaudeTurnQueue {
    fn new() -> Self {
        Self {
            next: AtomicU64::new(0),
            serving: AtomicU64::new(0),
            changed: Notify::new(),
        }
    }

    fn issue(&self) -> u64 {
        self.next.fetch_add(1, Ordering::AcqRel)
    }

    async fn wait(self: &Arc<Self>, ticket: u64) -> ClaudeTurnGuard {
        loop {
            let changed = self.changed.notified();
            if self.serving.load(Ordering::Acquire) == ticket {
                return ClaudeTurnGuard {
                    queue: Arc::clone(self),
                };
            }
            changed.await;
        }
    }

    fn has_pending(&self) -> bool {
        self.next.load(Ordering::Acquire) > self.serving.load(Ordering::Acquire)
    }
}

struct ClaudeTurnGuard {
    queue: Arc<ClaudeTurnQueue>,
}

impl Drop for ClaudeTurnGuard {
    fn drop(&mut self) {
        self.queue.serving.fetch_add(1, Ordering::AcqRel);
        self.queue.changed.notify_waiters();
    }
}

impl Drop for ClaudeProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        self.stderr_task.abort();
    }
}

struct ClaudeRunCleanup {
    database_path: PathBuf,
    run_id: String,
    mcp_config_path: Option<PathBuf>,
    broker: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for ClaudeRunCleanup {
    fn drop(&mut self) {
        if let Ok(store) = crate::conversation_store::ConversationStore::open(&self.database_path) {
            let _ = store.remove_actor_run(&self.run_id);
        }
        if let Some(path) = self.mcp_config_path.as_ref() {
            let _ = std::fs::remove_file(path);
        }
        if let Some(broker) = self.broker.take() {
            broker.abort();
        }
    }
}

impl ClaudeAdapter {
    pub fn new(config: ClaudeConfig) -> Self {
        Self {
            config,
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn resolved_command(&self) -> Result<ResolvedCommand> {
        resolve_command(&self.config.command).map_err(|error| {
            BridgeError::startup(format!(
                "Unable to start Claude Code: configured command {:?} could not be resolved: {error}",
                self.config.command
            ))
        })
    }

    async fn run_turn(
        &self,
        conversation_id: Option<&str>,
        message_id: Option<&str>,
        content: &str,
        project_path: Option<&str>,
        mode: Option<&str>,
        coworker_id: Option<&str>,
        session_name: Option<&str>,
    ) -> Result<Value> {
        let session_key = conversation_id
            .map(str::to_owned)
            .unwrap_or_else(|| format!("<new>:{}", uuid::Uuid::new_v4()));
        let input_id = message_id
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let cancellation = Arc::new(Notify::new());

        let result = self
            .run_turn_inner(
                conversation_id,
                &session_key,
                &input_id,
                content,
                project_path,
                mode,
                coworker_id,
                session_name,
                Arc::clone(&cancellation),
            )
            .await;
        self.active_sessions
            .lock()
            .await
            .retain(|_, value| !Arc::ptr_eq(value, &cancellation));
        result
    }

    async fn run_turn_inner(
        &self,
        conversation_id: Option<&str>,
        session_key: &str,
        input_id: &str,
        content: &str,
        project_path: Option<&str>,
        mode: Option<&str>,
        coworker_id: Option<&str>,
        session_name: Option<&str>,
        cancellation: Arc<Notify>,
    ) -> Result<Value> {
        if content.trim().is_empty() {
            return Err(BridgeError::message("Claude message content is required"));
        }
        let process = self
            .process_for_turn(
                conversation_id,
                project_path,
                mode,
                coworker_id,
                session_name,
            )
            .await?;
        let result = self
            .send_process_message(&process, session_key, input_id, content, cancellation)
            .await;
        if result.is_ok() {
            self.schedule_idle_shutdown(&process, CLAUDE_IDLE_TIMEOUT)
                .await;
        } else {
            self.evict_process(&process, conversation_id).await;
        }
        result
    }

    async fn process_for_turn(
        &self,
        conversation_id: Option<&str>,
        project_path: Option<&str>,
        mode: Option<&str>,
        coworker_id: Option<&str>,
        session_name: Option<&str>,
    ) -> Result<Arc<ClaudeProcessHandle>> {
        let project_path = project_path
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned);
        let mode = effective_claude_permission_mode(mode, &self.config.permissions_mode)
            .map(str::to_owned);
        if let Some(session_id) = conversation_id
            && let Some(process) = self.sessions.lock().await.get(session_id).cloned()
        {
            // Streaming input accepts another user message while Claude is
            // active. Return the live process without waiting for its stdout
            // reader; send_process_message writes to the independent stdin.
            if self.active_sessions.lock().await.contains_key(session_id) {
                return Ok(process);
            }
            let reusable = {
                let mut process = process.state.lock().await;
                let reusable = !process.idle_expired
                    && process.child.try_wait()?.is_none()
                    && process.project_path == project_path
                    && process.mode == mode;
                if reusable {
                    process.idle_generation = process.idle_generation.wrapping_add(1);
                }
                reusable
            };
            if reusable {
                return Ok(process);
            }
            self.evict_process(&process, Some(session_id)).await;
        }

        let process = Arc::new(
            self.spawn_process(
                conversation_id,
                project_path.as_deref(),
                mode.as_deref(),
                coworker_id,
                session_name,
            )
            .await?,
        );
        if let Some(session_id) = conversation_id {
            self.sessions
                .lock()
                .await
                .insert(session_id.to_owned(), Arc::clone(&process));
        }
        Ok(process)
    }

    async fn spawn_process(
        &self,
        conversation_id: Option<&str>,
        project_path: Option<&str>,
        mode: Option<&str>,
        coworker_id: Option<&str>,
        session_name: Option<&str>,
    ) -> Result<ClaudeProcessHandle> {
        let resolved = self.resolved_command()?;
        let run_id = uuid::Uuid::new_v4().to_string();
        let run_store = crate::conversation_store::ConversationStore::open(
            self.config.storage_dir.join("desktop.sqlite3"),
        )?;
        run_store.set_actor_run(&run_id, ActorId::Claude, conversation_id)?;
        run_store.set_actor_run_coworker_id(&run_id, coworker_id)?;
        let sidecar_token = uuid::Uuid::new_v4().to_string();
        run_store.set_actor_run_token(&run_id, &sidecar_token)?;
        let (ipc_port, broker) = if let Some(config_path) = self.config.desktop_config_path.as_ref()
        {
            let (port, handle) =
                crate::mcp_sidecar::serve_loopback(config_path, &run_id, &sidecar_token).await?;
            (Some(port), Some(handle))
        } else {
            (None, None)
        };
        let mcp_config_path = self.write_mcp_config(&run_id, &sidecar_token, ipc_port)?;
        let cleanup = ClaudeRunCleanup {
            database_path: self.config.storage_dir.join("desktop.sqlite3"),
            run_id: run_id.clone(),
            mcp_config_path: mcp_config_path.clone(),
            broker,
        };
        let mut command = resolved.command();
        command
            .args(&self.config.args)
            .args([
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--replay-user-messages",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.kill_on_drop(true);
        let attachment_dir = self.config.storage_dir.join("attachments").join("incoming");
        std::fs::create_dir_all(&attachment_dir)?;
        command.args(["--add-dir", attachment_dir.to_string_lossy().as_ref()]);
        if let Some(path) = mcp_config_path.as_ref() {
            command.args([
                "--mcp-config",
                path.to_string_lossy().as_ref(),
                "--permission-prompt-tool",
                "mcp__coworker_desktop__request_permission",
                "--allowedTools",
                DESKTOP_ALLOWED_MCP_TOOLS,
            ]);
        }
        if let Some(session_id) = conversation_id {
            command.args(["--resume", session_id]);
        }
        if let Some(args) = claude_session_name_args(session_name) {
            command.args(args);
        }
        if let Some(mode) = mode {
            command.args(["--permission-mode", mode]);
        }
        if let Some(cwd) = project_path.filter(|value| !value.trim().is_empty()) {
            command.current_dir(cwd);
        }
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| BridgeError::message("Claude stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BridgeError::message("Claude stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| BridgeError::message("Claude stderr was not piped"))?;
        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log_subprocess_line("claude_code", &line);
            }
        });
        Ok(ClaudeProcessHandle {
            stdin: Mutex::new(stdin),
            turns: Arc::new(ClaudeTurnQueue::new()),
            acknowledged_inputs: Mutex::new(HashSet::new()),
            state: Mutex::new(ClaudeProcess {
                child,
                stdout: BufReader::new(stdout).lines(),
                session_id: conversation_id.map(str::to_owned),
                project_path: project_path.map(str::to_owned),
                mode: mode.map(str::to_owned),
                idle_generation: 1,
                idle_expired: false,
                _cleanup: cleanup,
                stderr_task,
            }),
        })
    }

    async fn send_process_message(
        &self,
        process: &Arc<ClaudeProcessHandle>,
        session_key: &str,
        input_id: &str,
        content: &str,
        cancellation: Arc<Notify>,
    ) -> Result<Value> {
        let input = json!({
            "type": "user",
            "uuid": input_id,
            "message": {"role": "user", "content": [{"type": "text", "text": content}]}
        });
        let ticket = {
            let mut stdin = process.stdin.lock().await;
            stdin
                .write_all(serde_json::to_string(&input)?.as_bytes())
                .await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
            process.turns.issue()
        };
        // Inputs are written immediately, even during an active turn. Result
        // consumption remains ordered so each caller receives the result for
        // the same user message it enqueued.
        let _turn = process.turns.wait(ticket).await;
        let mut process_state = process.state.lock().await;
        process_state.idle_generation = process_state.idle_generation.wrapping_add(1);
        self.active_sessions
            .lock()
            .await
            .insert(session_key.to_owned(), Arc::clone(&cancellation));
        if let Some(status) = process_state.child.try_wait()? {
            return Err(BridgeError::message(format!(
                "Claude Code exited with status {status}"
            )));
        }

        let mut events = Vec::new();
        let mut result_text = String::new();
        let mut stream_message_id = None;
        let mut acknowledged = process.acknowledged_inputs.lock().await.remove(input_id);
        let acknowledgement_deadline = tokio::time::Instant::now() + CLAUDE_INPUT_ACK_TIMEOUT;
        loop {
            let line = tokio::select! {
                line = process_state.stdout.next_line() => line?,
                _ = cancellation.notified() => {
                    let _ = process_state.child.start_kill();
                    return Err(BridgeError::message("Claude Code session was interrupted"));
                }
                _ = tokio::time::sleep_until(acknowledgement_deadline), if !acknowledged => {
                    return Err(BridgeError::message(format!(
                        "Claude Code did not acknowledge input {input_id} within {} seconds",
                        CLAUDE_INPUT_ACK_TIMEOUT.as_secs()
                    )));
                }
            };
            let Some(line) = line else {
                let status = process_state.child.wait().await?;
                return Err(BridgeError::message(format!(
                    "Claude Code exited before completing the turn with status {status}"
                )));
            };
            if line.trim().is_empty() {
                continue;
            }
            let event: Value = serde_json::from_str(&line).map_err(|error| {
                BridgeError::message(format!("invalid Claude stream-json event: {error}"))
            })?;
            if let Some(replayed_input_id) = replayed_input_id(&event) {
                if replayed_input_id == input_id {
                    acknowledged = true;
                } else {
                    process
                        .acknowledged_inputs
                        .lock()
                        .await
                        .insert(replayed_input_id.to_owned());
                }
            }
            if process_state.session_id.is_none() {
                process_state.session_id = event
                    .get("session_id")
                    .or_else(|| event.get("sessionId"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if let Some(value) = process_state.session_id.as_deref() {
                    let store = crate::conversation_store::ConversationStore::open(
                        &process_state._cleanup.database_path,
                    )?;
                    store.set_actor_run(
                        &process_state._cleanup.run_id,
                        ActorId::Claude,
                        Some(value),
                    )?;
                    self.active_sessions
                        .lock()
                        .await
                        .insert(value.to_owned(), Arc::clone(&cancellation));
                    self.sessions
                        .lock()
                        .await
                        .insert(value.to_owned(), Arc::clone(process));
                }
            }
            if let Some(value) = event
                .pointer("/event/message/id")
                .or_else(|| event.pointer("/message/id"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                stream_message_id = Some(value.to_owned());
            }
            if let Some(conversation_id) = process_state.session_id.as_deref() {
                publish_actor_stream_event(ActorStreamEvent {
                    actor_id: ActorId::Claude,
                    conversation_id: conversation_id.to_owned(),
                    message_id: stream_message_id.clone(),
                    event: event.clone(),
                });
            }
            if event.get("type").and_then(Value::as_str) == Some("result") {
                // A result is a stronger acknowledgement and keeps compatibility
                // with older Claude versions that do not replay stdin messages.
                acknowledged = true;
                result_text = event
                    .get("result")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
            }
            let completed = event.get("type").and_then(Value::as_str) == Some("result");
            events.push(event);
            if completed {
                break;
            }
        }
        let session_id = process_state
            .session_id
            .clone()
            .ok_or_else(|| BridgeError::message("Claude stream finished without a session_id"))?;
        Ok(json!({
            "actor_id": "claude",
            "conversation_id": session_id,
            "result": result_text,
            "events": events,
        }))
    }

    async fn evict_process(
        &self,
        process: &Arc<ClaudeProcessHandle>,
        conversation_id: Option<&str>,
    ) {
        let process_session_id = {
            let mut state = process.state.lock().await;
            state.idle_expired = true;
            let session_id = state.session_id.clone();
            if state.child.try_wait().ok().flatten().is_none() {
                let _ = state.child.start_kill();
                let _ = state.child.wait().await;
            }
            state.stderr_task.abort();
            session_id
        };
        let mut sessions = self.sessions.lock().await;
        for session_id in conversation_id
            .into_iter()
            .chain(process_session_id.as_deref())
        {
            if sessions
                .get(session_id)
                .is_some_and(|current| Arc::ptr_eq(current, process))
            {
                sessions.remove(session_id);
            }
        }
    }

    async fn schedule_idle_shutdown(&self, process: &Arc<ClaudeProcessHandle>, timeout: Duration) {
        if process.turns.has_pending() {
            return;
        }
        let (session_id, generation) = {
            let mut state = process.state.lock().await;
            state.idle_generation = state.idle_generation.wrapping_add(1);
            (state.session_id.clone(), state.idle_generation)
        };
        let Some(session_id) = session_id else {
            return;
        };
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), Arc::clone(process));

        let process = Arc::downgrade(process);
        let sessions = Arc::downgrade(&self.sessions);
        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            expire_idle_process(process, sessions, &session_id, generation).await;
        });
    }

    fn write_mcp_config(
        &self,
        run_id: &str,
        sidecar_token: &str,
        ipc_port: Option<u16>,
    ) -> Result<Option<PathBuf>> {
        let Some(config_path) = self.config.desktop_config_path.as_ref() else {
            return Ok(None);
        };
        let ipc_port =
            ipc_port.ok_or_else(|| BridgeError::message("MCP loopback broker did not start"))?;
        let executable = std::env::current_exe()?;
        let directory = self.config.storage_dir.join("mcp");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{run_id}.json"));
        let value = json!({
            "mcpServers": {
                "coworker_desktop": {
                    "type": "stdio",
                    "command": executable,
                    "args": [
                        "--mcp-sidecar",
                        "--config",
                        config_path,
                        "--run-id",
                        run_id,
                        "--sidecar-token",
                        sidecar_token,
                        "--ipc-port",
                        ipc_port.to_string(),
                    ]
                }
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&value)?)?;
        Ok(Some(path))
    }
}

async fn expire_idle_process(
    process: Weak<ClaudeProcessHandle>,
    sessions: Weak<Mutex<HashMap<String, Arc<ClaudeProcessHandle>>>>,
    session_id: &str,
    generation: u64,
) {
    let (Some(process), Some(sessions)) = (process.upgrade(), sessions.upgrade()) else {
        return;
    };
    let mut state = process.state.lock().await;
    if state.idle_expired || state.idle_generation != generation {
        return;
    }
    let session_map = sessions.lock().await;
    if !session_map
        .get(session_id)
        .is_some_and(|current| Arc::ptr_eq(current, &process))
    {
        return;
    }
    state.idle_expired = true;
    if state.child.try_wait().ok().flatten().is_none() {
        let _ = state.child.start_kill();
        let _ = state.child.wait().await;
    }
    state.stderr_task.abort();
    drop(session_map);
    let mut session_map = sessions.lock().await;
    if session_map
        .get(session_id)
        .is_some_and(|current| Arc::ptr_eq(current, &process))
    {
        session_map.remove(session_id);
    }
}

fn claude_permission_mode(mode: Option<&str>) -> Option<&str> {
    match mode {
        Some(mode @ ("acceptEdits" | "plan" | "bypassPermissions")) => Some(mode),
        _ => None,
    }
}

fn effective_claude_permission_mode<'a>(
    mode: Option<&'a str>,
    permissions_mode: &str,
) -> Option<&'a str> {
    claude_permission_mode(mode).or_else(|| {
        if mode.is_some_and(|value| value != "default") {
            return None;
        }
        match permissions_mode {
            "workspace-write" => Some("acceptEdits"),
            "danger-full-access" => Some("bypassPermissions"),
            _ => None,
        }
    })
}

fn claude_session_name_args(session_name: Option<&str>) -> Option<[&str; 2]> {
    session_name
        .filter(|value| !value.trim().is_empty())
        .map(|value| ["--name", value])
}

fn replayed_input_id(event: &Value) -> Option<&str> {
    (event.get("type").and_then(Value::as_str) == Some("user")
        && event.get("isReplay").and_then(Value::as_bool) == Some(true))
    .then(|| event.get("uuid").and_then(Value::as_str))
    .flatten()
}

#[async_trait::async_trait]
impl ActorAdapter for ClaudeAdapter {
    fn actor_id(&self) -> ActorId {
        ActorId::Claude
    }

    async fn health(&self) -> ActorHealth {
        if !self.config.enabled {
            return ActorHealth {
                actor_id: ActorId::Claude,
                available: false,
                message: "Claude actor is disabled".to_owned(),
            };
        }
        match self.resolved_command() {
            Ok(command) => ActorHealth {
                actor_id: ActorId::Claude,
                available: true,
                message: format!(
                    "Claude command resolved to {}",
                    command.display_path().display()
                ),
            },
            Err(error) => ActorHealth {
                actor_id: ActorId::Claude,
                available: false,
                message: error.to_string(),
            },
        }
    }

    async fn list_conversations(&self, limit: usize) -> Result<Vec<ActorConversation>> {
        list_history_conversations(&self.config, limit)
    }

    async fn conversation_snapshot(&self, limit: usize) -> Result<ActorConversationPage> {
        list_history_conversation_snapshot(&self.config, limit)
    }

    async fn load_messages(
        &self,
        conversation_id: &str,
        before_cursor: Option<&str>,
        page_size: usize,
    ) -> Result<ActorMessagePage> {
        let skip = before_cursor
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let target = skip.saturating_add(page_size.max(1));
        let messages =
            load_history_messages(&self.config, conversation_id, target.saturating_add(1))?;
        let end = messages.len().saturating_sub(skip);
        let start = end.saturating_sub(page_size.max(1));
        Ok(ActorMessagePage {
            messages: messages[start..end].to_vec(),
            next_before_cursor: (start > 0).then(|| skip.saturating_add(end - start).to_string()),
        })
    }

    async fn send_message(
        &self,
        conversation_id: Option<&str>,
        input: ActorMessageInput<'_>,
    ) -> Result<Value> {
        if !self.config.enabled {
            return Err(BridgeError::message("Claude actor is disabled"));
        }
        let history_project_path = if input.project_path.is_none() {
            conversation_id.and_then(|id| history_project_path(&self.config, id))
        } else {
            None
        };
        let content = content_with_actor_attachments(input.content, input.attachment_paths);
        let session_name = if conversation_id.is_none() && input.author_kind == "coworker" {
            default_conversation_title(input.content)
        } else {
            None
        };
        let content = actor_model_message(
            input.author_kind,
            input.author_id,
            input.author_label,
            &content,
        )?;
        self.run_turn(
            conversation_id,
            input.message_id,
            &content,
            input.project_path.or(history_project_path.as_deref()),
            input.mode,
            input.coworker_id,
            session_name.as_deref(),
        )
        .await
    }

    async fn interrupt(&self, conversation_id: &str) -> Result<()> {
        if let Some(cancellation) = self
            .active_sessions
            .lock()
            .await
            .get(conversation_id)
            .cloned()
        {
            cancellation.notify_one();
        }
        Ok(())
    }
}

fn content_with_actor_attachments(content: &str, paths: &[String]) -> String {
    if paths.is_empty() {
        return content.to_owned();
    }
    let manifest = paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            format!(
                "{}. {} saved_path={}",
                index + 1,
                Path::new(path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("attachment"),
                path
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if content.trim().is_empty() {
        format!("[附件]\n{manifest}")
    } else {
        format!("{content}\n\n[附件]\n{manifest}")
    }
}

pub fn list_history_conversations(
    config: &ClaudeConfig,
    limit: usize,
) -> Result<Vec<ActorConversation>> {
    Ok(list_history_conversation_snapshot(config, limit)?.conversations)
}

fn list_history_conversation_snapshot(
    config: &ClaudeConfig,
    limit: usize,
) -> Result<ActorConversationPage> {
    let mut files = collect_session_jsonl(&config.home_dir.join("projects"))?;
    files.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .map(std::cmp::Reverse)
    });
    let complete = files.len() <= limit;
    let conversations = files
        .into_iter()
        .take(limit)
        .filter_map(|path| {
            let history = read_history_summary(&path).ok()?;
            let project_name = history.cwd.as_deref().and_then(|path| {
                path.trim_end_matches(['/', '\\'])
                    .rsplit(['/', '\\'])
                    .next()
                    .map(str::to_owned)
            });
            Some(ActorConversation {
                actor_id: ActorId::Claude,
                conversation_id: history.session_id,
                title: history.title,
                project_id: history.cwd.clone(),
                project_name,
                project_path: history.cwd,
                writable: false,
                updated_at: history.updated_at,
                mode: None,
            })
        })
        .collect();
    Ok(ActorConversationPage {
        conversations,
        complete,
    })
}

fn history_project_path(config: &ClaudeConfig, conversation_id: &str) -> Option<String> {
    let path = collect_session_jsonl(&config.home_dir.join("projects"))
        .ok()?
        .into_iter()
        .find(|path| path.file_stem().and_then(|value| value.to_str()) == Some(conversation_id))?;
    read_history_summary(&path).ok()?.cwd
}

#[derive(Default)]
struct ClaudeHistorySummary {
    session_id: String,
    title: String,
    cwd: Option<String>,
    updated_at: Option<String>,
}

/// Claude stores primary transcripts at `projects/<encoded-cwd>/<session>.jsonl`.
/// Sub-agent transcripts live below a session directory and must not be exposed as
/// independent conversations in the Desktop sidebar.
fn collect_session_jsonl(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(error) => return Err(error.into()),
    };
    for project in entries {
        let project = project?.path();
        if !project.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(project)? {
            let path = entry?.path();
            if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
            {
                files.push(path);
            }
        }
    }
    Ok(files)
}

fn read_history_summary(path: &Path) -> Result<ClaudeHistorySummary> {
    let fallback_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned();
    let mut summary = ClaudeHistorySummary {
        session_id: fallback_id.clone(),
        title: format!("Claude {fallback_id}"),
        updated_at: std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .map(chrono::DateTime::<chrono::Utc>::from)
            .map(|value| value.to_rfc3339()),
        ..ClaudeHistorySummary::default()
    };
    let file = std::fs::File::open(path)?;
    let mut first_prompt = None;
    for line in std::io::BufReader::new(file)
        .lines()
        .map_while(std::result::Result::ok)
    {
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(value) = event
            .get("sessionId")
            .or_else(|| event.get("session_id"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            summary.session_id = value.to_owned();
        }
        if let Some(value) = event
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            summary.cwd = Some(value.to_owned());
        }
        if let Some(value) = event
            .get("timestamp")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            summary.updated_at = Some(value.to_owned());
        }
        if event.get("type").and_then(Value::as_str) == Some("ai-title")
            && let Some(value) = event
                .get("aiTitle")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
        {
            summary.title = value.trim().to_owned();
        }
        if first_prompt.is_none()
            && event.get("type").and_then(Value::as_str) == Some("user")
            && !event
                .get("isMeta")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            first_prompt = history_content_text(event.pointer("/message/content"));
        }
    }
    if summary.title == format!("Claude {fallback_id}")
        && let Some(prompt) = first_prompt
    {
        summary.title = prompt.chars().take(60).collect();
    }
    Ok(summary)
}

pub fn load_history_messages(
    config: &ClaudeConfig,
    conversation_id: &str,
    limit: usize,
) -> Result<Vec<ActorMessage>> {
    let path = collect_session_jsonl(&config.home_dir.join("projects"))?
        .into_iter()
        .find(|path| path.file_stem().and_then(|value| value.to_str()) == Some(conversation_id))
        .ok_or_else(|| {
            BridgeError::message(format!("Claude transcript not found: {conversation_id}"))
        })?;
    let fallback_time = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .ok()
        .map(chrono::DateTime::<chrono::Utc>::from)
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let file = std::fs::File::open(path)?;
    let mut messages = Vec::new();
    let mut tool_positions: HashMap<String, usize> = HashMap::new();
    for (index, line) in std::io::BufReader::new(file).lines().enumerate() {
        let Ok(line) = line else { continue };
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(event_type @ ("user" | "assistant")) = event.get("type").and_then(Value::as_str)
        else {
            continue;
        };
        if event
            .get("isMeta")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || event
                .get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            continue;
        }
        let event_id = event
            .get("uuid")
            .or_else(|| event.pointer("/message/id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("claude-{conversation_id}-{index}"));
        let created_at = event
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or(&fallback_time)
            .to_owned();
        match event.pointer("/message/content") {
            Some(Value::String(content)) if !content.trim().is_empty() => {
                messages.push(history_message(
                    &event_id,
                    conversation_id,
                    if event_type == "user" {
                        "local"
                    } else {
                        "assistant"
                    },
                    content.trim().to_owned(),
                    &created_at,
                    json!({"source": "claude-history", "kind": "text"}),
                ));
            }
            Some(Value::Array(blocks)) => {
                for (block_index, block) in blocks.iter().enumerate() {
                    let block_id = format!("{event_id}:{block_index}");
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(text) = block
                                .get("text")
                                .and_then(Value::as_str)
                                .filter(|value| !value.trim().is_empty())
                            {
                                messages.push(history_message(
                                    &block_id,
                                    conversation_id,
                                    if event_type == "user" {
                                        "local"
                                    } else {
                                        "assistant"
                                    },
                                    text.trim().to_owned(),
                                    &created_at,
                                    json!({"source": "claude-history", "kind": "text"}),
                                ));
                            }
                        }
                        Some("thinking") => {
                            if let Some(thinking) = block
                                .get("thinking")
                                .and_then(Value::as_str)
                                .filter(|value| !value.trim().is_empty())
                            {
                                messages.push(history_message(
                                    &block_id,
                                    conversation_id,
                                    "assistant",
                                    thinking.trim().to_owned(),
                                    &created_at,
                                    json!({"source": "claude-history", "kind": "reasoning"}),
                                ));
                            }
                        }
                        Some("tool_use") => {
                            let tool_use_id = block
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or(&block_id)
                                .to_owned();
                            let tool_name = block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("tool")
                                .to_owned();
                            let input = block.get("input").cloned().unwrap_or(Value::Null);
                            let content = format_tool_exchange(&tool_name, &input, None, false);
                            tool_positions.insert(tool_use_id.clone(), messages.len());
                            messages.push(history_message(
                                &block_id,
                                conversation_id,
                                "tool",
                                content,
                                &created_at,
                                json!({
                                    "source": "claude-history",
                                    "kind": "tool",
                                    "tool_use_id": tool_use_id,
                                    "tool_name": tool_name,
                                    "input": input,
                                    "output": Value::Null,
                                    "is_error": false,
                                }),
                            ));
                        }
                        Some("tool_result") => {
                            let tool_use_id = block
                                .get("tool_use_id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned();
                            let output = block.get("content").cloned().unwrap_or(Value::Null);
                            let is_error = block
                                .get("is_error")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            if let Some(position) = tool_positions.get(&tool_use_id).copied() {
                                let message = &mut messages[position];
                                let tool_name = message
                                    .metadata
                                    .get("tool_name")
                                    .and_then(Value::as_str)
                                    .unwrap_or("tool")
                                    .to_owned();
                                let input = message
                                    .metadata
                                    .get("input")
                                    .cloned()
                                    .unwrap_or(Value::Null);
                                message.content = format_tool_exchange(
                                    &tool_name,
                                    &input,
                                    Some(&output),
                                    is_error,
                                );
                                message.metadata["output"] = output;
                                message.metadata["is_error"] = Value::Bool(is_error);
                                message.metadata["result_id"] = Value::String(block_id);
                                message.metadata["result_created_at"] =
                                    Value::String(created_at.clone());
                            } else {
                                messages.push(history_message(
                                    &block_id,
                                    conversation_id,
                                    "tool",
                                    format!("[未匹配的工具结果]\n{}", history_value_text(&output)),
                                    &created_at,
                                    json!({
                                        "source": "claude-history",
                                        "kind": "tool_result",
                                        "tool_use_id": tool_use_id,
                                        "output": output,
                                        "is_error": is_error,
                                    }),
                                ));
                            }
                        }
                        Some("image") => messages.push(history_message(
                            &block_id,
                            conversation_id,
                            if event_type == "user" {
                                "local"
                            } else {
                                "assistant"
                            },
                            "[图片]".to_owned(),
                            &created_at,
                            json!({"source": "claude-history", "kind": "image"}),
                        )),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    let start = messages.len().saturating_sub(limit.max(1));
    Ok(messages.split_off(start))
}

fn history_message(
    id: &str,
    conversation_id: &str,
    author_kind: &str,
    content: String,
    created_at: &str,
    metadata: Value,
) -> ActorMessage {
    ActorMessage {
        id: id.to_owned(),
        actor_id: ActorId::Claude,
        conversation_id: conversation_id.to_owned(),
        author_kind: author_kind.to_owned(),
        content,
        created_at: created_at.to_owned(),
        metadata,
    }
}

fn format_tool_exchange(
    tool_name: &str,
    input: &Value,
    output: Option<&Value>,
    is_error: bool,
) -> String {
    let mut text = format!("[工具调用] {tool_name}\n{}", history_value_text(input));
    if let Some(output) = output {
        let label = if is_error {
            "工具错误"
        } else {
            "工具结果"
        };
        text.push_str(&format!("\n\n[{label}]\n{}", history_value_text(output)));
    }
    text
}

fn history_value_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(values) => values
            .iter()
            .filter_map(|value| match value.get("type").and_then(Value::as_str) {
                Some("text") => value.get("text").and_then(Value::as_str).map(str::to_owned),
                Some("image") => Some("[图片]".to_owned()),
                _ => Some(serde_json::to_string_pretty(value).unwrap_or_default()),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        value => serde_json::to_string_pretty(value).unwrap_or_default(),
    }
}

pub fn merge_history_messages(
    config: &ClaudeConfig,
    conversation_id: &str,
    stored: Vec<ActorMessage>,
    limit: usize,
) -> Vec<ActorMessage> {
    let Ok(mut history) = load_history_messages(config, conversation_id, limit) else {
        return stored;
    };
    let mut overlays = Vec::new();
    for message in stored {
        if matches!(message.author_kind.as_str(), "local" | "coworker") {
            // Claude records every prompt as a native `user` event. Desktop
            // overlays carry the real author, so consume exactly one matching
            // native event before adding the overlay. This also handles the
            // same prompt being sent more than once without collapsing turns.
            let native_content = message
                .metadata
                .get("native_content")
                .and_then(Value::as_str)
                .unwrap_or(&message.content);
            if let Some(position) = history.iter().position(|native| {
                native.author_kind == "local" && native.content == native_content
            }) {
                history.remove(position);
            }
            overlays.push(message);
            continue;
        }

        let already_native = history.iter().any(|native| {
            native.author_kind == message.author_kind && native.content == message.content
        });
        if !already_native {
            overlays.push(message);
        }
    }
    history.extend(overlays);
    history.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let start = history.len().saturating_sub(limit.max(1));
    history.split_off(start)
}

fn history_content_text(content: Option<&Value>) -> Option<String> {
    let content = content?;
    let text = match content {
        Value::String(value) => value.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str).map(str::to_owned))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    };
    (!text.trim().is_empty()).then(|| text.trim().to_owned())
}

fn default_claude_home() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|home| PathBuf::from(home).join(".claude"))
        })
        .unwrap_or_else(|| PathBuf::from(".claude"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_modes_match_claude_code_cli() {
        assert_eq!(claude_permission_mode(Some("default")), None);
        for mode in ["acceptEdits", "plan", "bypassPermissions"] {
            assert_eq!(claude_permission_mode(Some(mode)), Some(mode));
        }
        assert_eq!(claude_permission_mode(Some("codex-mode")), None);
    }

    #[test]
    fn default_permission_mode_follows_desktop_permissions() {
        assert_eq!(
            effective_claude_permission_mode(Some("default"), "read-only"),
            None
        );
        assert_eq!(
            effective_claude_permission_mode(Some("default"), "workspace-write"),
            Some("acceptEdits")
        );
        assert_eq!(
            effective_claude_permission_mode(None, "danger-full-access"),
            Some("bypassPermissions")
        );
        assert_eq!(
            effective_claude_permission_mode(Some("plan"), "danger-full-access"),
            Some("plan")
        );
    }

    #[test]
    fn session_name_uses_claude_code_native_flag() {
        assert_eq!(
            claude_session_name_args(Some("原始消息标题")),
            Some(["--name", "原始消息标题"])
        );
        assert_eq!(claude_session_name_args(Some("  ")), None);
    }

    #[test]
    fn replayed_user_message_acknowledges_its_input_uuid() {
        assert_eq!(
            replayed_input_id(&json!({
                "type": "user",
                "uuid": "input-1",
                "isReplay": true,
            })),
            Some("input-1")
        );
        assert_eq!(
            replayed_input_id(&json!({"type": "user", "uuid": "input-1"})),
            None
        );
    }

    #[test]
    fn desktop_communication_tools_bypass_the_permission_prompt_tool() {
        assert!(DESKTOP_ALLOWED_MCP_TOOLS.contains("mcp__coworker_desktop__list_coworkers"));
        assert!(DESKTOP_ALLOWED_MCP_TOOLS.contains("mcp__coworker_desktop__send_to_coworker"));
        assert!(!DESKTOP_ALLOWED_MCP_TOOLS.contains("request_permission"));
    }

    #[tokio::test]
    async fn disabled_adapter_is_unavailable() {
        let adapter = ClaudeAdapter::new(ClaudeConfig {
            enabled: false,
            ..ClaudeConfig::default()
        });
        assert!(!adapter.health().await.available);
    }

    #[tokio::test]
    async fn consecutive_turns_reuse_the_same_cli_process() {
        let root = std::env::temp_dir().join(format!(
            "coworker-claude-persistent-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();

        #[cfg(windows)]
        let (command, args) = {
            let script = root.join("fake-claude.cmd");
            std::fs::write(
                &script,
                concat!(
                    "@echo off\r\n",
                    "set /p first=\r\n",
                    "echo {\"type\":\"result\",\"session_id\":\"persistent-session\",\"result\":\"first\"}\r\n",
                    "set /p second=\r\n",
                    "echo {\"type\":\"result\",\"session_id\":\"persistent-session\",\"result\":\"second\"}\r\n",
                    "ping -n 30 127.0.0.1 ^>nul\r\n",
                ),
            )
            .unwrap();
            (
                "cmd.exe".to_owned(),
                vec![
                    "/D".to_owned(),
                    "/S".to_owned(),
                    "/C".to_owned(),
                    script.to_string_lossy().into_owned(),
                ],
            )
        };
        #[cfg(not(windows))]
        let (command, args) = {
            use std::os::unix::fs::PermissionsExt;

            let script = root.join("fake-claude.sh");
            std::fs::write(
                &script,
                concat!(
                    "#!/bin/sh\n",
                    "read first\n",
                    "echo '{\"type\":\"result\",\"session_id\":\"persistent-session\",\"result\":\"first\"}'\n",
                    "read second\n",
                    "echo '{\"type\":\"result\",\"session_id\":\"persistent-session\",\"result\":\"second\"}'\n",
                    "sleep 30\n",
                ),
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            (script.to_string_lossy().into_owned(), Vec::new())
        };

        let adapter = ClaudeAdapter::new(ClaudeConfig {
            command,
            args,
            storage_dir: root.clone(),
            desktop_config_path: None,
            ..ClaudeConfig::default()
        });
        let first = adapter
            .run_turn(
                None,
                None,
                "one",
                Some(root.to_string_lossy().as_ref()),
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let second = adapter
            .run_turn(
                Some("persistent-session"),
                None,
                "two",
                Some(root.to_string_lossy().as_ref()),
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(first["result"], "first");
        assert_eq!(second["result"], "second");
        assert_eq!(adapter.sessions.lock().await.len(), 1);
        let process = adapter
            .sessions
            .lock()
            .await
            .get("persistent-session")
            .cloned()
            .unwrap();
        adapter
            .schedule_idle_shutdown(&process, Duration::from_millis(20))
            .await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while !adapter.sessions.lock().await.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("idle Claude session should be removed");
        let mut state = process.state.lock().await;
        assert!(state.idle_expired);
        tokio::time::timeout(Duration::from_secs(2), state.child.wait())
            .await
            .expect("idle Claude process should be terminated")
            .unwrap();
        drop(state);
        drop(process);
        drop(adapter);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn active_turn_accepts_next_input_before_first_result() {
        let root =
            std::env::temp_dir().join(format!("coworker-claude-queued-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();

        #[cfg(windows)]
        let (command, args) = {
            let script = root.join("fake-claude.ps1");
            std::fs::write(
                &script,
                concat!(
                    "$first = [Console]::In.ReadLine()\r\n",
                    "[Console]::Out.WriteLine('{\"type\":\"system\",\"session_id\":\"queued-session\"}')\r\n",
                    "$second = [Console]::In.ReadLine()\r\n",
                    "[Console]::Out.WriteLine('{\"type\":\"user\",\"session_id\":\"queued-session\",\"uuid\":\"input-two\",\"isReplay\":true}')\r\n",
                    "[Console]::Out.WriteLine('{\"type\":\"result\",\"session_id\":\"queued-session\",\"result\":\"first\"}')\r\n",
                    "[Console]::Out.WriteLine('{\"type\":\"result\",\"session_id\":\"queued-session\",\"result\":\"second\"}')\r\n",
                    "Start-Sleep -Seconds 30\r\n",
                ),
            )
            .unwrap();
            (
                "powershell.exe".to_owned(),
                vec![
                    "-NoLogo".to_owned(),
                    "-NoProfile".to_owned(),
                    "-NonInteractive".to_owned(),
                    "-ExecutionPolicy".to_owned(),
                    "Bypass".to_owned(),
                    "-File".to_owned(),
                    script.to_string_lossy().into_owned(),
                ],
            )
        };
        #[cfg(not(windows))]
        let (command, args) = {
            use std::os::unix::fs::PermissionsExt;

            let script = root.join("fake-claude.sh");
            std::fs::write(
                &script,
                concat!(
                    "#!/bin/sh\n",
                    "read first\n",
                    "echo '{\"type\":\"system\",\"session_id\":\"queued-session\"}'\n",
                    "read second\n",
                    "echo '{\"type\":\"user\",\"session_id\":\"queued-session\",\"uuid\":\"input-two\",\"isReplay\":true}'\n",
                    "echo '{\"type\":\"result\",\"session_id\":\"queued-session\",\"result\":\"first\"}'\n",
                    "echo '{\"type\":\"result\",\"session_id\":\"queued-session\",\"result\":\"second\"}'\n",
                    "sleep 30\n",
                ),
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            (script.to_string_lossy().into_owned(), Vec::new())
        };

        let adapter = ClaudeAdapter::new(ClaudeConfig {
            command,
            args,
            storage_dir: root.clone(),
            desktop_config_path: None,
            ..ClaudeConfig::default()
        });
        let project_path = root.to_string_lossy().into_owned();
        let first_adapter = adapter.clone();
        let first_project_path = project_path.clone();
        let first = tokio::spawn(async move {
            first_adapter
                .run_turn(
                    Some("queued-session"),
                    Some("input-one"),
                    "one",
                    Some(&first_project_path),
                    None,
                    None,
                    None,
                )
                .await
        });

        for _ in 0..100 {
            if adapter
                .active_sessions
                .lock()
                .await
                .contains_key("queued-session")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            adapter
                .active_sessions
                .lock()
                .await
                .contains_key("queued-session"),
            "first Claude turn should be active before the reply arrives"
        );

        let second_adapter = adapter.clone();
        let second = tokio::spawn(async move {
            second_adapter
                .run_turn(
                    Some("queued-session"),
                    Some("input-two"),
                    "two",
                    Some(&project_path),
                    None,
                    None,
                    None,
                )
                .await
        });

        let (first, second) = tokio::time::timeout(Duration::from_secs(3), async {
            (
                first.await.unwrap().unwrap(),
                second.await.unwrap().unwrap(),
            )
        })
        .await
        .expect("second input should reach Claude before the first result");
        assert_eq!(first["result"], "first");
        assert_eq!(second["result"], "second");

        drop(adapter);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn history_is_read_only() {
        let root =
            std::env::temp_dir().join(format!("coworker-claude-history-{}", uuid::Uuid::new_v4()));
        let projects = root.join("projects/p1");
        std::fs::create_dir_all(&projects).unwrap();
        std::fs::write(projects.join("session-1.jsonl"), "{}\n").unwrap();
        let adapter = ClaudeAdapter::new(ClaudeConfig {
            home_dir: root.clone(),
            enabled: false,
            ..ClaudeConfig::default()
        });
        let sessions = adapter.list_conversations(10).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(!sessions[0].writable);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn history_uses_transcript_metadata_and_ignores_subagents() {
        let root =
            std::env::temp_dir().join(format!("coworker-claude-metadata-{}", uuid::Uuid::new_v4()));
        let project = root.join("projects/encoded-project");
        std::fs::create_dir_all(project.join("session-1/subagents")).unwrap();
        std::fs::write(
            project.join("session-1.jsonl"),
            concat!(
                "{\"type\":\"user\",\"sessionId\":\"session-1\",\"cwd\":\"D:\\\\work\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"uuid\":\"u1\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n",
                "{\"type\":\"ai-title\",\"sessionId\":\"session-1\",\"aiTitle\":\"Useful title\"}\n",
                "{\"type\":\"assistant\",\"sessionId\":\"session-1\",\"timestamp\":\"2026-01-01T00:00:01Z\",\"uuid\":\"a1\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"world\"}]}}\n"
            ),
        )
        .unwrap();
        std::fs::write(
            project.join("session-1/subagents/agent-noise.jsonl"),
            "{}\n",
        )
        .unwrap();
        let adapter = ClaudeAdapter::new(ClaudeConfig {
            home_dir: root.clone(),
            ..ClaudeConfig::default()
        });

        let sessions = adapter.list_conversations(10).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].conversation_id, "session-1");
        assert_eq!(sessions[0].title, "Useful title");
        assert_eq!(sessions[0].project_path.as_deref(), Some("D:\\work"));
        assert_eq!(
            sessions[0].updated_at.as_deref(),
            Some("2026-01-01T00:00:01Z")
        );
        assert_eq!(
            history_project_path(&adapter.config, "session-1").as_deref(),
            Some("D:\\work")
        );
        let snapshot = adapter.conversation_snapshot(1).await.unwrap();
        assert!(snapshot.complete);
        std::fs::write(project.join("session-2.jsonl"), "{}\n").unwrap();
        let snapshot = adapter.conversation_snapshot(1).await.unwrap();
        assert!(!snapshot.complete);

        let messages = load_history_messages(&adapter.config, "session-1", 10).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].author_kind, "local");
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[1].author_kind, "assistant");
        assert_eq!(messages[1].content, "world");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn history_pairs_tool_results_without_treating_them_as_local_user_messages() {
        let root =
            std::env::temp_dir().join(format!("coworker-claude-tools-{}", uuid::Uuid::new_v4()));
        let project = root.join("projects/encoded-project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("session-tools.jsonl"),
            concat!(
                "{\"type\":\"user\",\"sessionId\":\"session-tools\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"uuid\":\"user-1\",\"message\":{\"role\":\"user\",\"content\":\"inspect it\"}}\n",
                "{\"type\":\"assistant\",\"sessionId\":\"session-tools\",\"timestamp\":\"2026-01-01T00:00:01Z\",\"uuid\":\"assistant-1\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"thinking\",\"thinking\":\"I should read it\"},{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"Read\",\"input\":{\"file_path\":\"a.txt\"}}]}}\n",
                "{\"type\":\"user\",\"sessionId\":\"session-tools\",\"timestamp\":\"2026-01-01T00:00:02Z\",\"uuid\":\"result-1\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"file body\"}]}}\n",
                "{\"type\":\"assistant\",\"sessionId\":\"session-tools\",\"timestamp\":\"2026-01-01T00:00:03Z\",\"uuid\":\"assistant-2\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"done\"}]}}\n"
            ),
        )
        .unwrap();
        let config = ClaudeConfig {
            home_dir: root.clone(),
            ..ClaudeConfig::default()
        };

        let messages = load_history_messages(&config, "session-tools", 20).unwrap();

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].author_kind, "local");
        assert_eq!(messages[1].metadata["kind"], "reasoning");
        assert_eq!(messages[2].author_kind, "tool");
        assert_eq!(messages[2].metadata["tool_use_id"], "toolu_1");
        assert_eq!(messages[2].metadata["tool_name"], "Read");
        assert_eq!(messages[2].metadata["output"], "file body");
        assert!(messages[2].content.contains("[工具调用] Read"));
        assert!(messages[2].content.contains("[工具结果]\nfile body"));
        assert_eq!(messages[3].author_kind, "assistant");
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.author_kind == "local")
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn desktop_overlay_replaces_native_claude_user_event_without_duplicates() {
        let root =
            std::env::temp_dir().join(format!("coworker-claude-overlay-{}", uuid::Uuid::new_v4()));
        let project = root.join("projects/encoded-project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("session-overlay.jsonl"),
            concat!(
                "{\"type\":\"user\",\"sessionId\":\"session-overlay\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"uuid\":\"user-1\",\"message\":{\"role\":\"user\",\"content\":\"[Coworker] hello\"}}\n",
                "{\"type\":\"assistant\",\"sessionId\":\"session-overlay\",\"timestamp\":\"2026-01-01T00:00:01Z\",\"uuid\":\"assistant-1\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"world\"}]}}\n"
            ),
        )
        .unwrap();
        let config = ClaudeConfig {
            home_dir: root.clone(),
            ..ClaudeConfig::default()
        };
        let stored = vec![
            history_message(
                "overlay-user",
                "session-overlay",
                "coworker",
                "hello".to_owned(),
                "2026-01-01T00:00:00Z",
                json!({"native_content": "[Coworker] hello"}),
            ),
            history_message(
                "overlay-result",
                "session-overlay",
                "assistant",
                "world".to_owned(),
                "2026-01-01T00:00:01Z",
                json!({"local_only": true}),
            ),
        ];

        let messages = merge_history_messages(&config, "session-overlay", stored, 20);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].author_kind, "coworker");
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[1].author_kind, "assistant");
        assert_eq!(messages[1].content, "world");
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.author_kind == "local")
                .count(),
            0
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
