use std::sync::Arc;

use tokio::task::JoinSet;

use crate::config::Config;
use crate::language;
use crate::lines;
use crate::runtime::{RuntimeClient, RuntimeError};
use crate::tokens::{self, Token, TokenError};

/// Shared state for the inbound HTTP handler.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pipeline: Arc<Pipeline>,
}

/// The end-to-end translation pipeline.
#[derive(Clone)]
pub struct Pipeline {
    config: Arc<Config>,
    runtime: Arc<RuntimeClient>,
}

impl Pipeline {
    pub fn new(config: Arc<Config>, runtime: Arc<RuntimeClient>) -> Self {
        Self { config, runtime }
    }

    /// Translate a Translation Unit, or return it unchanged when translation is
    /// not safe. The contract is passthrough-first: every failure path returns
    /// `text` exactly as received, with its original separators.
    pub async fn translate(&self, text: &str, from: Option<&str>, to: Option<&str>) -> String {
        if text == "{text}" || lines::is_blank(text) {
            return text.to_string();
        }

        let Some((from, to)) = language::resolve_pair(&self.config.languages, from, to) else {
            return text.to_string();
        };

        let segments = lines::split_lines(text);
        let mut units = Vec::new();
        for (index, segment) in segments.iter().enumerate() {
            if lines::is_blank(segment.text) {
                continue;
            }
            let source_tokens = match tokens::extract(segment.text) {
                Ok(tokens) => tokens,
                Err(_) => return text.to_string(),
            };
            units.push((index, segment.text.to_string(), source_tokens));
        }

        if units.is_empty() {
            return text.to_string();
        }

        let mut tasks = JoinSet::new();
        for (index, line, source_tokens) in units {
            let runtime = self.runtime.clone();
            let from = from.clone();
            let to = to.clone();
            tasks.spawn(async move {
                translate_line(runtime.as_ref(), index, line, source_tokens, &from, &to).await
            });
        }

        let mut translated: Vec<Option<String>> = vec![None; segments.len()];
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok(Ok((index, line))) => translated[index] = Some(line),
                Ok(Err(failure)) => {
                    tracing::warn!(%failure, "line translation failed; returning the original unit");
                    tasks.abort_all();
                    return text.to_string();
                }
                Err(error) => {
                    tracing::warn!(%error, "line translation task failed; returning the original unit");
                    tasks.abort_all();
                    return text.to_string();
                }
            }
        }

        let mut output = String::with_capacity(text.len());
        for (index, segment) in segments.iter().enumerate() {
            output.push_str(translated[index].as_deref().unwrap_or(segment.text));
            output.push_str(segment.separator);
        }
        output
    }
}

async fn translate_line(
    runtime: &RuntimeClient,
    index: usize,
    line: String,
    source_tokens: Vec<Token>,
    from: &str,
    to: &str,
) -> Result<(usize, String), LineFailure> {
    let translated = runtime
        .translate_line(&line, from, to)
        .await
        .map_err(LineFailure::Runtime)?;

    let translated_tokens =
        tokens::extract(&translated).map_err(LineFailure::MalformedTranslation)?;
    if !tokens::matches(&source_tokens, &translated_tokens) {
        return Err(LineFailure::TokenMismatch);
    }
    if translated.contains(['\r', '\n']) {
        return Err(LineFailure::InjectedLineBreak);
    }

    Ok((index, translated))
}

#[derive(Debug)]
enum LineFailure {
    Runtime(RuntimeError),
    MalformedTranslation(TokenError),
    TokenMismatch,
    InjectedLineBreak,
}

impl std::fmt::Display for LineFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(error) => write!(formatter, "runtime error: {error}"),
            Self::MalformedTranslation(error) => {
                write!(formatter, "malformed translation: {error}")
            }
            Self::TokenMismatch => write!(formatter, "rendering token mismatch"),
            Self::InjectedLineBreak => write!(formatter, "translation injected a line break"),
        }
    }
}
