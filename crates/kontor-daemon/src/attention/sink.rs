//! The one outbound webhook and its isolated delivery task.
//!
//! Delivery is a detached one-shot per alert (CON-006): spawned, bounded by the
//! configured timeout and attempt count, and never awaited by the command path.
//! A failure is logged with the receipt id and discarded (GUD-001); nothing here
//! can roll back or delay the receipt it observed.
//!
//! The payload stays channel-neutral. This module is the edge where a
//! destination's formatting happens — the Teams Workflows document and its
//! required `@mention` live here and nowhere else.

use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::attention::config::{AttentionPolicy, MentionPolicy, SinkPolicy};
use crate::attention::payload::AttentionPayload;

/// Why one delivery attempt did not succeed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SinkError {
    /// The HTTP client could not be built.
    #[error("the attention HTTP client could not be constructed")]
    Client,
    /// The request did not complete inside the configured timeout.
    #[error("the attention delivery timed out")]
    Timeout,
    /// The transport failed before a response was received.
    #[error("the attention delivery transport failed")]
    Transport,
    /// The destination answered a non-success status.
    #[error("the attention destination refused the delivery with {status}")]
    Status {
        /// The HTTP status code.
        status: u16,
    },
    /// Every bounded attempt failed.
    #[error("the attention delivery exhausted its attempts")]
    Exhausted,
}

/// One configured Teams Workflows destination.
#[derive(Debug)]
pub struct AttentionSink {
    client: reqwest::Client,
    sink: SinkPolicy,
    mention: MentionPolicy,
}

impl AttentionSink {
    /// Compose the configured destination.
    ///
    /// # Errors
    /// Returns [`SinkError::Client`] when the HTTP client cannot be built.
    pub fn from_policy(policy: &AttentionPolicy) -> Result<Self, SinkError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(policy.sink.timeout_seconds))
            .build()
            .map_err(|_| SinkError::Client)?;
        Ok(Self {
            client,
            sink: policy.sink.clone(),
            mention: policy.mention.clone(),
        })
    }

    /// Spawn exactly one detached delivery task for one alert.
    ///
    /// The trigger is never delayed and never observes the outcome. A runtime
    /// without a Tokio reactor logs and drops the alert rather than turning a
    /// command into a failure.
    pub fn dispatch(self: &Arc<Self>, payload: AttentionPayload) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            warn!(
                kind = payload.kind.as_str(),
                receipt = ?payload.receipt_id,
                "attention delivery was not dispatched because no async runtime is present"
            );
            return;
        };
        let sink = Arc::clone(self);
        handle.spawn(async move {
            match sink.deliver_once(&payload).await {
                Ok(attempts) => info!(
                    kind = payload.kind.as_str(),
                    receipt = ?payload.receipt_id,
                    attempts,
                    "attention delivered"
                ),
                Err(error) => warn!(
                    kind = payload.kind.as_str(),
                    receipt = ?payload.receipt_id,
                    detail = %error,
                    "attention delivery failed and was discarded"
                ),
            }
        });
    }

    /// Deliver one alert, retrying inside this call.
    ///
    /// This is the bounded task body. It is public so a contract test can await
    /// the same delivery the detached task performs.
    ///
    /// # Errors
    /// Returns the final [`SinkError`] after every configured attempt failed.
    pub async fn deliver_once(&self, payload: &AttentionPayload) -> Result<u32, SinkError> {
        let document = teams_document(payload, &self.mention.target);
        let mut last = SinkError::Exhausted;
        for attempt in 1..=self.sink.max_attempts {
            match self
                .client
                .post(&self.sink.url)
                .json(&document)
                .timeout(Duration::from_secs(self.sink.timeout_seconds))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => return Ok(attempt),
                Ok(response) => {
                    last = SinkError::Status {
                        status: response.status().as_u16(),
                    };
                    warn!(
                        kind = payload.kind.as_str(),
                        receipt = ?payload.receipt_id,
                        attempt,
                        status = response.status().as_u16(),
                        "attention destination refused one attempt"
                    );
                }
                Err(error) if error.is_timeout() => {
                    last = SinkError::Timeout;
                    warn!(
                        kind = payload.kind.as_str(),
                        receipt = ?payload.receipt_id,
                        attempt,
                        "attention delivery attempt timed out"
                    );
                }
                Err(_) => {
                    last = SinkError::Transport;
                    warn!(
                        kind = payload.kind.as_str(),
                        receipt = ?payload.receipt_id,
                        attempt,
                        "attention delivery attempt could not reach the destination"
                    );
                }
            }
            if attempt < self.sink.max_attempts {
                tokio::time::sleep(retry_delay(attempt)).await;
            }
        }
        Err(last)
    }
}

