use serde::Deserialize;
use std::env;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub model_runtime: ModelRuntimeConfig,
    pub languages: LanguagesConfig,
    pub prompt: PromptConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ModelRuntimeConfig {
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_in_flight")]
    pub max_in_flight: usize,
    #[serde(default = "default_seed")]
    pub seed: i64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LanguagesConfig {
    #[serde(default = "default_supported_pairs")]
    pub supported_pairs: Vec<(String, String)>,
    #[serde(default = "default_from")]
    pub default_from: String,
    #[serde(default = "default_to")]
    pub default_to: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PromptConfig {
    #[serde(default = "default_system_message")]
    pub system_message: String,
}

fn default_listen_addr() -> String {
    "127.0.0.1:5000".to_string()
}
fn default_endpoint() -> String {
    "http://127.0.0.1:11434/api/chat".to_string()
}
fn default_model() -> String {
    "gemma2:9b".to_string()
}
fn default_timeout_secs() -> u64 {
    5
}
fn default_max_in_flight() -> usize {
    4
}
fn default_seed() -> i64 {
    12345
}
fn default_supported_pairs() -> Vec<(String, String)> {
    vec![("zh-CN".to_string(), "en".to_string())]
}
fn default_from() -> String {
    "zh-CN".to_string()
}
fn default_to() -> String {
    "en".to_string()
}
fn default_system_message() -> String {
    "You are a translation engine. Translate the user's text between the requested languages. Return ONLY the translated text in the required structured format. Preserve every XML/HTML tag, sprite/quad token, and escaped line break exactly as-is, with identical count and order. Do not add explanations, notes, or extra text.".to_string()
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let mut builder = config::Config::builder()
            .add_source(config::File::with_name("config").required(false))
            .add_source(config::Environment::with_prefix("TRANSLATOR_PROXY").separator("_"));

        // Also try config.toml explicitly
        if std::path::Path::new("config.toml").exists() {
            builder = builder.add_source(config::File::with_name("config.toml").required(false));
        }

        let cfg = builder.build()?;
        let mut config: Config = cfg.try_deserialize()?;

        // Ensure defaults if missing (config crate should handle via defaults)
        Ok(config)
    }
}
