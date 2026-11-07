use std::sync::Arc;

use crate::config::Config;
use crate::language;
use crate::lines;

/// Shared state for the inbound HTTP handler.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pipeline: Arc<Pipeline>,
}

/// The translation pipeline.
///
/// This ticket wires the inbound contract, language validation, and line
/// handling; model calls are added by the orchestration ticket.
#[derive(Clone)]
pub struct Pipeline {
    config: Arc<Config>,
}

impl Pipeline {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }

    /// Translate a Translation Unit, or return it unchanged when translation is
    /// not safe. The contract is passthrough-first: every failure path returns
    /// `text` exactly as received.
    pub async fn translate(&self, text: &str, from: Option<&str>, to: Option<&str>) -> String {
        if text == "{text}" || lines::is_blank(text) {
            return text.to_string();
        }

        let Some((_from, _to)) = language::resolve_pair(&self.config.languages, from, to) else {
            return text.to_string();
        };

        let segments = lines::split_lines(text);
        lines::reassemble(&segments)
    }
}
