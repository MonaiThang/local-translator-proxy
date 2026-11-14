//! Shared integration-test harness: a configurable mock Model Runtime, router
//! helpers, and in-flight/concurrency tracking.

#![allow(dead_code)]

use std::future::Future;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use local_translator_proxy::config::Config;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tower::ServiceExt;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone)]
pub enum Behavior {
    Echo,
    Prefix(&'static str),
    Replace(&'static str),
    Fenced(&'static str),
    ProseWrapped(&'static str),
    Status(u16),
    Delay(Duration),
    InjectNewline,
    DoubleEscapedBrackets,
    FailWhenContaining(&'static str),
    ReplaceWhenContaining {
        needle: &'static str,
        replacement: &'static str,
    },
}

#[derive(Clone)]
pub struct MockState {
    pub requests: Arc<Mutex<Vec<Value>>>,
    pub current_in_flight: Arc<AtomicUsize>,
    pub peak_in_flight: Arc<AtomicUsize>,
    behavior: Behavior,
}

pub async fn spawn_runtime(behavior: Behavior) -> (String, MockState) {
    let state = MockState {
        requests: Arc::new(Mutex::new(Vec::new())),
        current_in_flight: Arc::new(AtomicUsize::new(0)),
        peak_in_flight: Arc::new(AtomicUsize::new(0)),
        behavior,
    };
    let app = Router::new()
        .route("/api/chat", post(mock_handler))
        .with_state(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock server");
    });

    (format!("http://{addr}/api/chat"), state)
}

pub fn test_config(endpoint: String) -> Config {
    test_config_with(endpoint, 1, 4)
}

pub fn test_config_with(endpoint: String, timeout_secs: u64, max_in_flight: usize) -> Config {
    let mut config = Config::default();
    config.model_runtime.endpoint = endpoint;
    config.model_runtime.timeout_secs = timeout_secs;
    config.model_runtime.max_in_flight = max_in_flight;
    config
}

pub async fn request(app: &Router, method: &str, uri: &str) -> (StatusCode, String, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("infallible");

    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = String::from_utf8(body.to_vec()).expect("utf-8 body");

    (status, content_type, body)
}

/// Capture tracing output emitted while `future` runs on this thread.
///
/// The subscriber is thread-local, so it does not interfere with other tests.
/// Use a current-thread Tokio test; spawned tasks are then polled on the same
/// thread and inherit the subscriber.
pub async fn capture_logs<F, Fut, T>(filter: &str, future: F) -> (T, String)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    let buffer = LogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(buffer.clone())
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    let output = future().await;
    drop(guard);
    (output, buffer.contents())
}

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl LogBuffer {
    fn contents(&self) -> String {
        String::from_utf8(self.0.lock().expect("log buffer").clone()).expect("utf-8 logs")
    }
}

impl Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogBuffer {
    type Writer = LogBuffer;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

pub fn request_count(state: &MockState) -> usize {
    state.requests.lock().expect("requests lock").len()
}

pub fn peak_in_flight(state: &MockState) -> usize {
    state.peak_in_flight.load(Ordering::SeqCst)
}

struct InFlight<'a>(&'a MockState);

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.current_in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn mock_handler(State(state): State<MockState>, Json(body): Json<Value>) -> Response {
    let current = state.current_in_flight.fetch_add(1, Ordering::SeqCst) + 1;
    state.peak_in_flight.fetch_max(current, Ordering::SeqCst);
    let _in_flight = InFlight(&state);

    state
        .requests
        .lock()
        .expect("requests lock")
        .push(body.clone());

    let line = body
        .pointer("/messages/1/content")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match &state.behavior {
        Behavior::Echo => translated(line),
        Behavior::Prefix(prefix) => translated(&format!("{prefix}{line}")),
        Behavior::Replace(replacement) => translated(replacement),
        Behavior::Fenced(text) => fenced_translated(text),
        Behavior::ProseWrapped(text) => prose_wrapped_fenced(text),
        Behavior::Status(status) => StatusCode::from_u16(*status)
            .expect("valid status")
            .into_response(),
        Behavior::Delay(duration) => {
            tokio::time::sleep(*duration).await;
            translated(line)
        }
        Behavior::InjectNewline => translated(&format!("{line}\n")),
        Behavior::DoubleEscapedBrackets => {
            translated(&line.replace('[', r"\u005b").replace(']', r"\u005d"))
        }
        Behavior::FailWhenContaining(needle) if line.contains(needle) => {
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Behavior::ReplaceWhenContaining {
            needle,
            replacement,
        } if line.contains(needle) => translated(&line.replace(needle, replacement)),
        _ => translated(line),
    }
}

fn message_response(content: String) -> Response {
    Json(json!({ "message": { "role": "assistant", "content": content } })).into_response()
}

fn translated(text: &str) -> Response {
    message_response(json!({ "translation": text }).to_string())
}

fn fenced_translated(text: &str) -> Response {
    let payload = json!({ "translation": text }).to_string();
    message_response(format!("```json\n{payload}\n```"))
}

fn prose_wrapped_fenced(text: &str) -> Response {
    let payload = json!({ "translation": text }).to_string();
    message_response(format!("Here is the translation:\n```json\n{payload}\n```"))
}
