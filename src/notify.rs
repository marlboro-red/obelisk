use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{debug, warn};

fn notifications_suppressed() -> bool {
    cfg!(test)
}

/// Send a desktop notification (toast/banner). Fails silently if the OS
/// does not support it or the notification daemon is unavailable.
pub fn send_notification(title: &str, body: &str) {
    if notifications_suppressed() {
        return;
    }

    let _ = notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .show();
}

/// Write a terminal bell character (BEL, \x07) to stderr as a lightweight
/// audio fallback for terminals that support it.
pub fn send_bell() {
    if notifications_suppressed() {
        return;
    }

    use std::io::Write;
    let _ = std::io::stderr().write_all(b"\x07");
    let _ = std::io::stderr().flush();
}

// ── Webhook notifications ───────────────────────────────────────

/// Event types that can trigger a webhook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEventType {
    AgentCompleted,
    AgentFailed,
    HighPriorityReady,
}

impl WebhookEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AgentCompleted => "agent_completed",
            Self::AgentFailed => "agent_failed",
            Self::HighPriorityReady => "high_priority_ready",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "agent_completed" => Some(Self::AgentCompleted),
            "agent_failed" => Some(Self::AgentFailed),
            "high_priority_ready" => Some(Self::HighPriorityReady),
            _ => None,
        }
    }

    pub const ALL: &[Self] = &[
        Self::AgentCompleted,
        Self::AgentFailed,
        Self::HighPriorityReady,
    ];
}

/// Webhook configuration loaded from `[notifications.webhook]` in obelisk.toml.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WebhookConfig {
    pub url: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub events: Vec<String>,
}

impl WebhookConfig {
    /// Whether a webhook URL is configured.
    pub fn is_enabled(&self) -> bool {
        self.url.as_ref().is_some_and(|u| !u.is_empty())
    }

    /// Check if a specific event type is subscribed.
    pub fn is_subscribed(&self, event: WebhookEventType) -> bool {
        if self.events.is_empty() {
            return true; // empty = all events
        }
        self.events
            .iter()
            .any(|s| WebhookEventType::from_str(s) == Some(event))
    }

    /// Validate the webhook config and return warnings.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if let Some(url) = &self.url {
            if !url.is_empty() && !url.starts_with("http://") && !url.starts_with("https://") {
                warnings.push(format!(
                    "Webhook URL '{}' does not start with http:// or https://",
                    url
                ));
            }
        }
        for event_str in &self.events {
            if WebhookEventType::from_str(event_str).is_none() {
                warnings.push(format!(
                    "Unknown webhook event type '{}' (valid: {})",
                    event_str,
                    WebhookEventType::ALL
                        .iter()
                        .map(|e| e.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        warnings
    }
}

/// Webhook payload sent for agent events.
#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload {
    pub event: String,
    pub issue_id: String,
    pub title: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_details: Option<String>,
    pub timestamp: String,
}

/// Fire a webhook POST request in the background. Non-blocking — failures
/// are logged and surfaced to the TUI via `failure_tx`.
pub fn send_webhook(
    config: &WebhookConfig,
    event: WebhookEventType,
    payload: WebhookPayload,
    failure_tx: &mpsc::UnboundedSender<String>,
) {
    if notifications_suppressed() {
        return;
    }
    if !config.is_enabled() || !config.is_subscribed(event) {
        return;
    }

    let url = match config.url.clone() {
        Some(u) => u,
        None => return,
    };
    let headers = config.headers.clone();
    let event_str = event.as_str().to_string();
    let tx = failure_tx.clone();

    debug!(event = %event_str, url = %url, "sending webhook");

    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut req = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "obelisk-orchestrator");

        for (key, value) in &headers {
            req = req.header(key.as_str(), value.as_str());
        }

        match req.json(&payload).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    debug!(
                        event = %event_str,
                        status = %resp.status(),
                        "webhook delivered"
                    );
                } else {
                    let msg = format!(
                        "Webhook failed for {}: HTTP {}",
                        event_str,
                        resp.status()
                    );
                    warn!(
                        event = %event_str,
                        status = %resp.status(),
                        "webhook returned non-success status"
                    );
                    let _ = tx.send(msg);
                }
            }
            Err(e) => {
                let msg = format!("Webhook failed for {}: {}", event_str, e);
                warn!(
                    event = %event_str,
                    error = %e,
                    "webhook delivery failed"
                );
                let _ = tx.send(msg);
            }
        }
    });
}

