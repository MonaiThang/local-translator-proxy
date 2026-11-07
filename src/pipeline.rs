//! Translation Unit orchestration.
//!
//! [`Pipeline::translate`] is the passthrough-first core: it validates the
//! language pair, splits and token-checks each line, runs model calls
//! concurrently under a shared in-flight cap, verifies translations, and
//! reassembles the unit. The whole unit shares one wall-clock deadline, and the
//! first failure returns the original text untouched.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::Instant;

use crate::config::Config;
use crate::language;
use crate::lines::{self, Segment};
use crate::runtime::{RuntimeClient, RuntimeError};
use crate::tokens::{self, Token, TokenError};

/// Shared state for the inbound HTTP handler.
#[derive(Clone)]
pub struct AppState {
    /// Validated startup configuration.
    pub config: Arc<Config>,
    /// The shared translation pipeline.
    pub pipeline: Arc<Pipeline>,
}

/// The end-to-end translation pipeline.
///
/// One instance is shared across requests; the semaphore caps concurrent model
/// calls for the whole process.
#[derive(Clone)]
pub struct Pipeline {
    config: Arc<Config>,
    runtime: Arc<RuntimeClient>,
    permits: Arc<Semaphore>,
}

impl Pipeline {
    /// Build a pipeline from validated configuration and a runtime client.
    pub fn new(config: Arc<Config>, runtime: Arc<RuntimeClient>) -> Self {
        let permits = Arc::new(Semaphore::new(config.model_runtime.max_in_flight));
        Self {
            config,
            runtime,
            permits,
        }
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

        let timeout_secs = self.config.model_runtime.timeout_secs;
        let line_count = units.len();
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        tracing::debug!(line_count, %from, %to, "translating unit");

        // The deadline covers the whole Translation Unit: permit waits, every
        // model call, and token verification. A single slow line cannot consume
        // more than the unit's remaining budget.
        match tokio::time::timeout_at(deadline, self.run_unit(&segments, units, &from, &to)).await {
            Ok(Some(output)) => {
                tracing::debug!(line_count, "translation unit completed");
                output
            }
            Ok(None) => text.to_string(),
            Err(_) => {
                tracing::warn!(
                    timeout_secs,
                    line_count,
                    "translation unit timed out; returning the original unit"
                );
                text.to_string()
            }
        }
    }

    async fn run_unit(
        &self,
        segments: &[Segment<'_>],
        units: Vec<(usize, String, Vec<Token>)>,
        from: &str,
        to: &str,
    ) -> Option<String> {
        let mut tasks = JoinSet::new();
        for (index, line, source_tokens) in units {
            let runtime = self.runtime.clone();
            let permits = self.permits.clone();
            let from = from.to_string();
            let to = to.to_string();
            tasks.spawn(async move {
                let _permit = permits
                    .acquire_owned()
                    .await
                    .map_err(|_| LineFailure::Closed)?;
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
                    return None;
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "line translation task failed; returning the original unit"
                    );
                    tasks.abort_all();
                    return None;
                }
            }
        }

        let capacity = segments
            .iter()
            .map(|segment| segment.text.len() + segment.separator.len())
            .sum();
        let mut output = String::with_capacity(capacity);
        for (index, segment) in segments.iter().enumerate() {
            output.push_str(translated[index].as_deref().unwrap_or(segment.text));
            output.push_str(segment.separator);
        }
        Some(output)
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
    Closed,
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
            Self::Closed => write!(formatter, "in-flight limiter is closed"),
        }
    }
}
