use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::Serialize;
use tokio::{
    sync::{Mutex, mpsc, oneshot},
    task::JoinHandle,
};

use crate::{
    actor::{ActorHealth, CodexActorAdapter},
    app_server::CodexAppServerClient,
    bridge::CodexBridge,
    claude::ClaudeAdapter,
    config::{BridgeConfig, BridgeCoworker, DesktopConfig},
    desktop_router::DesktopRouter,
    error::{BridgeError, Result},
    lock::BridgeInstanceLock,
    logging::{error_chain, init_logging, update_log_levels},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeRuntimeState {
    Stopped,
    Running,
    Exited,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeRuntimeStatus {
    pub state: BridgeRuntimeState,
    pub config_path: Option<String>,
    pub codex_id: Option<String>,
    pub desktop_id: Option<String>,
    pub coworkers: Vec<BridgeCoworker>,
    pub actors: Vec<ActorHealth>,
    pub development_mode: bool,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct BridgeRuntime {
    running: Mutex<Option<RunningBridge>>,
    last_error: Arc<Mutex<Option<String>>>,
}

struct RunningBridge {
    config_path: PathBuf,
    config: BridgeConfig,
    desktop_config: DesktopConfig,
    bridge: Option<Arc<CodexBridge>>,
    desktop_router: Arc<DesktopRouter>,
    actors: Vec<ActorHealth>,
    shutdown: Option<oneshot::Sender<()>>,
    handle: JoinHandle<Result<()>>,
}

impl BridgeRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn start(&self, config_path: impl AsRef<Path>) -> Result<BridgeRuntimeStatus> {
        let config_path = config_path.as_ref().to_path_buf();
        let mut running = self.running.lock().await;
        if let Some(current) = running.as_ref()
            && !current.handle.is_finished()
        {
            return Err(BridgeError::startup("Codex bridge is already running"));
        }
        if let Some(current) = running.take() {
            let _ = current.handle.await;
        }

        let desktop_config = DesktopConfig::from_file(&config_path)?;
        let config = desktop_config.codex.clone();
        init_logging(&config)?;
        let mut instance_lock = BridgeInstanceLock::new(&config);
        instance_lock.acquire(&config)?;

        let mut codex_runtime = None;
        let (outbound_tx, outbound_rx) = mpsc::channel(128);
        if desktop_config.codex_enabled {
            let (client, notifications, server_requests) =
                CodexAppServerClient::new(config.command.clone(), config.args.clone());
            let bridge = CodexBridge::new(config.clone(), client.clone(), outbound_tx)?;
            match client.start().await {
                Ok(()) => codex_runtime = Some((client, bridge, notifications, server_requests)),
                Err(error) => {
                    tracing::warn!(%error, "Codex actor unavailable; CoWorker Desktop will continue");
                }
            }
        }

        let running_bridge = codex_runtime
            .as_ref()
            .map(|(_, bridge, _, _)| Arc::clone(bridge));
        let mut adapters: Vec<Arc<dyn crate::actor::ActorAdapter>> = Vec::new();
        if let Some(bridge) = running_bridge.as_ref() {
            adapters.push(Arc::new(CodexActorAdapter::new(Arc::clone(bridge))));
        }
        adapters.push(Arc::new(ClaudeAdapter::new(desktop_config.claude.clone())));
        let desktop_router = Arc::new(DesktopRouter::new(desktop_config.clone(), adapters)?);
        let actors = desktop_router.actor_health().await;

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let last_error = Arc::clone(&self.last_error);
        let running_router = Arc::clone(&desktop_router);
        let handle = tokio::spawn(async move {
            let (router_shutdown_tx, router_shutdown_rx) = oneshot::channel();
            let router_task = tokio::spawn(async move {
                running_router
                    .run_until_shutdown(router_shutdown_rx, outbound_rx)
                    .await
            });
            let mut codex_shutdown_tx = None;
            let mut codex_task = None;
            let mut codex_client = None;
            if let Some((client, bridge, notifications, server_requests)) = codex_runtime {
                let (tx, rx) = oneshot::channel();
                codex_shutdown_tx = Some(tx);
                codex_client = Some(client);
                codex_task = Some(tokio::spawn(async move {
                    bridge
                        .run_app_server_until_shutdown(notifications, server_requests, rx)
                        .await
                }));
            }
            let _ = shutdown_rx.await;
            let _ = router_shutdown_tx.send(());
            if let Some(tx) = codex_shutdown_tx {
                let _ = tx.send(());
            }
            let router_result = router_task.await.map_err(|error| {
                BridgeError::startup(format!("desktop router task failed: {error}"))
            })?;
            let run_result = match codex_task {
                Some(task) => task.await.map_err(|error| {
                    BridgeError::startup(format!("Codex bridge task failed: {error}"))
                })?,
                None => Ok(()),
            };
            let stop_result = match codex_client {
                Some(client) => client.stop().await,
                None => Ok(()),
            };
            instance_lock.release();

            let result = match (router_result, run_result, stop_result) {
                (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => Err(error),
                _ => Ok(()),
            };
            if let Err(error) = &result {
                tracing::error!("{}", error_chain(error));
                *last_error.lock().await = Some(error.to_string());
            }
            result
        });

        *self.last_error.lock().await = None;
        *running = Some(RunningBridge {
            config_path: config_path.clone(),
            config,
            desktop_config,
            bridge: running_bridge,
            desktop_router,
            actors,
            shutdown: Some(shutdown_tx),
            handle,
        });
        Ok(status_from_running(running.as_ref(), None))
    }

    pub async fn stop(&self) -> Result<BridgeRuntimeStatus> {
        let current = {
            let mut running = self.running.lock().await;
            running.take()
        };
        let Some(mut current) = current else {
            let last_error = self.last_error.lock().await.clone();
            return Ok(status_from_running(None, last_error));
        };
        if let Some(shutdown) = current.shutdown.take() {
            let _ = shutdown.send(());
        }
        match current.handle.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::error!("{}", error_chain(&error));
                let message = error.to_string();
                *self.last_error.lock().await = Some(message);
                return Err(error);
            }
            Err(error) => {
                tracing::error!("{}", error_chain(&error));
                let message = format!("bridge task failed: {error}");
                *self.last_error.lock().await = Some(message.clone());
                return Err(BridgeError::startup(message));
            }
        }
        Ok(status_from_running(None, None))
    }

    pub async fn status(&self) -> BridgeRuntimeStatus {
        let running = self.running.lock().await;
        let last_error = self.last_error.lock().await.clone();
        status_from_running(running.as_ref(), last_error)
    }

    pub async fn bridge(&self) -> Option<Arc<CodexBridge>> {
        let running = self.running.lock().await;
        running
            .as_ref()
            .filter(|current| !current.handle.is_finished())
            .and_then(|current| current.bridge.as_ref().map(Arc::clone))
    }

    pub async fn desktop_router(&self) -> Option<Arc<DesktopRouter>> {
        let running = self.running.lock().await;
        running
            .as_ref()
            .filter(|current| !current.handle.is_finished())
            .map(|current| Arc::clone(&current.desktop_router))
    }

    pub async fn apply_saved_config(
        &self,
        config_path: &Path,
        config: &DesktopConfig,
    ) -> Result<bool> {
        let mut running = self.running.lock().await;
        let Some(current) = running
            .as_mut()
            .filter(|current| !current.handle.is_finished() && current.config_path == config_path)
        else {
            return Ok(false);
        };

        if only_log_levels_changed(&current.desktop_config, config)
            && update_log_levels(&config.codex.log_level, &config.codex.file_log_level)
        {
            current.config.log_level = config.codex.log_level.clone();
            current.config.file_log_level = config.codex.file_log_level.clone();
            current.desktop_config.codex.log_level = config.codex.log_level.clone();
            current.desktop_config.codex.file_log_level = config.codex.file_log_level.clone();
            return Ok(true);
        }

        drop(running);
        self.stop().await?;
        self.start(config_path).await?;
        Ok(true)
    }
}

fn only_log_levels_changed(current: &DesktopConfig, next: &DesktopConfig) -> bool {
    let mut current = current.clone();
    current.codex.log_level = next.codex.log_level.clone();
    current.codex.file_log_level = next.codex.file_log_level.clone();
    current == *next
}

fn status_from_running(
    running: Option<&RunningBridge>,
    last_error: Option<String>,
) -> BridgeRuntimeStatus {
    match running {
        Some(current) => BridgeRuntimeStatus {
            state: if current.handle.is_finished() {
                BridgeRuntimeState::Exited
            } else {
                BridgeRuntimeState::Running
            },
            config_path: Some(current.config_path.display().to_string()),
            codex_id: Some(current.config.codex_id.clone()),
            desktop_id: Some(current.desktop_config.desktop_id.clone()),
            coworkers: current.config.coworkers.clone(),
            actors: current.actors.clone(),
            development_mode: current.desktop_config.security.development_mode,
            last_error,
        },
        None => BridgeRuntimeStatus {
            state: BridgeRuntimeState::Stopped,
            config_path: None,
            codex_id: None,
            desktop_id: None,
            coworkers: Vec::new(),
            actors: Vec::new(),
            development_mode: false,
            last_error,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_config_value;
    use serde_json::json;
    use tokio::time::{Duration, timeout};

    #[test]
    fn detects_when_a_saved_config_only_changes_log_levels() {
        let mut value = default_config_value("desktop-test", "https://coworker.example.test");
        value["coworkers"][0]["bearer_token"] = json!("test-token");
        let current = DesktopConfig::from_value(value).expect("desktop config");
        let mut next = current.clone();
        next.codex.log_level = "DEBUG".to_owned();
        next.codex.file_log_level = "DEBUG".to_owned();

        assert!(only_log_levels_changed(&current, &next));

        next.desktop_id = "another-desktop".to_owned();
        assert!(!only_log_levels_changed(&current, &next));
    }

    #[tokio::test]
    async fn stop_when_not_running_returns_stopped() {
        let runtime = BridgeRuntime::new();

        let status = timeout(Duration::from_millis(100), runtime.stop())
            .await
            .expect("stop should not hang when runtime is not running")
            .expect("stop should succeed when runtime is not running");

        assert!(matches!(status.state, BridgeRuntimeState::Stopped));
    }
}
