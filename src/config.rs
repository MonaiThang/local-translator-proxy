//! Runtime configuration.
//!
//! [`Config::load`] reads `config.toml` from the working directory (defaults
//! apply when absent), applies the documented `TRANSLATOR_PROXY_*` environment
//! overrides, normalizes language tags, and validates every value. Invalid
//! configuration fails startup instead of serving a half-configured proxy.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::net::SocketAddr;

use crate::language::{is_language_tag, normalize_tag};

/// Runtime configuration for the Translation Proxy.
///
/// Values are loaded from `config.toml` (when present) and then overridden by
/// the `TRANSLATOR_PROXY_*` environment variables documented in the README.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub model_runtime: ModelRuntimeConfig,
    pub languages: LanguagesConfig,
    pub prompt: PromptConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Loopback address the HTTP server binds to.
    pub listen_addr: String,
}

/// Settings for the separately installed Model Runtime.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ModelRuntimeConfig {
    /// Ollama-compatible non-streaming chat endpoint.
    pub endpoint: String,
    /// Model name sent with each request.
    pub model: String,
    /// Wall-clock deadline for one whole Translation Unit, in seconds.
    pub timeout_secs: u64,
    /// Maximum concurrent Model Runtime calls across the process.
    pub max_in_flight: usize,
    /// Deterministic sampling seed.
    pub seed: i64,
}

/// The fixed source-to-target allowlist and its fallbacks.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LanguagesConfig {
    /// Allowed `(from, to)` pairs; compared after normalization.
    pub supported_pairs: Vec<(String, String)>,
    /// Source language used when the request omits or empties `from`.
    pub default_from: String,
    /// Target language used when the request omits or empties `to`.
    pub default_to: String,
}

/// The system instruction sent to the model.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PromptConfig {
    /// Base instruction; the proxy appends the requested language directive.
    pub system_message: String,
}

/// Default system instruction sent to the model.
///
/// It states the exact JSON schema because the client rejects any other shape.
/// The escaping sentence keeps literal `\n` / `\r\n` Rendering Tokens intact
/// when the model embeds them in a JSON string.
const DEFAULT_SYSTEM_MESSAGE: &str = "You are a translation engine. Translate the user's text from the source language to the target language. Return exactly one JSON object and nothing else: {\"translation\": \"<translated text>\"}. Do not use markdown code fences. Do not add explanations, notes, or any other keys. Preserve every XML/HTML tag, sprite/quad token, and escaped line break exactly as-is, with identical count and order. The translation value must be a valid JSON string: escape ASCII double quotes and backslashes, so a literal \\n in the source appears as \\\\n in the output.";

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:5000".to_string(),
        }
    }
}

impl Default for ModelRuntimeConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:11434/api/chat".to_string(),
            model: "gemma2:9b".to_string(),
            timeout_secs: 5,
            max_in_flight: 4,
            seed: 12345,
        }
    }
}

impl Default for LanguagesConfig {
    fn default() -> Self {
        Self {
            supported_pairs: vec![("zh-CN".to_string(), "en".to_string())],
            default_from: "zh-CN".to_string(),
            default_to: "en".to_string(),
        }
    }
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            system_message: DEFAULT_SYSTEM_MESSAGE.to_string(),
        }
    }
}

impl Config {
    /// Load `config.toml` from the current directory (if present), apply
    /// environment overrides, normalize, and validate.
    pub fn load() -> Result<Self> {
        let mut config = match std::fs::read_to_string("config.toml") {
            Ok(contents) => Self::from_toml_unvalidated(&contents).context("parse config.toml")?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(err) => return Err(err).context("read config.toml"),
        };

        config.apply_env_overrides(std::env::vars())?;
        config.finish()?;
        Ok(config)
    }

    /// Parse and validate a TOML document using the same rules as [`Config::load`].
    pub fn from_toml_str(contents: &str) -> Result<Self> {
        let mut config = Self::from_toml_unvalidated(contents).context("parse TOML")?;
        config.finish()?;
        Ok(config)
    }

    fn from_toml_unvalidated(contents: &str) -> Result<Self> {
        toml::from_str(contents).context("deserialize TOML")
    }

