use crate::config::LanguagesConfig;

/// Normalize a BCP47-like tag for comparison.
///
/// v1 has no language aliases; the only normalization is trimming whitespace
/// and lowercasing.
pub fn normalize_tag(tag: &str) -> String {
    tag.trim().to_lowercase()
}

/// A deliberately small BCP47-like check: non-empty, hyphen-separated,
/// ASCII-alphanumeric subtags. It rejects aliases such as `zh_CN` and
/// whitespace inside a tag.
pub fn is_language_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        })
}

/// Resolve the requested pair against the configured defaults and allowlist.
///
/// Missing or empty values fall back to the configured defaults. The pair is
/// returned in normalized form only when it is supported.
pub fn resolve_pair(
    config: &LanguagesConfig,
    from: Option<&str>,
    to: Option<&str>,
) -> Option<(String, String)> {
    let from = from
        .map(normalize_tag)
        .filter(|tag| !tag.is_empty())
        .unwrap_or_else(|| normalize_tag(&config.default_from));
    let to = to
        .map(normalize_tag)
        .filter(|tag| !tag.is_empty())
        .unwrap_or_else(|| normalize_tag(&config.default_to));

    if !is_language_tag(&from) || !is_language_tag(&to) {
        return None;
    }
    if !config.supports(&from, &to) {
        return None;
    }

    Some((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn languages() -> LanguagesConfig {
        LanguagesConfig {
            supported_pairs: vec![
                ("zh-cn".to_string(), "en".to_string()),
                ("ja".to_string(), "en".to_string()),
            ],
            default_from: "zh-cn".to_string(),
            default_to: "en".to_string(),
        }
    }

    #[test]
    fn normalization_trims_and_lowercases() {
        assert_eq!(normalize_tag("  zh-CN "), "zh-cn");
    }

    #[test]
    fn missing_values_use_defaults() {
        assert_eq!(
            resolve_pair(&languages(), None, None),
            Some(("zh-cn".to_string(), "en".to_string()))
        );
        assert_eq!(
            resolve_pair(&languages(), Some(""), Some("")),
            Some(("zh-cn".to_string(), "en".to_string()))
        );
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(
            resolve_pair(&languages(), Some("ZH-cn"), Some("EN")),
            Some(("zh-cn".to_string(), "en".to_string()))
        );
    }

    #[test]
    fn unsupported_pairs_are_rejected() {
        assert_eq!(resolve_pair(&languages(), Some("en"), Some("zh-cn")), None);
        assert_eq!(resolve_pair(&languages(), Some("fr"), Some("en")), None);
    }

    #[test]
    fn aliases_are_rejected() {
        assert_eq!(resolve_pair(&languages(), Some("zh_CN"), Some("en")), None);
        assert_eq!(
            resolve_pair(&languages(), Some("chinese"), Some("en")),
            None
        );
    }
}
