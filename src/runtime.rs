//! Model Runtime HTTP client.
//!
//! Sends one physical line per non-streaming Ollama-compatible `/api/chat`
//! request, appends the language directive to the configured system message,
//! and parses the structured `translation` field. Transport and schema failures
//! become [`RuntimeError`] for the pipeline to turn into a passthrough. A
//! literal `\uXXXX` sequence that the model double-escaped is decoded once
//! before the pipeline verifies the line.

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

        parse_response(&value, line)
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
fn parse_response(value: &Value, source: &str) -> Result<String, RuntimeError> {
    // Runtime-agnostic shape: the body itself carries the structured field.
    if value.get("translation").is_some() {
        return parse_translation(&value.to_string(), source);
    }

    // Reference target (Ollama): the structured field is inside the assistant
    // message content.
    let content = value
        .pointer("/message/content")
        .and_then(Value::as_str)
        .ok_or(RuntimeError::Schema)?;
    parse_translation(normalize_assistant_content(content), source)
}

/// Deserialize the strict structured payload, repair double-escaped unicode,
/// and reject an empty translation.
fn parse_translation(content: &str, source: &str) -> Result<String, RuntimeError> {
    let output: TranslationOutput =
        serde_json::from_str(content).map_err(|_| RuntimeError::Schema)?;
    let translation = decode_literal_escapes(&output.translation, source);
    if translation != output.translation {
        tracing::debug!(
            source = ?source,
            escaped = ?output.translation,
            decoded = ?translation,
            "decoded literal unicode escapes"
        );
    }
    non_empty(&translation)
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

/// Decode one remaining layer of JSON-style `\uXXXX` escapes that the model
/// introduced inside the translation value.
///
/// `serde_json` already decodes the wire layer. A sequence that survives into
/// the translation string was double-escaped by the model. Sequences that also
/// appear literally in the source are left untouched so game text that shows an
/// escape is not rewritten.
fn decode_literal_escapes(translation: &str, source: &str) -> String {
    let bytes = translation.as_bytes();
    let mut decoded = String::with_capacity(translation.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'\\'
            && let Some((end, sequence, code)) = parse_unicode_escape(translation, index)
        {
            if source.contains(sequence) {
                decoded.push_str(sequence);
                index = end;
                continue;
            }

            if let Some((pair_end, pair_sequence, low)) = parse_unicode_escape(translation, end)
                && !source.contains(pair_sequence)
                && (0xD800..=0xDBFF).contains(&code)
                && (0xDC00..=0xDFFF).contains(&low)
                && let Some(character) =
                    char::from_u32(0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00))
            {
                decoded.push(character);
                index = pair_end;
                continue;
            }

            if let Some(character) = char::from_u32(code) {
                decoded.push(character);
                index = end;
                continue;
            }

            // Lone surrogates are not valid Unicode scalars: keep them literal.
            decoded.push_str(sequence);
            index = end;
            continue;
        }

        let character = translation[index..]
            .chars()
            .next()
            .expect("index is within the string");
        decoded.push(character);
        index += character.len_utf8();
    }

    decoded
}

