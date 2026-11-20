//! Source-owned recovery plan for a line with Rendering Tokens.
//!
//! [`LinePlan`] partitions one physical line into text spans and verbatim
//! pieces. Only the text spans go to the model. The verbatim pieces come from
//! the source and never pass through the model. [`LinePlan::render`] is the
//! boundary that refuses unsafe model output.

use crate::tokens::{self, Token, TokenError};

/// Why a line cannot be rebuilt safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryError {
    /// The line has no non-blank text to translate.
    NoText,
    /// The supplied tokens do not describe this line.
    InconsistentTokens,
    /// The model returned the wrong number of span translations.
    CountMismatch,
    /// A span translation injected a real line break.
    InjectedLineBreak,
    /// A span translation is blank.
    EmptyTranslation,
    /// A rebuilt line contains a malformed Rendering Token.
    Malformed(TokenError),
    /// The rebuilt line's tokens differ from the source tokens.
    TokenDrift,
}

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoText => write!(formatter, "no text to recover"),
            Self::InconsistentTokens => write!(formatter, "inconsistent source tokens"),
            Self::CountMismatch => write!(formatter, "span translation count mismatch"),
            Self::InjectedLineBreak => write!(formatter, "span injected a line break"),
            Self::EmptyTranslation => write!(formatter, "empty span translation"),
            Self::Malformed(error) => write!(formatter, "{error}"),
            Self::TokenDrift => write!(formatter, "rendering token drift"),
        }
    }
}

/// One piece of a line. `Text` is model input. `Verbatim` is a source copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Piece<'a> {
    Text(&'a str),
    Verbatim(&'a str),
}

/// An ordered partition of one line around its Rendering Tokens.
///
/// Every byte of the source appears in exactly one piece, once, in source
/// order. A successful `of` proves that invariant before any model call.
#[derive(Debug, Clone)]
pub(crate) struct LinePlan<'a> {
    pieces: Vec<Piece<'a>>,
    tokens: Vec<Token>,
    text_count: usize,
}

impl<'a> LinePlan<'a> {
    /// Partition `line` around `tokens`.
    pub(crate) fn of(line: &'a str, tokens: Vec<Token>) -> Result<Self, RecoveryError> {
        let mut pieces = Vec::with_capacity(tokens.len() * 2 + 1);
        let mut cursor = 0;
        let mut text_count = 0;

        for token in &tokens {
            let start = token.start;
            let end = start
                .checked_add(token.text.len())
                .ok_or(RecoveryError::InconsistentTokens)?;
            if start < cursor || end > line.len() {
                return Err(RecoveryError::InconsistentTokens);
            }
            let gap = line
                .get(cursor..start)
                .ok_or(RecoveryError::InconsistentTokens)?;
            if !gap.is_empty() {
                if gap.trim().is_empty() {
                    pieces.push(Piece::Verbatim(gap));
                } else {
                    pieces.push(Piece::Text(gap));
                    text_count += 1;
                }
            }
            let verbatim = line
                .get(start..end)
                .ok_or(RecoveryError::InconsistentTokens)?;
            if verbatim != token.text {
                return Err(RecoveryError::InconsistentTokens);
            }
            pieces.push(Piece::Verbatim(verbatim));
            cursor = end;
        }

        let gap = &line[cursor..];
        if !gap.is_empty() {
            if gap.trim().is_empty() {
                pieces.push(Piece::Verbatim(gap));
            } else {
                pieces.push(Piece::Text(gap));
                text_count += 1;
            }
        }

        if text_count == 0 {
            return Err(RecoveryError::NoText);
        }
        Ok(Self {
            pieces,
            tokens,
            text_count,
        })
    }

    /// The non-blank source spans, in source order.
    pub(crate) fn texts(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.pieces.iter().filter_map(|piece| match piece {
            Piece::Text(text) => Some(*text),
            Piece::Verbatim(_) => None,
        })
    }

