mod common;

use axum::Router;
use axum::http::StatusCode;
use local_translator_proxy::build_router;

async fn app() -> Router {
    let (endpoint, _) = common::spawn_runtime(common::Behavior::Echo).await;
    build_router(common::test_config(endpoint)).expect("router builds")
}

#[tokio::test]
async fn fixture_request_returns_plain_text() {
    let app = app().await;
    let (status, content_type, body) = common::request(
        &app,
        "GET",
        "/?text=%e4%bd%a0%e5%a5%bd%ef%bc%8c%e6%97%85%e8%a1%8c%e8%80%85%e3%80%82&from=zh-CN&to=en",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "text/plain; charset=utf-8");
    assert_eq!(body, "你好，旅行者。");
}

#[tokio::test]
async fn any_path_is_accepted() {
    let app = app().await;
    let (status, _, body) =
        common::request(&app, "GET", "/some/deep/path?text=hello&from=zh-CN&to=en").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello");
}

#[tokio::test]
async fn verb_is_not_validated() {
    let app = app().await;
    let (status, _, body) = common::request(&app, "POST", "/?text=hello").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello");
}

#[tokio::test]
async fn missing_text_returns_empty_200() {
    let app = app().await;
    let (status, _, body) = common::request(&app, "GET", "/?from=zh-CN&to=en").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "");
}

#[tokio::test]
async fn empty_text_returns_empty_200() {
    let app = app().await;
    let (status, _, body) = common::request(&app, "GET", "/?text=&from=zh-CN&to=en").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "");
}

#[tokio::test]
async fn placeholder_is_returned_unchanged() {
    let app = app().await;
    let (status, _, body) = common::request(&app, "GET", "/?text=%7Btext%7D").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "{text}");
}

#[tokio::test]
async fn whitespace_only_is_returned_unchanged() {
    let app = app().await;
    let encoded = urlencoding::encode("  \t");
    let (status, _, body) = common::request(&app, "GET", &format!("/?text={encoded}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "  \t");
}

#[tokio::test]
async fn line_breaks_are_preserved_exactly() {
    let app = app().await;
    for original in ["a\r\nb\rc\nd", "a\n\nb", "a\r\n", "\r\nfirst"] {
        let encoded = urlencoding::encode(original);
        let (status, _, body) = common::request(&app, "GET", &format!("/?text={encoded}")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, original);
    }
}

#[tokio::test]
async fn plus_decodes_to_space_and_encoded_plus_survives() {
    let app = app().await;
    let (status, _, body) = common::request(&app, "GET", "/?text=hello+world%2Bagain").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello world+again");
}

#[tokio::test]
async fn malformed_percent_encoding_is_handled_leniently() {
    let app = app().await;
    let (status, _, body) = common::request(&app, "GET", "/?text=%ZZ%2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "%ZZ%2");
}

#[tokio::test]
async fn unknown_parameters_are_ignored() {
    let app = app().await;
    let (status, _, body) = common::request(&app, "GET", "/?text=hello&unknown=value").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello");
}

#[tokio::test]
async fn unsupported_pair_is_a_200_passthrough() {
    let app = app().await;
    let (status, content_type, body) =
        common::request(&app, "GET", "/?text=hello&from=en&to=zh-CN").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "text/plain; charset=utf-8");
    assert_eq!(body, "hello");
}

#[tokio::test]
async fn invalid_language_tag_is_a_200_passthrough() {
    let app = app().await;
    let (status, _, body) = common::request(&app, "GET", "/?text=hello&from=zh_CN&to=en").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello");
}

#[tokio::test]
async fn rendering_tokens_are_returned_unchanged() {
    let app = app().await;
    let original = "<color=red>Hello</color> [FF7700]world[-] <b>!</b> <br/>";
    let encoded = urlencoding::encode(original);
    let (status, _, body) = common::request(&app, "GET", &format!("/?text={encoded}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
}

#[tokio::test]
async fn malformed_rendering_token_is_a_200_passthrough() {
    let app = app().await;
    let original = "<color=red";
    let encoded = urlencoding::encode(original);
    let (status, _, body) = common::request(&app, "GET", &format!("/?text={encoded}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
}

#[tokio::test]
async fn malformed_bracket_token_is_a_200_passthrough() {
    let app = app().await;
    let original = "[FF7700";
    let encoded = urlencoding::encode(original);
    let (status, _, body) = common::request(&app, "GET", &format!("/?text={encoded}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, original);
}