/// Parse a literal `\uXXXX` sequence at `start`, returning its end, text, and
/// code point. Only lowercase `u` is recognised; JSON uses no other form.
fn parse_unicode_escape(text: &str, start: usize) -> Option<(usize, &str, u32)> {
    let end = start + 6;
    let sequence = text.get(start..end)?;
    let mut characters = sequence.chars();
    if characters.next()? != '\\' || characters.next()? != 'u' {
        return None;
    }

    let hex = sequence.get(2..)?;
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }

    let code = u32::from_str_radix(hex, 16).ok()?;
    Some((end, sequence, code))
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
        assert_eq!(parse_response(&value, "").expect("translation"), "Hello");
    }

    #[test]
    fn parses_ollama_message_content() {
        let value = json!({
            "message": { "role": "assistant", "content": "{\"translation\":\"Hello\"}" }
        });
        assert_eq!(parse_response(&value, "").expect("translation"), "Hello");
    }

    #[test]
    fn parses_fenced_ollama_message_content() {
        let value = json!({
            "message": { "content": "```json\n{\"translation\":\"Hello\"}\n```" }
        });
        assert_eq!(parse_response(&value, "").expect("translation"), "Hello");
    }

    #[test]
    fn parses_bare_fenced_ollama_message_content() {
        let value = json!({
            "message": { "content": "```\n{\"translation\":\"Hello\"}\n```" }
        });
        assert_eq!(parse_response(&value, "").expect("translation"), "Hello");
    }

    #[test]
    fn parses_same_line_fenced_ollama_message_content() {
        let value = json!({
            "message": { "content": "```json {\"translation\":\"Hello\"} ```" }
        });
        assert_eq!(parse_response(&value, "").expect("translation"), "Hello");
    }

    #[test]
    fn parses_fence_with_a_spaced_json_tag() {
        let value = json!({
            "message": { "content": "``` json\n{\"translation\":\"Hello\"}\n```" }
        });
        assert_eq!(parse_response(&value, "").expect("translation"), "Hello");
    }

    #[test]
    fn parses_four_backtick_fence() {
        let value = json!({
            "message": { "content": "````json\n{\"translation\":\"Hello\"}\n````" }
        });
        assert_eq!(parse_response(&value, "").expect("translation"), "Hello");
    }

    #[test]
    fn rejects_prose_wrapped_fenced_message_content() {
        let value = json!({
            "message": {
                "content": "Here is the translation:\n```json\n{\"translation\":\"Hello\"}\n```"
            }
        });
        assert!(matches!(
            parse_response(&value, ""),
            Err(RuntimeError::Schema)
        ));
    }

    #[test]
    fn rejects_extra_fields_in_top_level_translation() {
        let value = json!({ "translation": "Hello", "notes": "extra" });
        assert!(matches!(
            parse_response(&value, ""),
            Err(RuntimeError::Schema)
        ));
    }

    #[test]
    fn rejects_extra_fields_in_structured_output() {
        let value = json!({
            "message": { "content": "{\"translation\":\"Hello\",\"notes\":\"extra\"}" }
        });
        assert!(matches!(
            parse_response(&value, ""),
            Err(RuntimeError::Schema)
        ));
    }

    #[test]
    fn rejects_empty_translation() {
        let value = json!({ "translation": "   " });
        assert!(matches!(
            parse_response(&value, ""),
            Err(RuntimeError::EmptyTranslation)
        ));
    }

    #[test]
    fn rejects_missing_fields() {
        assert!(matches!(
            parse_response(&json!({ "message": { "role": "assistant" } }), ""),
            Err(RuntimeError::Schema)
        ));
        assert!(matches!(
            parse_response(&json!({ "message": { "content": "not json" } }), ""),
            Err(RuntimeError::Schema)
        ));
    }

    #[test]
    fn decodes_literal_unicode_escapes_once() {
        assert_eq!(
            decode_literal_escapes(r"\u005bFF7700\u005d", ""),
            "[FF7700]"
        );
        assert_eq!(decode_literal_escapes(r"\u4f60\u597d", ""), "你好");
        assert_eq!(
            decode_literal_escapes(r"\u005BFF7700\u005D", ""),
            "[FF7700]"
        );
        assert_eq!(
            decode_literal_escapes(r"\u005cu005b", ""),
            r"\u005b",
            "only one layer is decoded"
        );
    }

    #[test]
    fn leaves_source_literal_escapes_untouched() {
        assert_eq!(
            decode_literal_escapes(r"keep \u005b literal", r"\u005b"),
            r"keep \u005b literal"
        );
    }

    #[test]
    fn decodes_surrogate_pairs_and_keeps_lone_surrogates() {
        assert_eq!(decode_literal_escapes(r"\uD83D\uDE00", ""), "😀");
        assert_eq!(decode_literal_escapes(r"\uD83D x", ""), r"\uD83D x");
        assert_eq!(decode_literal_escapes(r"\uDE00", ""), r"\uDE00");
    }

    #[test]
    fn leaves_incomplete_and_non_hex_escapes() {
        assert_eq!(
            decode_literal_escapes(r"\u12G4 \u12 \u", ""),
            r"\u12G4 \u12 \u"
        );
    }

    #[test]
    fn parse_response_repairs_double_escaped_message_content() {
        let value = json!({
            "message": {
                "content": r#"{"translation":"\\u005bFF7700\\u005d"}"#
            }
        });
        assert_eq!(
            parse_response(&value, "[FF7700]").expect("translation"),
            "[FF7700]"
        );
    }

    #[test]
    fn decodes_before_the_empty_check() {
        let value = json!({ "translation": r"\u0020" });
        assert!(matches!(
            parse_response(&value, ""),
            Err(RuntimeError::EmptyTranslation)
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
