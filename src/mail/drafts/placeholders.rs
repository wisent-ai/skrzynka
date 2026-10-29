//! Placeholders in a template's text: `{{key}}`, a key of letters, digits,
//! `_` and `-`, spaces inside the braces aside. Filling a template puts the
//! value given for each key in place of every placeholder naming it. Braces
//! around anything else are text and stay as written.

use crate::error::AppError;
use std::collections::BTreeMap;

const OPEN: &str = "{{";
const CLOSE: &str = "}}";

/// One stretch of a template's text: written text, or a placeholder's key.
enum Piece<'a> {
    Text(&'a str),
    Key(&'a str),
}

/// The template's text cut into written text and placeholders, in order.
fn pieces(body: &str) -> Vec<Piece<'_>> {
    let mut parts = body.split(OPEN);
    let mut pieces: Vec<Piece<'_>> = parts.next().map(Piece::Text).into_iter().collect();
    for part in parts {
        match part.split_once(CLOSE) {
            Some((inside, after)) if is_key(inside.trim()) => {
                pieces.push(Piece::Key(inside.trim()));
                pieces.push(Piece::Text(after));
            }
            _ => {
                pieces.push(Piece::Text(OPEN));
                pieces.push(Piece::Text(part));
            }
        }
    }
    pieces
}

fn is_key(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_' || character == '-')
}

/// Each key the text's placeholders name, in order of first appearance, once.
pub fn placeholders(body: &str) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for piece in pieces(body) {
        if let Piece::Key(key) = piece {
            if !keys.iter().any(|known| known == key) {
                keys.push(key.to_string());
            }
        }
    }
    keys
}

/// Each of a template's texts (its subject and its body) with every
/// placeholder replaced by the value given for its key, the keys checked
/// across all of them together. A key with no value is
/// `TEMPLATE_VALUE_MISSING`; a value for a key no text names is
/// `TEMPLATE_VALUE_UNUSED`, so a misspelt key is never silently left out.
pub fn fill<const N: usize>(
    name: &str,
    texts: [&str; N],
    values: &BTreeMap<String, String>,
) -> Result<[String; N], AppError> {
    let mut keys: Vec<String> = Vec::new();
    for key in texts.iter().flat_map(|text| placeholders(text)) {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let missing: Vec<&String> = keys
        .iter()
        .filter(|key| !values.contains_key(*key))
        .collect();
    if !missing.is_empty() {
        return Err(AppError::invalid(
            "TEMPLATE_VALUE_MISSING",
            format!("template {name:?} needs a value for {missing:?}; give each as KEY=VALUE"),
        ));
    }
    let unused: Vec<&String> = values.keys().filter(|key| !keys.contains(key)).collect();
    if !unused.is_empty() {
        return Err(AppError::invalid(
            "TEMPLATE_VALUE_UNUSED",
            format!("template {name:?} has no placeholder for {unused:?}; it names {keys:?}"),
        ));
    }
    Ok(texts.map(|text| {
        pieces(text)
            .into_iter()
            .map(|piece| match piece {
                Piece::Text(text) => text,
                Piece::Key(key) => values[key].as_str(),
            })
            .collect()
    }))
}