    /// Apply the documented `TRANSLATOR_PROXY_*` overrides.
    ///
    /// This is deliberately an explicit mapping rather than the `config`
    /// crate's separator-based nesting: the documented names use single
    /// underscores inside section names (`MODEL_RUNTIME_ENDPOINT`), which the
    /// generic nested-key parser would read as `model.runtime.endpoint`.
    pub fn apply_env_overrides<I>(&mut self, vars: I) -> Result<()>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        for (key, value) in vars {
            match key.as_str() {
                "TRANSLATOR_PROXY_SERVER_LISTEN_ADDR" => self.server.listen_addr = value,
                "TRANSLATOR_PROXY_MODEL_RUNTIME_ENDPOINT" => self.model_runtime.endpoint = value,
                "TRANSLATOR_PROXY_MODEL_RUNTIME_MODEL" => self.model_runtime.model = value,
                "TRANSLATOR_PROXY_MODEL_RUNTIME_TIMEOUT_SECS" => {
                    self.model_runtime.timeout_secs = value
                        .parse()
                        .with_context(|| format!("invalid {key} value {value:?}"))?;
                }
                "TRANSLATOR_PROXY_MODEL_RUNTIME_MAX_IN_FLIGHT" => {
                    self.model_runtime.max_in_flight = value
                        .parse()
                        .with_context(|| format!("invalid {key} value {value:?}"))?;
                }
                "TRANSLATOR_PROXY_MODEL_RUNTIME_SEED" => {
                    self.model_runtime.seed = value
                        .parse()
                        .with_context(|| format!("invalid {key} value {value:?}"))?;
                }
                "TRANSLATOR_PROXY_LANGUAGES_SUPPORTED_PAIRS" => {
                    self.languages.supported_pairs = parse_pairs(&value)
                        .with_context(|| format!("invalid {key} value {value:?}"))?;
                }
                "TRANSLATOR_PROXY_LANGUAGES_DEFAULT_FROM" => self.languages.default_from = value,
                "TRANSLATOR_PROXY_LANGUAGES_DEFAULT_TO" => self.languages.default_to = value,
                "TRANSLATOR_PROXY_PROMPT_SYSTEM_MESSAGE" => self.prompt.system_message = value,
                _ => {}
            }
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        self.normalize();
        self.validate()
    }

    fn normalize(&mut self) {
        self.languages.normalize();
    }

    /// Check every field for a usable value.
    pub fn validate(&self) -> Result<()> {
        let addr: SocketAddr =
            self.server.listen_addr.parse().with_context(|| {
                format!("invalid server.listen_addr {:?}", self.server.listen_addr)
            })?;

        let endpoint = reqwest::Url::parse(&self.model_runtime.endpoint).with_context(|| {
            format!(
                "invalid model_runtime.endpoint {:?}",
                self.model_runtime.endpoint
            )
        })?;
        if endpoint.scheme() != "http" && endpoint.scheme() != "https" {
            bail!("model_runtime.endpoint must use http or https");
        }
        if endpoint.host_str().is_none() {
            bail!("model_runtime.endpoint must include a host");
        }

        if self.model_runtime.model.trim().is_empty() {
            bail!("model_runtime.model must not be empty");
        }
        if self.model_runtime.timeout_secs == 0 {
            bail!("model_runtime.timeout_secs must be greater than zero");
        }
        if self.model_runtime.max_in_flight == 0 {
            bail!("model_runtime.max_in_flight must be greater than zero");
        }

        if self.languages.supported_pairs.is_empty() {
            bail!("languages.supported_pairs must not be empty");
        }
        for (from, to) in &self.languages.supported_pairs {
            if !is_language_tag(from) || !is_language_tag(to) {
                bail!("invalid language pair {from:?} -> {to:?}");
            }
        }
        if !is_language_tag(&self.languages.default_from) {
            bail!(
                "invalid languages.default_from {:?}",
                self.languages.default_from
            );
        }
        if !is_language_tag(&self.languages.default_to) {
            bail!(
                "invalid languages.default_to {:?}",
                self.languages.default_to
            );
        }
        if !self
            .languages
            .supports(&self.languages.default_from, &self.languages.default_to)
        {
            bail!(
                "default language pair {} -> {} is not in languages.supported_pairs",
                self.languages.default_from,
                self.languages.default_to
            );
        }

        if self.prompt.system_message.trim().is_empty() {
            bail!("prompt.system_message must not be empty");
        }

        let _ = addr;
        Ok(())
    }
}

impl LanguagesConfig {
    /// Normalize tags to the lowercase form used for comparison.
    pub fn normalize(&mut self) {
        self.default_from = normalize_tag(&self.default_from);
        self.default_to = normalize_tag(&self.default_to);
        self.supported_pairs = self
            .supported_pairs
            .iter()
            .map(|(from, to)| (normalize_tag(from), normalize_tag(to)))
            .collect();
    }

    /// Whether the configured allowlist contains this pairing.
    pub fn supports(&self, from: &str, to: &str) -> bool {
        self.supported_pairs
            .iter()
            .any(|(allowed_from, allowed_to)| {
                allowed_from.eq_ignore_ascii_case(from) && allowed_to.eq_ignore_ascii_case(to)
            })
    }
}

