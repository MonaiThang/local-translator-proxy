use std::collections::HashMap;

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::pipeline::AppState;

/// Handle the game's request on any path and any verb.
///
/// The game only uses the query string; parsing is deliberately manual so that
/// malformed input becomes a Passthrough Response instead of an axum rejection
/// (400) that would stall the game.
pub async fn handle(State(state): State<AppState>, request: Request) -> Response {
    let params = parse_query(request.uri().query().unwrap_or_default());
    let text = params.get("text").cloned().unwrap_or_default();
    let from = params.get("from").cloned();
    let to = params.get("to").cloned();

    let translated = state
        .pipeline
        .translate(&text, from.as_deref(), to.as_deref())
        .await;

    plain_text(translated)
}

/// Parse an application/x-www-form-urlencoded query string.
///
/// - Decodes `%XX` as UTF-8 and treats `+` as a space (matching .NET's
///   `HttpUtility.ParseQueryString`, which the game's original proxy used).
/// - Leaves malformed percent sequences untouched.
/// - The first occurrence of a key wins.
pub fn parse_query(raw: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();

    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        params
            .entry(decode_component(key))
            .or_insert_with(|| decode_component(value));
    }

    params
}

fn decode_component(value: &str) -> String {
    let replaced = value.replace('+', " ");
    let decoded = urlencoding::decode_binary(replaced.as_bytes());
    String::from_utf8_lossy(&decoded).into_owned()
}

fn plain_text(body: String) -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        )],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_percent_encoded_utf8() {
        let params = parse_query("text=%e4%bd%a0%e5%a5%bd&from=zh-CN&to=en");
        assert_eq!(params.get("text").map(String::as_str), Some("你好"));
        assert_eq!(params.get("from").map(String::as_str), Some("zh-CN"));
        assert_eq!(params.get("to").map(String::as_str), Some("en"));
    }

    #[test]
    fn decodes_plus_as_space_but_keeps_encoded_plus() {
        let params = parse_query("text=hello+world%2Bagain");
        assert_eq!(
            params.get("text").map(String::as_str),
            Some("hello world+again")
        );
    }

    #[test]
    fn leaves_malformed_percent_sequences_untouched() {
        let params = parse_query("text=%ZZ%2");
        assert_eq!(params.get("text").map(String::as_str), Some("%ZZ%2"));
    }

    #[test]
    fn first_duplicate_key_wins() {
        let params = parse_query("text=first&text=second");
        assert_eq!(params.get("text").map(String::as_str), Some("first"));
    }

    #[test]
    fn missing_value_is_empty() {
        let params = parse_query("text");
        assert_eq!(params.get("text").map(String::as_str), Some(""));
    }
}
