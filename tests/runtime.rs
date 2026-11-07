use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use local_translator_proxy::config::Config;
use local_translator_proxy::runtime::{RuntimeClient, RuntimeError};
use serde_json::{Value, json};
use tokio::net::TcpListener;

#[derive(Clone)]
enum Behavior {
    Translate(&'static str),
    TopLevel(&'static str),
    Status(u16),
    InvalidJson,
    GarbageContent,
    Empty,
    Delay(Duration),
}

#[derive(Clone)]
struct MockState {
    requests: Arc<Mutex<Vec<Value>>>,
    behavior: Behavior,
}

async fn mock_handler(State(state): State<MockState>, Json(body): Json<Value>) -> Response {
    state.requests.lock().expect("requests lock").push(body);

    match &state.behavior {
        Behavior::Translate(text) => {
            let content = json!({ "translation": text }).to_string();
            Json(json!({ "message": { "role": "assistant", "content": content } })).into_response()
        }
        Behavior::TopLevel(text) => Json(json!({ "translation": text })).into_response(),
        Behavior::Status(status) => StatusCode::from_u16(*status)
            .expect("valid status")
            .into_response(),
        Behavior::InvalidJson => (StatusCode::OK, "not json").into_response(),
        Behavior::GarbageContent => {
            Json(json!({ "message": { "content": "not json" } })).into_response()
        }
        Behavior::Empty => Json(json!({
            "message": { "content": "{\"translation\":\"\"}" }
        }))
        .into_response(),
        Behavior::Delay(duration) => {
            tokio::time::sleep(*duration).await;
            let content = json!({ "translation": "late" }).to_string();
            Json(json!({ "message": { "content": content } })).into_response()
        }
    }
}

async fn spawn(behavior: Behavior) -> (String, MockState) {
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

fn client_for(uri: &str, timeout_secs: u64) -> RuntimeClient {
    let mut config = Config::default();
    config.model_runtime.endpoint = uri.to_string();
    config.model_runtime.timeout_secs = timeout_secs;
    RuntimeClient::new(&config.model_runtime, &config.prompt).expect("runtime client")
}

#[tokio::test]
async fn sends_expected_request_and_parses_content() {
    let (uri, state) = spawn(Behavior::Translate("Hello")).await;
    let client = client_for(&uri, 5);

    let translation = client
        .translate_line("你好", "zh-cn", "en")
        .await
        .expect("translation");
    assert_eq!(translation, "Hello");

    let requests = state.requests.lock().expect("requests lock");
    assert_eq!(requests.len(), 1);
    let request = &requests[0];

    assert_eq!(request["model"], "gemma2:9b");
    assert_eq!(request["stream"], false);
    assert_eq!(request["options"]["temperature"], 0);
    assert_eq!(request["options"]["seed"], 12345);
    assert_eq!(request["messages"].as_array().expect("messages").len(), 2);

    assert_eq!(request["messages"][0]["role"], "system");
    let system = request["messages"][0]["content"]
        .as_str()
        .expect("system content");
    assert!(system.contains("Source language: zh-cn"));
    assert!(system.contains("Target language: en"));

    assert_eq!(request["messages"][1]["role"], "user");
    assert_eq!(request["messages"][1]["content"], "你好");
}

#[tokio::test]
async fn parses_top_level_translation() {
    let (uri, _) = spawn(Behavior::TopLevel("Hello")).await;
    let client = client_for(&uri, 5);

    assert_eq!(
        client
            .translate_line("你好", "zh-cn", "en")
            .await
            .expect("translation"),
        "Hello"
    );
}

#[tokio::test]
async fn non_success_status_is_an_error() {
    let (uri, _) = spawn(Behavior::Status(500)).await;
    let client = client_for(&uri, 5);

    let error = client
        .translate_line("你好", "zh-cn", "en")
        .await
        .expect_err("status error");
    assert!(matches!(error, RuntimeError::HttpStatus(500)));
}

#[tokio::test]
async fn invalid_json_is_a_decode_error() {
    let (uri, _) = spawn(Behavior::InvalidJson).await;
    let client = client_for(&uri, 5);

    let error = client
        .translate_line("你好", "zh-cn", "en")
        .await
        .expect_err("decode error");
    assert!(matches!(error, RuntimeError::Decode));
}

#[tokio::test]
async fn garbage_content_is_a_schema_error() {
    let (uri, _) = spawn(Behavior::GarbageContent).await;
    let client = client_for(&uri, 5);

    let error = client
        .translate_line("你好", "zh-cn", "en")
        .await
        .expect_err("schema error");
    assert!(matches!(error, RuntimeError::Schema));
}

#[tokio::test]
async fn empty_translation_is_an_error() {
    let (uri, _) = spawn(Behavior::Empty).await;
    let client = client_for(&uri, 5);

    let error = client
        .translate_line("你好", "zh-cn", "en")
        .await
        .expect_err("empty error");
    assert!(matches!(error, RuntimeError::EmptyTranslation));
}

#[tokio::test]
async fn timeout_is_an_error() {
    let (uri, _) = spawn(Behavior::Delay(Duration::from_secs(3))).await;
    let client = client_for(&uri, 1);

    let error = client
        .translate_line("你好", "zh-cn", "en")
        .await
        .expect_err("timeout error");
    assert!(matches!(error, RuntimeError::Timeout));
}

#[tokio::test]
async fn closed_port_is_an_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(listener);

    let client = client_for(&format!("http://{addr}/api/chat"), 1);
    let error = client
        .translate_line("你好", "zh-cn", "en")
        .await
        .expect_err("unreachable error");
    assert!(
        matches!(error, RuntimeError::Unreachable | RuntimeError::Timeout),
        "unexpected error: {error:?}"
    );
}