/// Parse a comma-separated `from:to` list used by the env override.
fn parse_pairs(value: &str) -> Result<Vec<(String, String)>> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }

    value
        .split(',')
        .map(|pair| {
            let (from, to) = pair
                .split_once(':')
                .context("expected comma-separated from:to pairs")?;
            Ok((from.trim().to_string(), to.trim().to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn defaults_are_valid() {
        Config::default().validate().expect("defaults are valid");
    }

    #[test]
    fn toml_overrides_defaults() {
        let config = Config::from_toml_str(
            r#"
            [server]
            listen_addr = "127.0.0.1:5050"

            [model_runtime]
            model = "test-model"
            timeout_secs = 2

            [languages]
            supported_pairs = [["ja", "en"]]
            default_from = "ja"
            default_to = "en"
            "#,
        )
        .expect("valid config");

        assert_eq!(config.server.listen_addr, "127.0.0.1:5050");
        assert_eq!(config.model_runtime.model, "test-model");
        assert_eq!(config.model_runtime.timeout_secs, 2);
        assert_eq!(
            config.languages.supported_pairs,
            vec![("ja".to_string(), "en".to_string())]
        );
    }

    #[test]
    fn env_overrides_nested_keys() {
        let mut config = Config::default();
        config
            .apply_env_overrides(env(&[
                ("TRANSLATOR_PROXY_SERVER_LISTEN_ADDR", "127.0.0.1:5099"),
                (
                    "TRANSLATOR_PROXY_MODEL_RUNTIME_ENDPOINT",
                    "http://127.0.0.1:9999/api/chat",
                ),
                ("TRANSLATOR_PROXY_MODEL_RUNTIME_MODEL", "env-model"),
                ("TRANSLATOR_PROXY_MODEL_RUNTIME_TIMEOUT_SECS", "9"),
                ("TRANSLATOR_PROXY_MODEL_RUNTIME_MAX_IN_FLIGHT", "7"),
                ("TRANSLATOR_PROXY_MODEL_RUNTIME_SEED", "42"),
                (
                    "TRANSLATOR_PROXY_LANGUAGES_SUPPORTED_PAIRS",
                    "ja:en, zh-CN : en",
                ),
                ("TRANSLATOR_PROXY_LANGUAGES_DEFAULT_FROM", "ja"),
                ("TRANSLATOR_PROXY_LANGUAGES_DEFAULT_TO", "en"),
                ("TRANSLATOR_PROXY_PROMPT_SYSTEM_MESSAGE", "translate"),
            ]))
            .expect("valid env");
        config.finish().expect("valid config");

        assert_eq!(config.server.listen_addr, "127.0.0.1:5099");
        assert_eq!(
            config.model_runtime.endpoint,
            "http://127.0.0.1:9999/api/chat"
        );
        assert_eq!(config.model_runtime.model, "env-model");
        assert_eq!(config.model_runtime.timeout_secs, 9);
        assert_eq!(config.model_runtime.max_in_flight, 7);
        assert_eq!(config.model_runtime.seed, 42);
        assert_eq!(
            config.languages.supported_pairs,
            vec![
                ("ja".to_string(), "en".to_string()),
                ("zh-cn".to_string(), "en".to_string()),
            ]
        );
        assert_eq!(config.languages.default_from, "ja");
        assert_eq!(config.prompt.system_message, "translate");
    }

    #[test]
    fn invalid_timeout_is_rejected() {
        let err = Config::from_toml_str(
            r#"
            [model_runtime]
            timeout_secs = 0
            "#,
        )
        .expect_err("zero timeout must be rejected");
        assert!(err.to_string().contains("timeout_secs"));
    }

    #[test]
    fn unsupported_default_pair_is_rejected() {
        let err = Config::from_toml_str(
            r#"
            [languages]
            supported_pairs = [["ja", "en"]]
            default_from = "zh-CN"
            default_to = "en"
            "#,
        )
        .expect_err("default pair must be supported");
        assert!(err.to_string().contains("default language pair"));
    }

    #[test]
    fn malformed_pair_override_is_rejected() {
        let mut config = Config::default();
        let err = config
            .apply_env_overrides(env(&[(
                "TRANSLATOR_PROXY_LANGUAGES_SUPPORTED_PAIRS",
                "not-a-pair",
            )]))
            .expect_err("malformed pair must be rejected");
        assert!(
            err.to_string()
                .contains("TRANSLATOR_PROXY_LANGUAGES_SUPPORTED_PAIRS")
        );
    }

    #[test]
    fn default_prompt_states_the_schema_and_escaping() {
        let prompt = Config::default().prompt.system_message;

        assert!(prompt.contains(r#"{"translation": "<translated text>"}"#));
        assert!(prompt.contains("Preserve every XML/HTML tag"));
        assert!(prompt.contains("escape ASCII double quotes and backslashes"));
        assert!(prompt.contains(r"a literal \n in the source appears as \\n in the output"));
    }

    #[test]
    fn example_config_uses_the_default_prompt() {
        let example = Config::from_toml_str(include_str!("../config.example.toml"))
            .expect("config.example.toml parses");

        assert_eq!(
            example.prompt.system_message,
            Config::default().prompt.system_message
        );
    }

    #[test]
    fn tracked_config_uses_the_default_prompt() {
        let tracked =
            Config::from_toml_str(include_str!("../config.toml")).expect("config.toml parses");

        assert_eq!(
            tracked.prompt.system_message,
            Config::default().prompt.system_message
        );
    }
}
