mod common;

use axum::Router;
use axum::http::StatusCode;
use local_translator_proxy::build_router;

async fn app(behavior: common::Behavior) -> (Router, common::MockState) {
    let (endpoint, state) = common::spawn_runtime(behavior).await;
    (
        build_router(common::test_config(endpoint)).expect("router builds"),
        state,
    )
}

fn encode(text: &str) -> String {
    urlencoding::encode(text).into_owned()
}

#[tokio::test]
async fn translates_a_single_line() {
    let (app, _) = app(common::Behavior::Prefix("EN: ")).await;
    let (status, content_type, body) =
        common::request(&app, "GET", &format!("/?text={}", encode("你好"))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "text/plain; charset=utf-8");
    assert_eq!(body, "EN: 你好");
}

#[tokio::test]
async fn translates_multiple_lines_and_keeps_every_separator() {
    let (app, _) = app(common::Behavior::Prefix("EN: ")).await;
    let original = "一\r\n二\r三\n四";
    let (status, _, body) =
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "EN: 一\r\nEN: 二\rEN: 三\nEN: 四");
}

#[tokio::test]
async fn blank_lines_are_never_sent_to_the_runtime() {
    let (app, state) = app(common::Behavior::Prefix("EN: ")).await;
    let original = "a\r\n\r\n  \nb";
    let (status, _, body) =
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "EN: a\r\n\r\n  \nEN: b");
    assert_eq!(common::request_count(&state), 2);
}

#[tokio::test]
async fn token_preserving_translation_is_accepted() {
    let (app, _) = app(common::Behavior::ReplaceWhenContaining {
        needle: "Hello",
        replacement: "Bonjour",
    })
    .await;
    let original = "<color=red>Hello</color> <b>world</b>";
    let (status, _, body) =
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "<color=red>Bonjour</color> <b>world</b>");
}

#[tokio::test]
async fn token_mismatch_returns_the_whole_original_unit() {
    let (app, _) = app(common::Behavior::ReplaceWhenContaining {
        needle: "<color=red>",
        replacement: "<color=blue>",
    })
    .await;
    let original = "ok\n<color=red>Hello</color>";
    let (status, _, body) =
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
}

#[tokio::test]
async fn runtime_failure_on_one_line_returns_the_whole_original_unit() {
    let (app, _) = app(common::Behavior::FailWhenContaining("bad")).await;
    let original = "good\nbad";
    let (status, _, body) =
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
}

#[tokio::test]
async fn injected_line_break_returns_the_whole_original_unit() {
    let (app, _) = app(common::Behavior::InjectNewline).await;
    let original = "hello\nworld";
    let (status, _, body) =
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
}

#[tokio::test]
async fn unsupported_pair_makes_no_model_calls() {
    let (app, state) = app(common::Behavior::Prefix("EN: ")).await;
    let (status, _, body) = common::request(
        &app,
        "GET",
        &format!("/?text={}&from=en&to=zh-CN", encode("hello")),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello");
    assert_eq!(common::request_count(&state), 0);
}

#[tokio::test]
async fn placeholder_makes_no_model_calls() {
    let (app, state) = app(common::Behavior::Prefix("EN: ")).await;
    let (status, _, body) =
        common::request(&app, "GET", &format!("/?text={}", encode("{text}"))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "{text}");
    assert_eq!(common::request_count(&state), 0);
}

#[tokio::test]
async fn whitespace_only_unit_makes_no_model_calls() {
    let (app, state) = app(common::Behavior::Prefix("EN: ")).await;
    let (status, _, body) =
        common::request(&app, "GET", &format!("/?text={}", encode(" \t\r\n "))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, " \t\r\n ");
    assert_eq!(common::request_count(&state), 0);
}
