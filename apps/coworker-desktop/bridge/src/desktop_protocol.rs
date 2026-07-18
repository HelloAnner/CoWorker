use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{BridgeError, Result};

pub const DESKTOP_PROTOCOL_VERSION: u32 = 1;
pub const DESKTOP_REGISTRATION_KIND: &str = "coworker-desktop";
pub const REQUIRED_COWORKER_SKILL: &str = "coworker-desktop";

/// Typed desktop envelope event types -- the single source of truth for the
/// `type` wire field, replacing scattered string literals across the bridge.
///
/// Each variant serializes to its existing wire string via `#[serde(rename)]`,
/// so this change is wire-compatible. The Codex bridge forwards event types it
/// reads from session payloads; `from_wire_str` parses those at the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DesktopEventType {
    #[serde(rename = "desktop.actor.snapshot")]
    ActorSnapshot,
    #[serde(rename = "desktop.thread.event")]
    ThreadEvent,
    #[serde(rename = "desktop.command.result")]
    CommandResult,
    #[serde(rename = "desktop.approval.requested")]
    ApprovalRequested,
    #[serde(rename = "desktop.user_input.requested")]
    UserInputRequested,
    #[serde(rename = "desktop.server_request.resolved")]
    ServerRequestResolved,
    #[serde(rename = "desktop.error")]
    Error,
    #[serde(rename = "desktop.command")]
    Command,
}

impl DesktopEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ActorSnapshot => "desktop.actor.snapshot",
            Self::ThreadEvent => "desktop.thread.event",
            Self::CommandResult => "desktop.command.result",
            Self::ApprovalRequested => "desktop.approval.requested",
            Self::UserInputRequested => "desktop.user_input.requested",
            Self::ServerRequestResolved => "desktop.server_request.resolved",
            Self::Error => "desktop.error",
            Self::Command => "desktop.command",
        }
    }

    /// Parse a wire `type` string into the typed variant. Returns `None` for
    /// unknown strings so callers (notably the Codex bridge's dynamic event
    /// forwarding) can decide how to handle an unrecognized type.
    pub fn from_wire_str(value: &str) -> Option<Self> {
        Some(match value {
            "desktop.actor.snapshot" => Self::ActorSnapshot,
            "desktop.thread.event" => Self::ThreadEvent,
            "desktop.command.result" => Self::CommandResult,
            "desktop.approval.requested" => Self::ApprovalRequested,
            "desktop.user_input.requested" => Self::UserInputRequested,
            "desktop.server_request.resolved" => Self::ServerRequestResolved,
            "desktop.error" => Self::Error,
            "desktop.command" => Self::Command,
            _ => return None,
        })
    }
}

impl std::fmt::Display for DesktopEventType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorId {
    Local,
    Codex,
    Claude,
}

impl ActorId {
    pub const ALL: [Self; 3] = [Self::Local, Self::Codex, Self::Claude];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

impl std::fmt::Display for ActorId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationRef {
    pub actor_id: ActorId,
    pub conversation_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesktopEnvelopeV1 {
    pub protocol_version: u32,
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(rename = "type")]
    pub event_type: DesktopEventType,
    pub payload: Value,
}

impl DesktopEnvelopeV1 {
    pub fn new(event_type: DesktopEventType, payload: Value) -> Self {
        Self {
            protocol_version: DESKTOP_PROTOCOL_VERSION,
            message_id: Uuid::new_v4().to_string(),
            request_id: None,
            conversation_id: None,
            created_at: Utc::now(),
            event_type,
            payload,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.protocol_version != DESKTOP_PROTOCOL_VERSION {
            return Err(BridgeError::message(format!(
                "unsupported desktop protocol version: {}",
                self.protocol_version
            )));
        }
        if Uuid::parse_str(&self.message_id).is_err() {
            return Err(BridgeError::message("desktop message_id must be a UUID"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryAck {
    pub message_id: String,
    pub accepted: bool,
    pub duplicate: bool,
}

pub fn desktop_client_id(desktop_id: &str, actor: ActorId, coworker_id: &str) -> String {
    format!("{desktop_id}:{}:{coworker_id}", actor.as_str())
}

pub fn actor_model_message(
    author_kind: &str,
    author_id: Option<&str>,
    author_label: Option<&str>,
    content: &str,
) -> Result<String> {
    if author_kind != "coworker" {
        return Ok(content.to_owned());
    }
    let coworker_id = author_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| BridgeError::message("coworker message requires author_id"))?;
    let author_label = author_label
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("搭档");
    Ok(format!(
        "[来自Coworker:{coworker_id}][{author_label}]的消息:\n{content}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_round_trips_and_validates() {
        let envelope = DesktopEnvelopeV1::new(DesktopEventType::ActorSnapshot, json!({"ok": true}));
        envelope.validate().expect("valid envelope");
        let decoded: DesktopEnvelopeV1 =
            serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
        assert_eq!(decoded, envelope);
        assert_eq!(envelope.event_type, DesktopEventType::ActorSnapshot);
        assert_eq!(
            serde_json::to_string(&envelope.event_type).unwrap(),
            "\"desktop.actor.snapshot\""
        );
    }

    #[test]
    fn client_ids_are_actor_scoped() {
        assert_eq!(
            desktop_client_id("desk", ActorId::Claude, "cw_1"),
            "desk:claude:cw_1"
        );
    }

    #[test]
    fn coworker_model_message_has_routing_header() {
        assert_eq!(
            actor_model_message("coworker", Some("cw_01"), Some("搭档 A"), "继续").unwrap(),
            "[来自Coworker:cw_01][搭档 A]的消息:\n继续"
        );
        assert_eq!(
            actor_model_message("local", None, None, "继续").unwrap(),
            "继续"
        );
    }
}
