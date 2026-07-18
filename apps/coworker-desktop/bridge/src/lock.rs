use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

use fs4::FileExt;
use serde_json::json;

use crate::{
    config::BridgeConfig,
    error::{BridgeError, Result},
};

const BRIDGE_LOCK_FILE_NAME: &str = "coworker-desktop.lock";

pub struct BridgeInstanceLock {
    path: PathBuf,
    file: Option<File>,
}

impl BridgeInstanceLock {
    pub fn new(config: &BridgeConfig) -> Self {
        Self {
            path: lock_path(config),
            file: None,
        }
    }

    pub fn acquire(&mut self, config: &BridgeConfig) -> Result<()> {
        if let Err(error) = self.acquire_path(config) {
            self.release();
            return Err(BridgeError::startup(format!(
                "Another CoWorker Desktop instance is already running: {error}"
            )));
        }
        Ok(())
    }

    pub fn release(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = FileExt::unlock(&file);
        }
    }

    fn acquire_path(&mut self, config: &BridgeConfig) -> Result<()> {
        let path = &self.path;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        if file.metadata()?.len() == 0 {
            file.write_all(&[0])?;
            file.flush()?;
        } else {
            let mut first = [0u8; 1];
            file.seek(SeekFrom::Start(0))?;
            let _ = file.read(&mut first)?;
        }
        FileExt::try_lock(&file).map_err(std::io::Error::from)?;
        file.seek(SeekFrom::Start(1))?;
        file.set_len(1)?;
        let metadata = json!({
            "pid": std::process::id(),
            "codex_id": config.codex_id,
            "display_name": config.display_name,
            "service_name": config.service_name,
            "coworkers": config.coworkers.iter().map(|c| {
                json!({
                    "coworker_id": c.coworker_id,
                    "base_url": c.base_url,
                })
            }).collect::<Vec<_>>(),
        });
        file.write_all(serde_json::to_string_pretty(&metadata)?.as_bytes())?;
        file.flush()?;
        self.file = Some(file);
        Ok(())
    }
}

impl Drop for BridgeInstanceLock {
    fn drop(&mut self) {
        self.release();
    }
}

fn lock_path(config: &BridgeConfig) -> PathBuf {
    lock_root(config).join(BRIDGE_LOCK_FILE_NAME)
}

fn lock_root(config: &BridgeConfig) -> PathBuf {
    config
        .state_path
        .as_ref()
        .map(|path| {
            let mut root = PathBuf::from(path);
            root.pop();
            root.push("locks");
            root
        })
        .unwrap_or_else(|| PathBuf::from("data/coworker_desktop_locks"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BridgeConfig, BridgeCoworker};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn lock_path_uses_one_bridge_lock_per_state_dir() {
        let config = config_with_state_path(
            "data/coworker_desktop_state.json",
            "cw",
            "http://localhost:8000",
        );
        let path = lock_path(&config);

        assert!(path.to_string_lossy().contains("data"));
        assert!(path.to_string_lossy().contains("locks"));
        assert!(path.ends_with(BRIDGE_LOCK_FILE_NAME));
    }

    #[test]
    fn different_coworkers_share_the_same_bridge_lock_path() {
        let first = config_with_state_path(
            "data/coworker_desktop_state.json",
            "cw_01",
            "http://localhost:8000",
        );
        let second = config_with_state_path(
            "data/coworker_desktop_state.json",
            "cw_02",
            "http://localhost:9000",
        );

        assert_eq!(lock_path(&first), lock_path(&second));
    }

    #[test]
    fn acquire_blocks_second_bridge_in_same_state_dir() {
        let dir = temp_dir("lock");
        let state_path = dir.join("coworker_desktop_state.json");
        let state_path = state_path.to_string_lossy().into_owned();
        let first_config = config_with_state_path(&state_path, "cw_01", "http://localhost:8000");
        let second_config = config_with_state_path(&state_path, "cw_02", "http://localhost:9000");

        let mut first = BridgeInstanceLock::new(&first_config);
        first
            .acquire(&first_config)
            .expect("first bridge should acquire the lock");

        let mut second = BridgeInstanceLock::new(&second_config);
        let error = second
            .acquire(&second_config)
            .expect_err("second bridge should be rejected while the first lock is held");
        assert!(error.to_string().contains("already running"));

        first.release();
        second
            .acquire(&second_config)
            .expect("second bridge should acquire the lock after release");

        let _ = std::fs::remove_dir_all(dir);
    }

    fn config_with_state_path(state_path: &str, coworker_id: &str, base_url: &str) -> BridgeConfig {
        BridgeConfig {
            codex_id: "codex-local".into(),
            display_name: "Local Codex".into(),
            coworkers: vec![BridgeCoworker {
                coworker_id: coworker_id.into(),
                display_name: "Coworker".into(),
                base_url: base_url.into(),
            }],
            command: "codex".into(),
            args: vec!["app-server".into()],
            snapshot_thread_limit: 20,
            snapshot_scan_thread_limit: 200,
            snapshot_interval_seconds: 300,
            reconnect_seconds: 5,
            state_path: Some(state_path.into()),
            codex_home_dir: "data/codex_home".into(),
            session_overlay_dir: "data/coworker_desktop_sessions".into(),
            service_name: "coworker_desktop".into(),
            snapshot_source_kinds: vec!["cli".into()],
            permissions_mode: "read-only".into(),
            approvals_reviewer: "none".into(),
            approval_timeout_seconds: 300,
            auto_continue_interrupted_turns: true,
            auto_continue_interrupted_max_attempts: 3,
            auto_continue_interrupted_message: "continue".into(),
            attachment_store_dir: "data/coworker_desktop_attachments".into(),
            attachment_max_bytes: 20 * 1024 * 1024,
            attachment_max_count: 5,
            logs_dir: "data/logs".into(),
            log_level: "INFO".into(),
            file_log_level: "DEBUG".into(),
            chat_workspaces_dir: "data/codex_chats".into(),
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "coworker-desktop-{name}-{}-{nonce}",
            std::process::id()
        ))
    }
}
