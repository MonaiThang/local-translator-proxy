//! Rendering Token extraction and positional verification.
//!
//! The scanner recognises the v1 minimal tag set and escaped line breaks. A tag
//! that starts like a known tag but is malformed is treated as ambiguous, and a
//! translated token list that differs in content, order, or count from the
//! source causes a Passthrough Response.

/// A Rendering Token found in a line of game text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The exact source substring.
    pub text: String,
    /// Byte offset in the line.
    pub start: usize,
}

/// A malformed or ambiguous Rendering Token, located by byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenError {
    /// Byte offset where the ambiguous tag starts.
    pub offset: usize,
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "malformed rendering token at byte {}",
            self.offset
        )
    }
}

impl std::error::Error for TokenError {}

/// Extract the v1 minimal Rendering Token set in source order.
///
/// Unknown angle-bracket sequences are treated as ordinary text. A sequence
/// that starts like one of the supported tags but is malformed is ambiguous and
/// returns an error, which the pipeline turns into a Passthrough Response.
pub fn extract(text: &str) -> Result<Vec<Token>, TokenError> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'<' {
            if let Some((end, token)) = parse_tag(text, index) {
                tokens.push(Token {
                    text: token,
                    start: index,
                });
                index = end;
                continue;
            }
            if looks_like_known_tag(&text[index..]) {
                return Err(TokenError { offset: index });
            }
        } else if bytes[index] == b'\\'
            && let Some((end, token)) = parse_escaped_break(text, index)
        {
            tokens.push(Token {
                text: token,
                start: index,
            });
            index = end;
            continue;
        }
        index += 1;
    }

    Ok(tokens)
}

/// Positional comparison: same tokens, same order, same count, byte-for-byte.
pub fn matches(source: &[Token], translated: &[Token]) -> bool {
    source.len() == translated.len()
        && source
            .iter()
            .zip(translated)
            .all(|(left, right)| left.text == right.text)
}

fn parse_tag(text: &str, start: usize) -> Option<(usize, String)> {
    // Closing tags: </color>, </size>, </b>, </i>, </u>, </s>.
    for name in ["color", "size", "b", "i", "u", "s"] {
        let prefix = format!("</{name}");
        if starts_with_ignore_case_at(text, start, &prefix) {
            let after = skip_whitespace(text, start + prefix.len());
            if text[after..].starts_with('>') {
                let end = after + 1;
                return Some((end, text[start..end].to_string()));
            }
        }
    }

    // <br>, <br/>, and <br />.
    if starts_with_ignore_case_at(text, start, "<br") {
        let after = skip_whitespace(text, start + "<br".len());
        let after = if text[after..].starts_with('/') {
            skip_whitespace(text, after + 1)
        } else {
            after
        };
        if text[after..].starts_with('>') {
            let end = after + 1;
            return Some((end, text[start..end].to_string()));
        }
    }

    // Simple paired opening tags: <b>, <i>, <u>, <s>.
    for name in ["b", "i", "u", "s"] {
        let prefix = format!("<{name}");
        if starts_with_ignore_case_at(text, start, &prefix) {
            let after = skip_whitespace(text, start + prefix.len());
            if text[after..].starts_with('>') {
                let end = after + 1;
                return Some((end, text[start..end].to_string()));
            }
        }
    }

    // <color=...> and <size=...>.
    for name in ["color", "size"] {
        let prefix = format!("<{name}");
        if starts_with_ignore_case_at(text, start, &prefix) {
            let mut cursor = skip_whitespace(text, start + prefix.len());
            if text[cursor..].starts_with('=') {
                cursor = skip_whitespace(text, cursor + 1);
                if let Some(end) = value_end(text, cursor) {
                    return Some((end, text[start..end].to_string()));
                }
            }
        }
    }

    // <sprite name=...>.
    if starts_with_ignore_case_at(text, start, "<sprite") {
        let mut cursor = skip_whitespace(text, start + "<sprite".len());
        if starts_with_ignore_case_at(text, cursor, "name") {
            cursor = skip_whitespace(text, cursor + "name".len());
            if text[cursor..].starts_with('=') {
                cursor = skip_whitespace(text, cursor + 1);
                if let Some(end) = value_end(text, cursor) {
                    return Some((end, text[start..end].to_string()));
                }
            }
        }
    }

    // <quad ...>; an empty attribute list is allowed.
    if starts_with_ignore_case_at(text, start, "<quad") {
        let cursor = skip_whitespace(text, start + "<quad".len());
        if let Some(relative_end) = text[cursor..].find('>') {
            let end = cursor + relative_end + 1;
            return Some((end, text[start..end].to_string()));
        }
    }

    None
}

