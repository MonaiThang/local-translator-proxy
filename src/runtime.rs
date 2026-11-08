//! Model Runtime HTTP client.
//!
//! Sends one physical line per non-streaming Ollama-compatible `/api/chat`
//! request, appends the language directive to the configured system message,
//! and parses the structured `translation` field. Transport and schema failures
//! become [`RuntimeError`] for the pipeline to turn into a passthrough.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{ModelRuntimeConfig, PromptConfig};

/// HTTP client for the separately installed Model Runtime.
///
/// The wire shape is the Ollama chat-completions surface: a non-streaming
/// `POST /api/chat` with a system + user message and deterministic options.
#[derive(Clone)]
pub struct RuntimeClient {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    seed: i64,
    system_message: String,
}

/// Failure modes that make the pipeline return the original unit.
#[derive(Debug)]
pub enum RuntimeError {
    /// Connection refused, DNS failure, or another transport error.
    Unreachable,
    /// The request exceeded the configured HTTP timeout.
    Timeout,
    /// The runtime returned a non-2xx status.
    HttpStatus(u16),
    /// The response body was not valid JSON.
    Decode,
    /// The JSON did not match the expected `translation` envelope.
    Schema,
    /// The structured translation was missing or whitespace-only.
    EmptyTranslation,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable => write!(formatter, "model runtime unreachable"),
            Self::Timeout => write!(formatter, "model runtime timed out"),
            Self::HttpStatus(status) => write!(formatter, "model runtime returned HTTP {status}"),
            Self::Decode => write!(formatter, "model runtime response was not valid JSON"),
            Self::Schema => write!(formatter, "model runtime response did not match the schema"),
            Self::EmptyTranslation => {
                write!(formatter, "model runtime returned an empty translation")
            }
        }
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage; 2],
    stream: bool,
    options: ChatOptions,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatOptions {
    temperature: u8,
    seed: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationOutput {
    translation: String,
}

impl RuntimeClient {
    pub fn new(config: &ModelRuntimeConfig, prompt: &PromptConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;

        Ok(Self {
            client,
            endpoint: config.endpoint.clone(),
            model: config.model.clone(),
            seed: config.seed,
            system_message: prompt.system_message.clone(),
        })
    }

    /// Translate one physical line.
    ///
    /// The caller owns the Translation Unit deadline; this client additionally
    /// applies the configured HTTP timeout.
    pub async fn translate_line(
        &self,
        line: &str,
        from: &str,
        to: &str,
    ) -> Result<String, RuntimeError> {
        let request = ChatRequest {
            model: &self.model,
            messages: [
                ChatMessage {
                    role: "system",
                    content: self.system_message(from, to),
                },
                ChatMessage {
                    role: "user",
                    content: line.to_string(),
                },
            ],
            stream: false,
            options: ChatOptions {
                temperature: 0,
                seed: self.seed,
            },
        };

        let response = self
            .client
            .post(&self.endpoint)
            .json(&request)
            .send()
            .await
            .map_err(map_network_error)?;

        let status = response.status();
        if !status.is_success() {
            return Err(RuntimeError::HttpStatus(status.as_u16()));
        }

        let value: Value = response.json().await.map_err(|error| {
            if error.is_timeout() {
                RuntimeError::Timeout
            } else {
                RuntimeError::Decode
            }
        })?;

        parse_response(&value)
    }

    fn system_message(&self, from: &str, to: &str) -> String {
        format!(
            "{}\nSource language: {from}\nTarget language: {to}",
            self.system_message.trim_end()
        )
    }
}

/// Map a `reqwest` transport error onto the pipeline's failure vocabulary.
fn map_network_error(error: reqwest::Error) -> RuntimeError {
    if error.is_timeout() {
        RuntimeError::Timeout
    } else {
        RuntimeError::Unreachable
    }
}

/// Accept either a top-level `translation` field or Ollama's assistant message
/// content containing the same structured JSON.
fn parse_response(value: &Value) -> Result<String, RuntimeError> {
    // Runtime-agnostic shape: the body itself carries the structured field.
    if value.get("translation").is_some() {
        return parse_translation(&value.to_string());
    }

    // Reference target (Ollama): the structured field is inside the assistant
    // message content.
    let content = value
        .pointer("/message/content")
        .and_then(Value::as_str)
        .ok_or(RuntimeError::Schema)?;
    parse_translation(normalize_assistant_content(content))
}

/// Deserialize the strict structured payload and reject an empty translation.
fn parse_translation(content: &str) -> Result<String, RuntimeError> {
    let output: TranslationOutput =
        serde_json::from_str(content).map_err(|_| RuntimeError::Schema)?;
    non_empty(&output.translation)
}

/// Normalize assistant content before JSON parsing.
///
/// Models often wrap the structured payload in a Markdown fence even when told
/// not to, and they may add surrounding whitespace. One fence (three or more
/// backticks, optionally tagged `json`) that wraps the whole payload is
/// removed. Prose outside the fence is left in place and fails schema
/// validation.
fn normalize_assistant_content(content: &str) -> &str {
    let trimmed = content.trim();
    let opening = trimmed.len() - trimmed.trim_start_matches('`').len();
    if opening < 3 {
        return trimmed;
    }
    let after_opening = &trimmed[opening..];
    let closing = after_opening.len() - after_opening.trim_end_matches('`').len();
    if closing < opening {
        return trimmed;
    }
    let inner = after_opening[..after_opening.len() - closing].trim();
    if inner
        .get(..4)
        .is_some_and(|tag| tag.eq_ignore_ascii_case("json"))
    {
        inner[4..].trim()
    } else {
        inner
    }
}

/// Reject an absent or whitespace-only translation.
fn non_empty(translation: &str) -> Result<String, RuntimeError> {
    if translation.trim().is_empty() {
        Err(RuntimeError::EmptyTranslation)
    } else {
        Ok(translation.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_top_level_translation() {
        let value = json!({ "translation": "Hello" });
        assert_eq!(parse_response(&value).expect("translation"), "Hello");
    }

    #[test]
    fn parses_ollama_message_content() {
        let value = json!({
            "message": { "role": "assistant", "content": "{\"translation\":\"Hello\"}" }
        });
        assert_eq!(parse_response(&value).expect("translation"), "Hello");
    }

    #[test]
    fn parses_fenced_ollama_message_content() {
        let value = json!({
            "message": { "content": "```json\n{\"translation\":\"Hello\"}\n```" }
        });
        assert_eq!(parse_response(&value).expect("translation"), "Hello");
    }

    #[test]
    fn parses_bare_fenced_ollama_message_content() {
        let value = json!({
            "message": { "content": "```\n{\"translation\":\"Hello\"}\n```" }
        });
        assert_eq!(parse_response(&value).expect("translation"), "Hello");
    }

    #[test]
    fn parses_same_line_fenced_ollama_message_content() {
        let value = json!({
            "message": { "content": "```json {\"translation\":\"Hello\"} ```" }
        });
        assert_eq!(parse_response(&value).expect("translation"), "Hello");
    }

    #[test]
    fn parses_fence_with_a_spaced_json_tag() {
        let value = json!({
            "message": { "content": "``` json\n{\"translation\":\"Hello\"}\n```" }
        });
        assert_eq!(parse_response(&value).expect("translation"), "Hello");
    }

    #[test]
    fn parses_four_backtick_fence() {
        let value = json!({
            "message": { "content": "````json\n{\"translation\":\"Hello\"}\n````" }
        });
        assert_eq!(parse_response(&value).expect("translation"), "Hello");
    }

    #[test]
    fn rejects_prose_wrapped_fenced_message_content() {
        let value = json!({
            "message": {
                "content": "Here is the translation:\n```json\n{\"translation\":\"Hello\"}\n```"
            }
        });
        assert!(matches!(parse_response(&value), Err(RuntimeError::Schema)));
    }

    #[test]
    fn rejects_extra_fields_in_top_level_translation() {
        let value = json!({ "translation": "Hello", "notes": "extra" });
        assert!(matches!(parse_response(&value), Err(RuntimeError::Schema)));
    }

    #[test]
    fn rejects_extra_fields_in_structured_output() {
        let value = json!({
            "message": { "content": "{\"translation\":\"Hello\",\"notes\":\"extra\"}" }
        });
        assert!(matches!(parse_response(&value), Err(RuntimeError::Schema)));
    }

    #[test]
    fn rejects_empty_translation() {
        let value = json!({ "translation": "   " });
        assert!(matches!(
            parse_response(&value),
            Err(RuntimeError::EmptyTranslation)
        ));
    }

    #[test]
    fn rejects_missing_fields() {
        assert!(matches!(
            parse_response(&json!({ "message": { "role": "assistant" } })),
            Err(RuntimeError::Schema)
        ));
        assert!(matches!(
            parse_response(&json!({ "message": { "content": "not json" } })),
            Err(RuntimeError::Schema)
        ));
    }

    #[test]
    fn appends_language_directive_to_system_message() {
        let client = RuntimeClient {
            client: reqwest::Client::new(),
            endpoint: "http://127.0.0.1:1/api/chat".to_string(),
            model: "test".to_string(),
            seed: 1,
            system_message: "Translate.\n".to_string(),
        };
        assert_eq!(
            client.system_message("zh-cn", "en"),
            "Translate.\nSource language: zh-cn\nTarget language: en"
        );
    }
}