/// Notifications config section from obelisk.toml.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct NotificationsConfig {
    pub webhook: Option<WebhookConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suppresses_notifications_in_test_builds() {
        assert!(notifications_suppressed());
    }

    #[test]
    fn webhook_config_default_is_disabled() {
        let config = WebhookConfig::default();
        assert!(!config.is_enabled());
    }

    #[test]
    fn webhook_config_with_url_is_enabled() {
        let config = WebhookConfig {
            url: Some("https://example.com/hook".into()),
            ..Default::default()
        };
        assert!(config.is_enabled());
    }

    #[test]
    fn webhook_config_empty_url_is_disabled() {
        let config = WebhookConfig {
            url: Some(String::new()),
            ..Default::default()
        };
        assert!(!config.is_enabled());
    }

    #[test]
    fn empty_events_subscribes_to_all() {
        let config = WebhookConfig {
            url: Some("https://example.com".into()),
            events: vec![],
            ..Default::default()
        };
        assert!(config.is_subscribed(WebhookEventType::AgentCompleted));
        assert!(config.is_subscribed(WebhookEventType::AgentFailed));
        assert!(config.is_subscribed(WebhookEventType::HighPriorityReady));
    }

    #[test]
    fn selective_event_subscription() {
        let config = WebhookConfig {
            url: Some("https://example.com".into()),
            events: vec!["agent_failed".into()],
            ..Default::default()
        };
        assert!(!config.is_subscribed(WebhookEventType::AgentCompleted));
        assert!(config.is_subscribed(WebhookEventType::AgentFailed));
        assert!(!config.is_subscribed(WebhookEventType::HighPriorityReady));
    }

    #[test]
    fn event_type_round_trip() {
        for event in WebhookEventType::ALL {
            let s = event.as_str();
            let parsed = WebhookEventType::from_str(s);
            assert_eq!(parsed, Some(*event));
        }
    }

    #[test]
    fn unknown_event_type_returns_none() {
        assert_eq!(WebhookEventType::from_str("bogus"), None);
    }

    #[test]
    fn validate_warns_on_bad_url() {
        let config = WebhookConfig {
            url: Some("ftp://example.com".into()),
            ..Default::default()
        };
        let warnings = config.validate();
        assert!(warnings.iter().any(|w| w.contains("does not start with http")));
    }

    #[test]
    fn validate_warns_on_unknown_event() {
        let config = WebhookConfig {
            url: Some("https://example.com".into()),
            events: vec!["bogus_event".into()],
            ..Default::default()
        };
        let warnings = config.validate();
        assert!(warnings.iter().any(|w| w.contains("Unknown webhook event type")));
    }

    #[test]
    fn validate_no_warnings_for_valid_config() {
        let config = WebhookConfig {
            url: Some("https://hooks.slack.com/services/xxx".into()),
            headers: HashMap::from([("Authorization".into(), "Bearer tok".into())]),
            events: vec!["agent_completed".into(), "agent_failed".into()],
        };
        let warnings = config.validate();
        assert!(warnings.is_empty(), "Unexpected warnings: {:?}", warnings);
    }

    #[test]
    fn webhook_payload_serializes_correctly() {
        let payload = WebhookPayload {
            event: "agent_completed".into(),
            issue_id: "obelisk-abc".into(),
            title: "Fix bug".into(),
            status: "completed".into(),
            runtime: Some("claude".into()),
            elapsed_secs: Some(120),
            exit_code: None,
            failure_details: None,
            timestamp: "2026-03-14T00:00:00Z".into(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["event"], "agent_completed");
        assert_eq!(json["issue_id"], "obelisk-abc");
        assert!(json.get("exit_code").is_none()); // skip_serializing_if
        assert!(json.get("failure_details").is_none());
    }

    #[test]
    fn webhook_payload_includes_failure_details() {
        let payload = WebhookPayload {
            event: "agent_failed".into(),
            issue_id: "obelisk-def".into(),
            title: "Deploy service".into(),
            status: "failed".into(),
            runtime: Some("codex".into()),
            elapsed_secs: Some(45),
            exit_code: Some(1),
            failure_details: Some("3 compilation errors".into()),
            timestamp: "2026-03-14T01:00:00Z".into(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["exit_code"], 1);
        assert_eq!(json["failure_details"], "3 compilation errors");
    }

    #[test]
    fn webhook_config_round_trip_toml() {
        let config = NotificationsConfig {
            webhook: Some(WebhookConfig {
                url: Some("https://example.com/hook".into()),
                headers: HashMap::from([("X-Token".into(), "secret".into())]),
                events: vec!["agent_completed".into(), "agent_failed".into()],
            }),
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let restored: NotificationsConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, restored);
    }
}