    /// Rebuild the line and refuse any unsafe model output.
    pub(crate) fn render(&self, translated: &[String]) -> Result<String, RecoveryError> {
        if translated.len() != self.text_count {
            return Err(RecoveryError::CountMismatch);
        }
        if translated.iter().any(|text| text.contains(['\r', '\n'])) {
            return Err(RecoveryError::InjectedLineBreak);
        }
        if translated.iter().any(|text| text.trim().is_empty()) {
            return Err(RecoveryError::EmptyTranslation);
        }

        let mut replacements = translated.iter();
        let mut output = String::new();
        for piece in &self.pieces {
            match piece {
                Piece::Text(_) => {
                    let text = replacements.next().ok_or(RecoveryError::CountMismatch)?;
                    output.push_str(text);
                }
                Piece::Verbatim(text) => output.push_str(text),
            }
        }

        let rendered_tokens = tokens::extract(&output).map_err(RecoveryError::Malformed)?;
        if !tokens::matches(&self.tokens, &rendered_tokens) {
            return Err(RecoveryError::TokenDrift);
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(source: &str) -> LinePlan<'_> {
        let tokens = tokens::extract(source).expect("source tokens");
        LinePlan::of(source, tokens).expect("line plan")
    }

    #[test]
    fn keeps_tokens_verbatim_and_translates_only_text() {
        let source = "[FF7700]ＴＡＢ[-]键可切换视角的距离";
        let plan = plan(source);

        let texts: Vec<&str> = plan.texts().collect();
        assert_eq!(texts, vec!["ＴＡＢ", "键可切换视角的距离"]);

        let rendered = plan
            .render(&["The TAB key".into(), " changes the view distance".into()])
            .expect("render");
        assert_eq!(rendered, "[FF7700]The TAB key[-] changes the view distance");
    }

    #[test]
    fn preserves_whitespace_between_tokens() {
        let source = "<b> 一 </b> <i>二</i>";
        let plan = plan(source);
        let texts: Vec<&str> = plan.texts().collect();
        assert_eq!(texts, vec![" 一 ", "二"]);

        let rendered = plan
            .render(&[" one ".into(), "two".into()])
            .expect("render");
        assert_eq!(rendered, "<b> one </b> <i>two</i>");
    }

    #[test]
    fn rejects_a_line_with_only_tokens() {
        let source = "[FF7700][-]";
        let tokens = tokens::extract(source).expect("source tokens");
        assert_eq!(
            LinePlan::of(source, tokens).err(),
            Some(RecoveryError::NoText)
        );
    }

    #[test]
    fn rejects_inconsistent_tokens() {
        let source = "[FF7700]ＴＡＢ[-]键";
        let mut tokens = tokens::extract(source).expect("source tokens");
        tokens[0].start += 1;
        assert_eq!(
            LinePlan::of(source, tokens).err(),
            Some(RecoveryError::InconsistentTokens)
        );
    }

    #[test]
    fn rejects_a_wrong_translation_count() {
        let plan = plan("[FF7700]ＴＡＢ[-]键");
        assert_eq!(
            plan.render(&["TAB".into()]).err(),
            Some(RecoveryError::CountMismatch)
        );
    }

    #[test]
    fn rejects_an_injected_line_break() {
        let plan = plan("[FF7700]ＴＡＢ[-]键");
        assert_eq!(
            plan.render(&["TAB\nkey".into(), "distance".into()]).err(),
            Some(RecoveryError::InjectedLineBreak)
        );
    }

    #[test]
    fn rejects_a_blank_translation() {
        let plan = plan("[FF7700]ＴＡＢ[-]键");
        assert_eq!(
            plan.render(&["  ".into(), "distance".into()]).err(),
            Some(RecoveryError::EmptyTranslation)
        );
    }

    #[test]
    fn rejects_a_malformed_rebuilt_token() {
        let plan = plan("<b>Hello</b>");
        assert!(matches!(
            plan.render(&["<b class=\"x\">Hello".into()]).err(),
            Some(RecoveryError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_token_drift() {
        let plan = plan("[FF7700]ＴＡＢ[-]键");
        assert_eq!(
            plan.render(&["[FF0000]TAB".into(), "distance[-]".into()])
                .err(),
            Some(RecoveryError::TokenDrift)
        );
    }
}
