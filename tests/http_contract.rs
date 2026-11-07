use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use local_translator_proxy::build_router;
use local_translator_proxy::config::Config;
use tower::ServiceExt;

fn app() -> Router {
    build_router(Config::default()).expect("router builds")
}

async fn get(uri: &str) -> (StatusCode, String, String) {
    request("GET", uri).await
}

async fn request(method: &str, uri: &str) -> (StatusCode, String, String) {
    let response = app()
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

#[test]
fn fixture_request_returns_plain_text() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let (status, content_type, body) = runtime.block_on(get(
        "/?text=%e4%bd%a0%e5%a5%bd%ef%bc%8c%e6%97%85%e8%a1%8c%e8%80%85%e3%80%82&from=zh-CN&to=en",
    ));

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "text/plain; charset=utf-8");
    assert_eq!(body, "你好，旅行者。");
}

#[test]
fn any_path_is_accepted() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let (status, _, body) = runtime.block_on(get("/some/deep/path?text=hello&from=zh-CN&to=en"));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello");
}

#[test]
fn verb_is_not_validated() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let (status, _, body) = runtime.block_on(request("POST", "/?text=hello"));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello");
}

#[test]
fn missing_text_returns_empty_200() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let (status, _, body) = runtime.block_on(get("/?from=zh-CN&to=en"));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "");
}

#[test]
fn empty_text_returns_empty_200() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let (status, _, body) = runtime.block_on(get("/?text=&from=zh-CN&to=en"));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "");
}

#[test]
fn placeholder_is_returned_unchanged() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let (status, _, body) = runtime.block_on(get("/?text=%7Btext%7D"));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "{text}");
}

#[test]
fn whitespace_only_is_returned_unchanged() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let encoded = urlencoding::encode("  \t");
    let (status, _, body) = runtime.block_on(get(&format!("/?text={encoded}")));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "  \t");
}

#[test]
fn line_breaks_are_preserved_exactly() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    for original in ["a\r\nb\rc\nd", "a\n\nb", "a\r\n", "\r\nfirst"] {
        let encoded = urlencoding::encode(original);
        let (status, _, body) = runtime.block_on(get(&format!("/?text={encoded}")));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, original);
    }
}

#[test]
fn plus_decodes_to_space_and_encoded_plus_survives() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let (status, _, body) = runtime.block_on(get("/?text=hello+world%2Bagain"));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello world+again");
}

#[test]
fn malformed_percent_encoding_is_handled_leniently() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let (status, _, body) = runtime.block_on(get("/?text=%ZZ%2"));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "%ZZ%2");
}

#[test]
fn unknown_parameters_are_ignored() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let (status, _, body) = runtime.block_on(get("/?text=hello&unknown=value"));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello");
}

#[test]
fn unsupported_pair_is_a_200_passthrough() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let (status, content_type, body) = runtime.block_on(get("/?text=hello&from=en&to=zh-CN"));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "text/plain; charset=utf-8");
    assert_eq!(body, "hello");
}

#[test]
fn invalid_language_tag_is_a_200_passthrough() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let (status, _, body) = runtime.block_on(get("/?text=hello&from=zh_CN&to=en"));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello");
}