/// Jittered backoff without a new random dependency.
fn retry_delay(attempt: u32) -> Duration {
    let base = 100_u64.saturating_mul(u64::from(attempt));
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    Duration::from_millis(base + u64::from(nanos % 100))
}

/// Render the destination document.
///
/// The only channel-specific words in the whole channel live in this function:
/// the Adaptive Card shape and the `<at>` mention entity. The payload supplies
/// no markup and no mention syntax.
#[must_use]
pub fn teams_document(payload: &AttentionPayload, mention: &str) -> serde_json::Value {
    let mention_text = format!("<at>{mention}</at>");
    let mut facts = vec![
        fact("Realm", &payload.realm_id.to_string()),
        fact("Project", &payload.project_id.to_string()),
        fact("State", payload.resulting_state.as_str()),
        fact("Kind", payload.kind.as_str()),
        fact("Occurred", &payload.occurred_at.to_string()),
    ];
    if let Some(epic) = payload.mini_project_id {
        facts.push(fact("Epic", &epic.to_string()));
    }
    if let Some(task) = payload.task_id {
        facts.push(fact("Task", &task.to_string()));
    }
    if let Some(seat) = &payload.seat {
        facts.push(fact("Seat", seat.as_str()));
    }
    if let Some(command) = payload.command_kind {
        facts.push(fact("Command", command.as_str()));
    }
    if let Some(receipt) = payload.receipt_id {
        facts.push(fact("Receipt", &receipt.to_string()));
    }
    serde_json::json!({
        "type": "message",
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "content": {
                "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                "type": "AdaptiveCard",
                "version": "1.4",
                "body": [
                    {"type": "TextBlock", "text": mention_text, "wrap": true},
                    {"type": "TextBlock", "text": payload.title, "weight": "Bolder", "wrap": true},
                    {"type": "TextBlock", "text": payload.body, "wrap": true},
                    {"type": "FactSet", "facts": facts}
                ],
                "actions": [{
                    "type": "Action.OpenUrl",
                    "title": "Open in Kontor",
                    "url": payload.deep_link
                }],
                "msteams": {
                    "entities": [{
                        "type": "mention",
                        "text": mention_text,
                        "mentioned": {"id": mention, "name": mention}
                    }]
                }
            }
        }]
    })
}

fn fact(name: &str, value: &str) -> serde_json::Value {
    serde_json::json!({"title": name, "value": value})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_destination_document_is_the_only_place_markup_appears() {
        let payload = crate::attention::payload::AttentionPayload::new(
            crate::attention::payload::AlertKind::TaskNeedsHuman,
            crate::attention::payload::Severity::Critical,
            kontor_core::id::RealmId::generate(),
            kontor_core::id::ProjectId::generate(),
            None,
            Some(kontor_core::id::TaskId::generate()),
            None,
            None,
            None,
            crate::attention::payload::ResultingState::NeedsHuman,
            kontor_core::id::Timestamp::now(),
            "http://127.0.0.1:7717/v1/projects/x/tasks/y".to_owned(),
            "Task needs a human".to_owned(),
            "The task entered needs_human.".to_owned(),
        )
        .expect("a valid payload");
        let document = teams_document(&payload, "operator@example.com");
        let rendered = document.to_string();
        assert!(rendered.contains("<at>operator@example.com</at>"));
        assert!(
            !serde_json::to_string(&payload)
                .expect("serializes")
                .contains("<at>")
        );
    }
}
