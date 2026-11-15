use std::collections::HashSet;

use local_translator_proxy::tokens;
use serde_json::Value;

const CORPUS: &str = include_str!("../corpus/zh-en.json");

const ALL_CATEGORIES: [&str; 9] = [
    "dialogue",
    "punctuation",
    "ui",
    "combat",
    "item",
    "colour",
    "markup",
    "line-break",
    "square-bracket",
];

const REQUIRED_CATEGORIES: [&str; 6] = [
    "dialogue",
    "punctuation",
    "colour",
    "markup",
    "line-break",
    "square-bracket",
];

fn corpus() -> Value {
    serde_json::from_str(CORPUS).expect("corpus/zh-en.json parses")
}

fn token_texts(text: &str) -> Vec<String> {
    tokens::extract(text)
        .expect("corpus token extraction succeeds")
        .into_iter()
        .map(|token| token.text)
        .collect()
}

#[test]
fn corpus_entries_are_well_formed_and_match_the_token_scanner() {
    let corpus = corpus();
    assert_eq!(corpus["version"], 1, "corpus version");
    assert_eq!(corpus["from"], "zh-CN", "corpus source language");
    assert_eq!(corpus["to"], "en", "corpus target language");

    let entries = corpus["entries"].as_array().expect("entries is an array");
    assert!(entries.len() >= 20, "corpus has at least 20 entries");

    let mut ids = HashSet::new();

    for entry in entries {
        let id = entry["id"].as_str().expect("entry id is a string");
        assert!(ids.insert(id), "entry id {id} is unique");

        let category = entry["category"].as_str().expect("category is a string");
        assert!(
            ALL_CATEGORIES.contains(&category),
            "{id} has a documented category, got {category}"
        );

        let source = entry["source"].as_str().expect("source is a string");
        let reference = entry["reference"].as_str().expect("reference is a string");
        assert!(!source.trim().is_empty(), "{id} has a source");
        assert!(!reference.trim().is_empty(), "{id} has a reference");
        assert!(
            !source.contains(['\r', '\n']),
            "{id} source is one physical line"
        );
        assert!(
            !reference.contains(['\r', '\n']),
            "{id} reference is one physical line"
        );

        let listed: Vec<String> = entry["must_preserve"]
            .as_array()
            .expect("must_preserve is an array")
            .iter()
            .map(|token| {
                token
                    .as_str()
                    .expect("must_preserve token is a string")
                    .to_string()
            })
            .collect();

        assert_eq!(
            listed,
            token_texts(source),
            "{id} must_preserve matches the tokens the scanner extracts from source"
        );
        assert_eq!(
            listed,
            token_texts(reference),
            "{id} must_preserve matches the tokens the scanner extracts from reference"
        );
    }
}

#[test]
fn corpus_covers_every_required_category() {
    let corpus = corpus();
    let categories: HashSet<&str> = corpus["entries"]
        .as_array()
        .expect("entries is an array")
        .iter()
        .map(|entry| entry["category"].as_str().expect("category is a string"))
        .collect();

    for required in REQUIRED_CATEGORIES {
        assert!(
            categories.contains(required),
            "corpus covers the {required} category"
        );
    }
}
