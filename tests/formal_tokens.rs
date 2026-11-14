//! Formal-style verification of the Rendering Token scanner.
//!
//! The property: for every input, `tokens::extract` either reports an error at
//! a valid byte position, or returns tokens that are exact, ordered,
//! non-overlapping slices of the source. A panic fails the property too. This
//! guards the two promises the scanner makes to the pipeline: a malformed tag
//! becomes a Passthrough Response, and a well-formed token can be compared
//! positionally with the model's answer.
//!
//! Scope: generated inputs up to proptest's case count and size bound, not a
//! proof over all strings. See `.opencode/skills/verify-token-scanner/SKILL.md`
//! for the coverage record and how to rerun.

use local_translator_proxy::tokens::{self, TokenError};
use proptest::prelude::*;

/// Assert the scanner's structural contract for one input.
///
/// A panic inside `extract` aborts this call and fails the property with the
/// input that caused it.
fn assert_structural_contract(input: &str) -> Result<(), TestCaseError> {
    match tokens::extract(input) {
        Err(TokenError { offset }) => {
            prop_assert!(
                offset < input.len(),
                "error offset {offset} is past the end of {input:?}"
            );
            prop_assert!(
                input.is_char_boundary(offset),
                "error offset {offset} is not a char boundary in {input:?}"
            );
            let rest = &input[offset..];
            prop_assert!(
                rest.starts_with('<') || rest.starts_with('['),
                "error offset {offset} does not point at a tag opener in {input:?}"
            );
        }
        Ok(tokens) => {
            let mut previous_end = 0;
            for token in tokens {
                prop_assert!(
                    token.start >= previous_end,
                    "token {:?} overlaps or is out of order in {input:?}",
                    token.text
                );
                let end = token.start + token.text.len();
                prop_assert_eq!(
                    input.get(token.start..end),
                    Some(token.text.as_str()),
                    "token {:?} is not the source slice at {} in {:?}",
                    token.text,
                    token.start,
                    input
                );
                let first = token
                    .text
                    .chars()
                    .next()
                    .expect("a returned token is never empty");
                prop_assert!(
                    matches!(first, '<' | '[' | '\\'),
                    "token {:?} does not start with a token opener in {input:?}",
                    token.text
                );
                if first == '\\' {
                    prop_assert!(
                        matches!(token.text.as_str(), "\\r\\n" | "\\r" | "\\n"),
                        "escaped token {:?} is not a supported escaped break in {input:?}",
                        token.text
                    );
                }
                previous_end = end;
            }
        }
    }

    Ok(())
}

/// Arbitrary Unicode text, including control characters and lone openers.
fn arbitrary_text() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 0..64).prop_map(|chars| chars.into_iter().collect())
}

/// Tag-shaped fragments recombined to reach every parser branch.
fn tag_corpus() -> impl Strategy<Value = String> {
    let fragment = prop_oneof![
        Just("<color=red>".to_string()),
        Just("</color>".to_string()),
        Just("<size=24>".to_string()),
        Just("</size>".to_string()),
        Just("<b>".to_string()),
        Just("</b>".to_string()),
        Just("<br/>".to_string()),
        Just("<br />".to_string()),
        Just("<quad width=2>".to_string()),
        Just("<sprite name=\"icon\"/>".to_string()),
        Just("<sprite name=>".to_string()),
        Just("<b class=\"x\">".to_string()),
        Just("[FF7700]".to_string()),
        Just("[-]".to_string()),
        Just("[FF7700AA]".to_string()),
        Just("[FF7700".to_string()),
        Just("\\r\\n".to_string()),
        Just("\\r".to_string()),
        Just("\\n".to_string()),
        Just("你好".to_string()),
        Just("b".to_string()),
        Just("color".to_string()),
        Just("F0".to_string()),
        Just("<".to_string()),
        Just(">".to_string()),
        Just("[".to_string()),
        Just("]".to_string()),
        Just("\\".to_string()),
        Just("=".to_string()),
        Just(" ".to_string()),
        Just("\r".to_string()),
        Just("\n".to_string()),
        any::<char>().prop_map(|character| character.to_string()),
    ];

    prop::collection::vec(fragment, 0..16).prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_text_keeps_the_scanner_contract(input in arbitrary_text()) {
        assert_structural_contract(&input)?;
    }

    #[test]
    fn tag_corpus_keeps_the_scanner_contract(input in tag_corpus()) {
        assert_structural_contract(&input)?;
    }
}
