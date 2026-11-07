#![allow(dead_code)]

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

#[derive(Clone)]
pub enum Behavior {
    Echo,
    Prefix(&'static str),
    Replace(&'static str),
    Status(u16),
    Delay(Duration),
    InjectNewline,
    FailWhenContaining(&'static str),
    ReplaceWhenContaining {
        needle: &'static str,
        replacement: &'static str,
    },
}

#[derive(Clone)]
pub struct MockState {
    pub requests: Arc<Mutex<Vec<Value>>>,
    behavior: Behavior,
}

pub async fn spawn_runtime(behavior: Behavior) -> (String, MockState) {
    let state = MockState {
        requests: Arc::new(Mutex::new(Vec::new())),
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
    let mut config = Config::default();
    config.model_runtime.endpoint = endpoint;
    config.model_runtime.timeout_secs = 1;
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

pub fn request_count(state: &MockState) -> usize {
    state.requests.lock().expect("requests lock").len()
}

async fn mock_handler(State(state): State<MockState>, Json(body): Json<Value>) -> Response {
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
        Behavior::Status(status) => StatusCode::from_u16(*status)
            .expect("valid status")
            .into_response(),
        Behavior::Delay(duration) => {
            tokio::time::sleep(*duration).await;
            translated(line)
        }
        Behavior::InjectNewline => translated(&format!("{line}\n")),
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

fn translated(text: &str) -> Response {
    let content = json!({ "translation": text }).to_string();
    Json(json!({ "message": { "role": "assistant", "content": content } })).into_response()
}
