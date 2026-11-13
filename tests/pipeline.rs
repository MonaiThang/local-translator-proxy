mod common;

use std::time::{Duration, Instant};

use axum::Router;
use axum::http::StatusCode;
use local_translator_proxy::build_router;

async fn app(behavior: common::Behavior) -> (Router, common::MockState) {
    app_with(behavior, 1, 4).await
}

async fn app_with(
    behavior: common::Behavior,
    timeout_secs: u64,
    max_in_flight: usize,
) -> (Router, common::MockState) {
    let (endpoint, state) = common::spawn_runtime(behavior).await;
    let config = common::test_config_with(endpoint, timeout_secs, max_in_flight);
    (build_router(config).expect("router builds"), state)
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
async fn sends_the_schema_and_escaping_instructions_to_the_runtime() {
    let (app, state) = app(common::Behavior::Prefix("EN: ")).await;
    let _ = common::request(&app, "GET", &format!("/?text={}", encode("你好"))).await;

    let requests = state.requests.lock().expect("requests lock");
    let system = requests[0]
        .pointer("/messages/0/content")
        .and_then(serde_json::Value::as_str)
        .expect("system message");

    assert!(system.contains(r#"{"translation": "<translated text>"}"#));
    assert!(system.contains("escape ASCII double quotes and backslashes"));
    assert!(system.contains("Source language: zh-cn"));
    assert!(system.contains("Target language: en"));
}

#[tokio::test]
async fn fenced_json_from_the_runtime_is_translated() {
    let (app, _) = app(common::Behavior::Fenced("Hello")).await;
    let (status, content_type, body) =
        common::request(&app, "GET", &format!("/?text={}", encode("你好"))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "text/plain; charset=utf-8");
    assert_eq!(body, "Hello");
}

#[tokio::test]
async fn prose_wrapped_fenced_response_returns_the_whole_original_unit() {
    let (app, _) = app(common::Behavior::ProseWrapped("Hello")).await;
    let original = "你好";
    let (status, content_type, body) =
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "text/plain; charset=utf-8");
    assert_eq!(body, original);
}

#[tokio::test]
async fn escaped_line_break_tokens_survive_fenced_json() {
    let original = r"第一行\r\n第二行\n第三行";
    let translated = r"First line\r\nSecond line\nThird line";
    let (app, _) = app(common::Behavior::Fenced(translated)).await;
    let (status, _, body) =
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, translated);
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

#[tokio::test]
async fn concurrency_is_capped_by_max_in_flight() {
    let (app, state) = app_with(common::Behavior::Delay(Duration::from_millis(100)), 5, 2).await;
    let original = "a\nb\nc\nd\ne\nf";
    let (status, _, body) =
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
    assert_eq!(common::request_count(&state), 6);
    assert_eq!(common::peak_in_flight(&state), 2);
}

#[tokio::test]
async fn unit_deadline_bounds_the_whole_translation_unit() {
    let (app, _) = app_with(common::Behavior::Delay(Duration::from_millis(400)), 1, 1).await;
    let original = "a\nb\nc\nd";
    let started = Instant::now();
    let (status, _, body) =
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await;
    let elapsed = started.elapsed();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
    assert!(
        elapsed < Duration::from_millis(1500),
        "unit should stop at its deadline, took {elapsed:?}"
    );
}

#[tokio::test]
async fn logs_verified_lines_at_info_in_source_order() {
    let (app, _) = app(common::Behavior::Prefix("EN: ")).await;
    let original = "一\r\n\r\n二\n三";
    let ((status, _, body), logs) = common::capture_logs("local_translator_proxy=info", || async {
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await
    })
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "EN: 一\r\n\r\nEN: 二\nEN: 三");

    let info: Vec<&str> = logs
        .lines()
        .filter(|line| line.contains(" INFO "))
        .collect();
    assert_eq!(info.len(), 3, "logs: {logs}");
    assert!(info[0].contains("line=0"), "logs: {logs}");
    assert!(info[0].contains("from=zh-cn"), "logs: {logs}");
    assert!(info[0].contains("to=en"), "logs: {logs}");
    assert!(info[0].contains(r#"source="一""#), "logs: {logs}");
    assert!(info[0].contains(r#"translated="EN: 一""#), "logs: {logs}");
    assert!(info[1].contains("line=2"), "logs: {logs}");
    assert!(info[1].contains(r#"source="二""#), "logs: {logs}");
    assert!(info[1].contains(r#"translated="EN: 二""#), "logs: {logs}");
    assert!(info[2].contains("line=3"), "logs: {logs}");
    assert!(info[2].contains(r#"source="三""#), "logs: {logs}");
    assert!(info[2].contains(r#"translated="EN: 三""#), "logs: {logs}");
}

#[tokio::test]
async fn logs_rejected_translation_at_warn_and_the_original_unit() {
    let (app, _) = app(common::Behavior::ReplaceWhenContaining {
        needle: "<color=red>",
        replacement: "<color=blue>",
    })
    .await;
    let original = "ok\n<color=red>Hello</color>";
    let ((status, _, body), logs) = common::capture_logs("local_translator_proxy=info", || async {
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await
    })
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
    assert!(logs.contains("translation rejected"), "logs: {logs}");
    assert!(logs.contains("line=1"), "logs: {logs}");
    assert!(
        logs.contains(r#"source="<color=red>Hello</color>""#),
        "logs: {logs}"
    );
    assert!(
        logs.contains(r#"rejected="<color=blue>Hello</color>""#),
        "logs: {logs}"
    );
    assert!(logs.contains("rendering token mismatch"), "logs: {logs}");
    assert!(logs.contains("line translation failed"), "logs: {logs}");
    assert!(
        logs.contains(r#"text="ok\n<color=red>Hello</color>""#),
        "logs: {logs}"
    );
    assert!(!logs.contains("line=0"), "logs: {logs}");
}

#[tokio::test]
async fn logs_malformed_translated_tokens_as_rejected() {
    let (app, _) = app(common::Behavior::ReplaceWhenContaining {
        needle: "<b>",
        replacement: r#"<b class="x">"#,
    })
    .await;
    let original = "<b>Hello</b>";
    let ((status, _, body), logs) = common::capture_logs("local_translator_proxy=info", || async {
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await
    })
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
    assert!(logs.contains("translation rejected"), "logs: {logs}");
    assert!(logs.contains("malformed rendering token"), "logs: {logs}");
    assert!(
        logs.contains(r#"rejected="<b class=\"x\">Hello</b>""#),
        "logs: {logs}"
    );
    assert!(logs.contains("line translation failed"), "logs: {logs}");
}

#[tokio::test]
async fn logs_injected_line_breaks_escaped_on_one_physical_line() {
    let (app, _) = app(common::Behavior::InjectNewline).await;
    let original = "hello";
    let ((status, _, body), logs) = common::capture_logs("local_translator_proxy=info", || async {
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await
    })
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
    assert!(logs.contains("translation rejected"), "logs: {logs}");
    assert!(logs.contains(r#"rejected="hello\n""#), "logs: {logs}");
    assert!(logs.contains("line translation failed"), "logs: {logs}");

    let rejected: Vec<&str> = logs
        .lines()
        .filter(|line| line.contains("rejected="))
        .collect();
    assert_eq!(rejected.len(), 1, "logs: {logs}");
    assert!(
        rejected[0].contains(r#"rejected="hello\n""#),
        "logs: {logs}"
    );
}

#[tokio::test]
async fn logs_runtime_failure_as_a_unit_warn_without_rejected_text() {
    let (app, _) = app(common::Behavior::FailWhenContaining("bad")).await;
    let original = "good\nbad";
    let ((status, _, body), logs) = common::capture_logs("local_translator_proxy=info", || async {
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await
    })
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
    assert!(logs.contains("line translation failed"), "logs: {logs}");
    assert!(logs.contains("HTTP 500"), "logs: {logs}");
    assert!(logs.contains(r#"text="good\nbad""#), "logs: {logs}");
    assert!(!logs.contains("rejected="), "logs: {logs}");
    assert!(!logs.contains("source="), "logs: {logs}");
}

#[tokio::test]
async fn logs_unsupported_pair_at_debug_with_the_original_unit() {
    let (app, state) = app(common::Behavior::Prefix("EN: ")).await;
    let ((status, _, body), logs) =
        common::capture_logs("local_translator_proxy=debug", || async {
            common::request(
                &app,
                "GET",
                &format!("/?text={}&from=en&to=zh-CN", encode("hello")),
            )
            .await
        })
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello");
    assert_eq!(common::request_count(&state), 0);
    assert!(logs.contains("unsupported language pair"), "logs: {logs}");
    assert!(
        logs.contains(r#"requested_from=Some("en")"#),
        "logs: {logs}"
    );
    assert!(
        logs.contains(r#"requested_to=Some("zh-CN")"#),
        "logs: {logs}"
    );
    assert!(logs.contains(r#"text="hello""#), "logs: {logs}");
}

#[tokio::test]
async fn logs_ambiguous_source_tokens_at_debug_with_the_original_unit() {
    let (app, state) = app(common::Behavior::Prefix("EN: ")).await;
    let original = "ok\n<color=red";
    let ((status, _, body), logs) =
        common::capture_logs("local_translator_proxy=debug", || async {
            common::request(&app, "GET", &format!("/?text={}", encode(original))).await
        })
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
    assert_eq!(common::request_count(&state), 0);
    assert!(logs.contains("ambiguous rendering tokens"), "logs: {logs}");
    assert!(logs.contains("line=1"), "logs: {logs}");
    assert!(logs.contains(r#"text="ok\n<color=red""#), "logs: {logs}");
}

#[tokio::test]
async fn logs_timeout_at_warn_with_the_original_unit() {
    let (app, _) = app_with(common::Behavior::Delay(Duration::from_millis(600)), 1, 1).await;
    let original = "a\nb";
    let ((status, _, body), logs) = common::capture_logs("local_translator_proxy=info", || async {
        common::request(&app, "GET", &format!("/?text={}", encode(original))).await
    })
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
    assert!(logs.contains("timed out"), "logs: {logs}");
    assert!(logs.contains("timeout_secs=1"), "logs: {logs}");
    assert!(logs.contains("line_count=2"), "logs: {logs}");
    assert!(logs.contains(r#"text="a\nb""#), "logs: {logs}");
    assert!(!logs.contains("line translated"), "logs: {logs}");
}

#[tokio::test]
async fn blank_and_placeholder_units_emit_no_logs() {
    let (app, _) = app(common::Behavior::Prefix("EN: ")).await;

    for text in [" \t\r\n ", "{text}"] {
        let ((status, _, body), logs) =
            common::capture_logs("local_translator_proxy=debug", || async {
                common::request(&app, "GET", &format!("/?text={}", encode(text))).await
            })
            .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, text);
        assert!(logs.is_empty(), "logs for {text:?}: {logs}");
    }
}