/// Find a non-empty value ending at `>`, returning the index just past it.
fn value_end(text: &str, start: usize) -> Option<usize> {
    let relative_end = text[start..].find('>')?;
    if text[start..start + relative_end].trim().is_empty() {
        return None;
    }
    Some(start + relative_end + 1)
}

fn parse_escaped_break(text: &str, start: usize) -> Option<(usize, String)> {
    let rest = &text[start..];
    for token in ["\\r\\n", "\\r", "\\n"] {
        if rest.starts_with(token) {
            return Some((start + token.len(), token.to_string()));
        }
    }
    None
}

fn starts_with_ignore_case_at(text: &str, start: usize, prefix: &str) -> bool {
    text.get(start..start + prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}

fn skip_whitespace(text: &str, mut index: usize) -> usize {
    while let Some(character) = text[index..].chars().next() {
        if !character.is_whitespace() {
            break;
        }
        index += character.len_utf8();
    }
    index
}

fn looks_like_known_tag(rest: &str) -> bool {
    let after_open = match rest.strip_prefix('<') {
        Some(after) => after,
        None => return false,
    };

    let (names, closing): (&[&str], bool) = match after_open.strip_prefix('/') {
        Some(_) => (&["color", "size", "b", "i", "u", "s"], true),
        None => (
            &["color", "size", "sprite", "quad", "br", "b", "i", "u", "s"],
            false,
        ),
    };

    names.iter().any(|name| {
        let Some(after_name) = strip_prefix_ignore_case(after_open, name) else {
            return false;
        };
        let Some(next) = after_name.chars().next() else {
            return false;
        };

        if closing {
            next == '>' || next.is_whitespace()
        } else {
            next == '>' || next == '=' || next == '/' || next.is_whitespace()
        }
    })
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let candidate = text.get(..prefix.len())?;
    if candidate.eq_ignore_ascii_case(prefix) {
        Some(&text[prefix.len()..])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(tokens: &[Token]) -> Vec<&str> {
        tokens.iter().map(|token| token.text.as_str()).collect()
    }

    #[test]
    fn extracts_tags_in_source_order() {
        let tokens = extract("<color=red>Hi</color> <b>bold</b> <br/>").expect("tokens");
        assert_eq!(
            texts(&tokens),
            vec!["<color=red>", "</color>", "<b>", "</b>", "<br/>"]
        );
    }

    #[test]
    fn extracts_size_sprite_and_quad_tokens() {
        let tokens =
            extract(r#"<size=24>x</size><sprite name="icon"/><quad width=2>y"#).expect("tokens");
        assert_eq!(
            texts(&tokens),
            vec![
                "<size=24>",
                "</size>",
                r#"<sprite name="icon"/>"#,
                "<quad width=2>"
            ]
        );
    }

    #[test]
    fn extracts_escaped_line_breaks() {
        let tokens = extract(r"a\r\nb\nc\rd").expect("tokens");
        assert_eq!(texts(&tokens), vec![r"\r\n", r"\n", r"\r"]);
    }

    #[test]
    fn known_tags_are_case_insensitive_but_captured_exactly() {
        let tokens = extract("<COLOR=red>x</Color>").expect("tokens");
        assert_eq!(texts(&tokens), vec!["<COLOR=red>", "</Color>"]);
    }

    #[test]
    fn unknown_angle_brackets_are_text() {
        let tokens = extract("a < b, <i18n>, <foo bar>").expect("tokens");
        assert!(tokens.is_empty());
    }

    #[test]
    fn malformed_known_tag_is_an_error() {
        assert!(extract("<color=red").is_err());
        assert!(extract("<b class=\"x\">").is_err());
        assert!(extract("<sprite name=>").is_err());
    }

    #[test]
    fn positional_matching_requires_same_count_and_order() {
        let source = extract("<b>a</b> <i>b</i>").expect("tokens");
        let same = extract("x <b>y</b> <i>z</i>").expect("tokens");
        let reordered = extract("<i>b</i> <b>a</b>").expect("tokens");
        let missing = extract("<b>a</b>").expect("tokens");

        assert!(matches(&source, &same));
        assert!(!matches(&source, &reordered));
        assert!(!matches(&source, &missing));
    }

    #[test]
    fn extra_token_is_a_mismatch() {
        let source = extract("no tokens").expect("tokens");
        let translated = extract("added <b>bold</b>").expect("tokens");
        assert!(!matches(&source, &translated));
    }

    #[test]
    fn whitespace_inside_tags_is_preserved() {
        let tokens = extract("<br />\n<b >").expect("tokens");
        assert_eq!(texts(&tokens), vec!["<br />", "<b >"]);
    }
}
