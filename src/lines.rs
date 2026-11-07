//! Translation Unit line handling.
//!
//! [`split_lines`] keeps the exact separator after each line so [`reassemble`]
//! can rebuild the original text byte-for-byte. The pipeline skips blank lines
//! and never sends them to the model.

/// One physical line of a Translation Unit.
///
/// `separator` is the exact sequence that followed the line in the original
/// request (`\r\n`, `\r`, `\n`, or empty for the final line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment<'a> {
    pub text: &'a str,
    pub separator: &'a str,
}

/// Split on `\r\n`, `\r`, or `\n`, keeping every separator verbatim.
pub fn split_lines(text: &str) -> Vec<Segment<'_>> {
    let bytes = text.as_bytes();
    let mut segments = Vec::new();
    let mut start = 0;
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                segments.push(Segment {
                    text: &text[start..index],
                    separator: &text[index..index + 2],
                });
                index += 2;
                start = index;
            }
            b'\r' | b'\n' => {
                segments.push(Segment {
                    text: &text[start..index],
                    separator: &text[index..index + 1],
                });
                index += 1;
                start = index;
            }
            _ => index += 1,
        }
    }

    segments.push(Segment {
        text: &text[start..],
        separator: "",
    });
    segments
}

/// Rebuild a Translation Unit, preserving every separator.
pub fn reassemble(segments: &[Segment<'_>]) -> String {
    let mut output = String::new();
    for segment in segments {
        output.push_str(segment.text);
        output.push_str(segment.separator);
    }
    output
}

/// Whitespace-only text is never sent to the Model Runtime.
pub fn is_blank(text: &str) -> bool {
    text.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_mixed_separators() {
        let input = "a\r\nb\rc\nd";
        let segments = split_lines(input);
        assert_eq!(
            segments,
            vec![
                Segment {
                    text: "a",
                    separator: "\r\n"
                },
                Segment {
                    text: "b",
                    separator: "\r"
                },
                Segment {
                    text: "c",
                    separator: "\n"
                },
                Segment {
                    text: "d",
                    separator: ""
                },
            ]
        );
        assert_eq!(reassemble(&segments), input);
    }

    #[test]
    fn trailing_separator_produces_empty_final_line() {
        let input = "a\r\n";
        let segments = split_lines(input);
        assert_eq!(
            segments,
            vec![
                Segment {
                    text: "a",
                    separator: "\r\n"
                },
                Segment {
                    text: "",
                    separator: ""
                },
            ]
        );
        assert_eq!(reassemble(&segments), input);
    }

    #[test]
    fn empty_input_round_trips() {
        let segments = split_lines("");
        assert_eq!(
            segments,
            vec![Segment {
                text: "",
                separator: ""
            }]
        );
        assert_eq!(reassemble(&segments), "");
    }

    #[test]
    fn unicode_is_not_split_mid_character() {
        let input = "你好\n世界";
        let segments = split_lines(input);
        assert_eq!(
            segments,
            vec![
                Segment {
                    text: "你好",
                    separator: "\n"
                },
                Segment {
                    text: "世界",
                    separator: ""
                },
            ]
        );
        assert_eq!(reassemble(&segments), input);
    }

    #[test]
    fn blank_detection_uses_unicode_whitespace() {
        assert!(is_blank(""));
        assert!(is_blank(" \t\r\n"));
        assert!(!is_blank(" x "));
    }
}
