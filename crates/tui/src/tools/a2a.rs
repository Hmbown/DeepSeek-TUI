//! Agent-to-Agent (A2A) protocol adapter.
//!
//! Defines the message format and adapter for inter-agent communication.
//! Agents exchange structured messages in a request/response pattern
//! with optional streaming.
//!
//! # Message format
//!
//! Each A2A message carries:
//! - `sender_id`: the sending agent's identifier
//! - `recipient_id`: the target agent (or broadcast)
//! - `kind`: message kind (Request, Response, Broadcast, Heartbeat)
//! - `payload`: arbitrary JSON payload
//! - `correlation_id`: links request/response pairs

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

// ── Message kind ─────────────────────────────────────────────────────────────

/// Kind of A2A message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum A2aMessageKind {
    /// A request expecting a response.
    Request,
    /// A response to a prior request.
    Response,
    /// A broadcast to all agents (no response expected).
    Broadcast,
    /// Periodic heartbeat / liveness check.
    Heartbeat,
}

impl A2aMessageKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Response => "response",
            Self::Broadcast => "broadcast",
            Self::Heartbeat => "heartbeat",
        }
    }
}

// ── Message ──────────────────────────────────────────────────────────────────

/// A single A2A protocol message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aMessage {
    /// Sender agent id.
    pub sender_id: String,
    /// Recipient agent id. `None` for broadcasts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient_id: Option<String>,
    /// Message kind.
    pub kind: A2aMessageKind,
    /// Arbitrary JSON payload.
    pub payload: JsonValue,
    /// Links request/response pairs. Required for Request and Response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Monotonic sequence number from the sender.
    pub sequence: u64,
    /// Unix timestamp in milliseconds when the message was created.
    pub timestamp_ms: u64,
}

impl A2aMessage {
    #[must_use]
    pub fn new(
        sender_id: impl Into<String>,
        kind: A2aMessageKind,
        payload: JsonValue,
        sequence: u64,
    ) -> Self {
        Self {
            sender_id: sender_id.into(),
            recipient_id: None,
            kind,
            payload,
            correlation_id: None,
            sequence,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }

    /// Set the recipient.
    pub fn with_recipient(mut self, recipient_id: impl Into<String>) -> Self {
        self.recipient_id = Some(recipient_id.into());
        self
    }

    /// Link to a prior message via correlation_id.
    pub fn with_correlation(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    /// Create a response to a request message.
    #[must_use]
    pub fn response_to(
        request: &Self,
        sender_id: impl Into<String>,
        payload: JsonValue,
        sequence: u64,
    ) -> Self {
        A2aMessage::new(sender_id, A2aMessageKind::Response, payload, sequence)
            .with_recipient(request.sender_id.clone())
            .with_correlation(
                request
                    .correlation_id
                    .clone()
                    .unwrap_or_else(|| format!("{}", request.sequence)),
            )
    }

    /// Create a heartbeat message.
    #[must_use]
    pub fn heartbeat(sender_id: impl Into<String>, sequence: u64) -> Self {
        A2aMessage::new(
            sender_id,
            A2aMessageKind::Heartbeat,
            JsonValue::Null,
            sequence,
        )
    }

    /// Create a broadcast message.
    #[must_use]
    pub fn broadcast(sender_id: impl Into<String>, payload: JsonValue, sequence: u64) -> Self {
        let mut msg = A2aMessage::new(sender_id, A2aMessageKind::Broadcast, payload, sequence);
        msg.recipient_id = None;
        msg
    }
}

// ── Adapter ──────────────────────────────────────────────────────────────────

/// A2A protocol adapter — manages message sequencing and correlation.
#[derive(Debug, Default)]
pub struct A2aAdapter {
    /// Monotonic sequence counter for outgoing messages.
    next_sequence: u64,
    /// Pending correlation ids awaiting responses.
    pending: Vec<String>,
}

impl A2aAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new request message with auto-incremented sequence.
    pub fn request(
        &mut self,
        sender_id: impl Into<String>,
        recipient_id: impl Into<String>,
        payload: JsonValue,
    ) -> A2aMessage {
        let seq = self.next_sequence;
        self.next_sequence += 1;
        let correlation_id = format!("corr_{seq}");
        self.pending.push(correlation_id.clone());

        A2aMessage::new(sender_id, A2aMessageKind::Request, payload, seq)
            .with_recipient(recipient_id)
            .with_correlation(correlation_id)
    }

    /// Create a broadcast message.
    #[must_use]
    pub fn broadcast(&mut self, sender_id: impl Into<String>, payload: JsonValue) -> A2aMessage {
        let seq = self.next_sequence;
        self.next_sequence += 1;
        A2aMessage::broadcast(sender_id, payload, seq)
    }

    /// Acknowledge a received response, removing it from pending.
    pub fn ack_response(&mut self, correlation_id: &str) -> bool {
        if let Some(pos) = self.pending.iter().position(|c| c == correlation_id) {
            self.pending.remove(pos);
            true
        } else {
            false
        }
    }

    /// Number of pending (unacknowledged) requests.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Current sequence number.
    #[must_use]
    pub fn sequence(&self) -> u64 {
        self.next_sequence
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = A2aMessage::new(
            "agent_a",
            A2aMessageKind::Request,
            serde_json::json!({"task": "analyze"}),
            1,
        )
        .with_recipient("agent_b")
        .with_correlation("corr_1");

        assert_eq!(msg.sender_id, "agent_a");
        assert_eq!(msg.recipient_id, Some("agent_b".to_string()));
        assert_eq!(msg.kind, A2aMessageKind::Request);
        assert_eq!(msg.sequence, 1);
        assert_eq!(msg.correlation_id, Some("corr_1".to_string()));
    }

    #[test]
    fn test_response_to() {
        let request = A2aMessage::new(
            "agent_a",
            A2aMessageKind::Request,
            serde_json::json!({"q": "status?"}),
            5,
        )
        .with_correlation("corr_5");

        let response =
            A2aMessage::response_to(&request, "agent_b", serde_json::json!({"status": "ok"}), 1);

        assert_eq!(response.kind, A2aMessageKind::Response);
        assert_eq!(response.recipient_id, Some("agent_a".to_string()));
        assert_eq!(response.correlation_id, Some("corr_5".to_string()));
    }

    #[test]
    fn test_adapter_request_sequencing() {
        let mut adapter = A2aAdapter::new();
        let req1 = adapter.request("a", "b", JsonValue::Null);
        let req2 = adapter.request("a", "b", JsonValue::Null);

        assert_eq!(req1.sequence, 0);
        assert_eq!(req2.sequence, 1);
        assert_eq!(adapter.pending_count(), 2);
    }

    #[test]
    fn test_adapter_ack_response() {
        let mut adapter = A2aAdapter::new();
        let req = adapter.request("a", "b", JsonValue::Null);
        let corr_id = req.correlation_id.unwrap();

        assert_eq!(adapter.pending_count(), 1);
        assert!(adapter.ack_response(&corr_id));
        assert_eq!(adapter.pending_count(), 0);
        assert!(!adapter.ack_response("nonexistent"));
    }

    #[test]
    fn test_broadcast_has_no_recipient() {
        let mut adapter = A2aAdapter::new();
        let msg = adapter.broadcast("agent_a", serde_json::json!({"alert": "done"}));
        assert_eq!(msg.kind, A2aMessageKind::Broadcast);
        assert!(msg.recipient_id.is_none());
    }
}
