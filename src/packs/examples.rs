//! Root-search parsing: what was typed, matched against an ability's `examples` with no model
//! and no network. `50 usd to inr` teaches the shape; `75 bucks to rupees` then fills the same
//! slots. An ability whose examples don't match can opt into AI slot-filling with `slots = true`.

use serde_json::{Map, Value, json};

use super::index::Ability;
use super::types::Type;
use crate::features::calc::currency;

#[derive(Clone, Debug, PartialEq)]
enum Part {
    /// A word that must be there (or one that means the same).
    Word(String),
    /// A field of the input, and what kind of token fills it.
    Slot(String, Class),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Class {
    Number,
    Currency,
    Url,
    Choice,
    /// Whatever words are left, up to the next fixed word.
    Text,
}

/// Small words that hold a phrase together; never a value.
const GLUE: &[&str] = &["to", "in", "into", "as", "from", "of", "for", "at", "by", "with", "on", "the", "a", "an", "is", "what", "how", "much", "many", "convert", "and", "→", "->", "="];

/// Words that say the same thing between two slots.
fn same_word(a: &str, b: &str) -> bool {
    const ARROWS: &[&str] = &["to", "in", "into", "as", "→", "->", "="];
    a == b || ARROWS.contains(&a) && ARROWS.contains(&b)
}

/// Splits what was typed into tokens: `€20` is a symbol and a number, `50usd` a number and a word.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split_whitespace() {
        let word = word.trim_matches(|c: char| matches!(c, ',' | '?' | '!' | ';'));
        let mut current = String::new();
        let mut numeric: Option<bool> = None;
        for c in word.chars() {
            if currency::is_symbol(c) {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                out.push(c.to_string());
                numeric = None;
                continue;
            }
            let is_num = c.is_ascii_digit() || (c == '.' || c == ',') && numeric == Some(true);
            if numeric.is_some_and(|n| n != is_num) && !current.is_empty() && word.contains("://").eq(&false) {
                out.push(std::mem::take(&mut current));
            }
            numeric = Some(is_num);
            current.push(c);
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

fn number(token: &str) -> Option<f64> {
    token.replace(',', "").parse::<f64>().ok().filter(|n| n.is_finite())
}

/// A currency's code from however it was said: `usd`, `$`, `bucks`, `rupees`.
fn currency_code(token: &str) -> Option<String> {
    let lower = token.to_lowercase();
    let known = match lower.as_str() {
        "$" | "bucks" | "buck" | "dollars" | "dollar" => Some("USD"),
        "€" | "euro" | "euros" => Some("EUR"),
        "£" | "pounds" | "pound" | "quid" => Some("GBP"),
        "¥" | "yen" => Some("JPY"),
        "₹" | "rupees" | "rupee" | "rs" => Some("INR"),
        _ => None,
    };
    if let Some(code) = known {
        return Some(code.to_string());
    }
    if lower.len() == 3 && lower.chars().all(|c| c.is_ascii_alphabetic()) && !currency::wordy(&lower) && !GLUE.contains(&lower.as_str()) {
        return Some(lower.to_uppercase());
    }
    currency::find(&lower).map(|unit| unit.symbol.clone()).filter(|s| s.len() == 3 && s.chars().all(|c| c.is_ascii_alphabetic())).map(|s| s.to_uppercase())
}

fn class_of(t: &Type) -> Class {
    match t.kind.as_str() {
        "number" | "percent" | "duration" => Class::Number,
        "currency" => Class::Currency,
        "url" => Class::Url,
        "enum" => Class::Choice,
        _ => Class::Text,
    }
}

fn fits(class: Class, token: &str, ty: Option<&Type>) -> bool {
    match class {
        Class::Number => number(token).is_some(),
        Class::Currency => currency_code(token).is_some(),
        Class::Url => token.contains("://"),
        Class::Choice => ty.is_some_and(|t| t.values.iter().any(|v| v.eq_ignore_ascii_case(token))),
        Class::Text => true,
    }
}

/// The shape an example teaches: which of its words are fixed, and which stand for a field.
fn pattern(example: &str, input: &Type) -> Vec<Part> {
    // Fields still to place, in a stable order: the order the example fills them is theirs.
    let mut free: Vec<(&String, Class)> = input.fields.iter().map(|(k, t)| (k, class_of(t))).collect();
    // `from` before `to`, `amount` first: common names in the order people say them.
    free.sort_by_key(|(k, _)| match k.as_str() {
        "amount" | "value" | "n" => 0,
        "from" | "source" => 1,
        "to" | "target" => 2,
        _ => 3,
    });
    let mut out: Vec<Part> = Vec::new();
    for token in tokens(example) {
        let lower = token.to_lowercase();
        let typed = [Class::Number, Class::Currency, Class::Url, Class::Choice].into_iter().find(|class| {
            free.iter().any(|(k, c)| c == class && fits(*class, &token, input.fields.get(*k)))
        });
        if let Some(class) = typed.filter(|_| !GLUE.contains(&lower.as_str()) || currency::is_symbol(lower.chars().next().unwrap_or(' '))) {
            let at = free.iter().position(|(k, c)| *c == class && fits(class, &token, input.fields.get(*k))).expect("found above");
            out.push(Part::Slot(free.remove(at).0.clone(), class));
        } else if GLUE.contains(&lower.as_str()) || !free.iter().any(|(_, c)| *c == Class::Text) {
            out.push(Part::Word(lower));
        } else if !matches!(out.last(), Some(Part::Slot(_, Class::Text))) {
            // Free words stand for the text field; a run of them is one value.
            let at = free.iter().position(|(_, c)| *c == Class::Text).expect("checked above");
            out.push(Part::Slot(free.remove(at).0.clone(), Class::Text));
        }
    }
    out
}

fn fill(parts: &[Part], words: &[String], input: &Type, out: &mut Map<String, Value>) -> bool {
    let Some((part, rest)) = parts.split_first() else { return words.is_empty() };
    match part {
        Part::Word(word) => {
            // A symbol-led amount may be said in either order: `€20` or `20 €`.
            words.first().is_some_and(|w| same_word(&w.to_lowercase(), word)) && fill(rest, &words[1..], input, out)
        }
        Part::Slot(field, Class::Text) => {
            // Takes one word or more, leaving the rest able to match.
            for take in (1..=words.len()).rev() {
                let mut attempt = out.clone();
                attempt.insert(field.clone(), json!(words[..take].join(" ")));
                if fill(rest, &words[take..], input, &mut attempt) {
                    *out = attempt;
                    return true;
                }
            }
            false
        }
        Part::Slot(field, class) => {
            let Some(word) = words.first().filter(|w| fits(*class, w, input.fields.get(field))) else { return false };
            let value = match class {
                Class::Number => match number(word).unwrap_or(0.0) {
                    n if n.fract() == 0.0 && n.abs() < 9.0e15 => json!(n as i64),
                    n => json!(n),
                },
                Class::Currency => json!(currency_code(word).unwrap_or_default()),
                Class::Choice => json!(input.fields.get(field).and_then(|t| t.values.iter().find(|v| v.eq_ignore_ascii_case(word)).cloned()).unwrap_or_default()),
                _ => json!(word),
            };
            out.insert(field.clone(), value);
            fill(rest, &words[1..], input, out)
        }
    }
}

/// A currency symbol before its amount reads the same as the code after it.
fn amount_first(words: Vec<String>) -> Vec<String> {
    let mut out = words;
    for i in 0..out.len().saturating_sub(1) {
        if out[i].chars().count() == 1 && currency::is_symbol(out[i].chars().next().unwrap_or(' ')) && number(&out[i + 1]).is_some() {
            out.swap(i, i + 1);
        }
    }
    out
}

/// The input `text` means for an ability, read through its examples. `None` when no example
/// has that shape.
pub fn parse(ability: &Ability, text: &str) -> Option<Value> {
    let words = amount_first(tokens(text));
    if words.is_empty() || ability.input.fields.is_empty() {
        return None;
    }
    for example in &ability.examples {
        // The example goes through the same reordering, so `€20 in yen` and `20 eur in yen` agree.
        let normal = amount_first(tokens(example)).join(" ");
        let parts = pattern(&normal, &ability.input);
        if !parts.iter().any(|p| matches!(p, Part::Slot(..))) {
            continue;
        }
        let mut found = Map::new();
        if fill(&parts, &words, &ability.input, &mut found)
            && let Ok(fitted) = ability.input.coerce(Value::Object(found)) {
            return Some(fitted);
        }
    }
    None
}

/// Whether an example parses to something its own schema accepts: what `dev check` verifies.
pub fn check(ability: &Ability) -> Vec<String> {
    ability.examples.iter().filter(|e| !ability.input.fields.is_empty() && parse(ability, e).is_none()).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert() -> Ability {
        Ability {
            input: Type::record([("amount", Type::of("number")), ("from", Type::of("currency")), ("to", Type::of("currency"))]),
            examples: vec!["50 usd to inr".into(), "€20 in yen".into()],
            ..Default::default()
        }
    }

    #[test]
    fn examples_teach_the_shape() {
        let a = convert();
        assert_eq!(parse(&a, "50 usd to inr"), Some(json!({ "amount": 50, "from": "USD", "to": "INR" })));
        assert_eq!(parse(&a, "75 bucks to rupees"), Some(json!({ "amount": 75, "from": "USD", "to": "INR" })));
        assert_eq!(parse(&a, "€20 in yen"), Some(json!({ "amount": 20, "from": "EUR", "to": "JPY" })));
        assert_eq!(parse(&a, "$1,250.50 into eur"), Some(json!({ "amount": 1250.5, "from": "USD", "to": "EUR" })));
        assert_eq!(parse(&a, "open safari"), None);
        assert_eq!(parse(&a, "50"), None);
        assert!(check(&a).is_empty());
    }

    #[test]
    fn free_words_fill_the_text_field() {
        let remind = Ability {
            input: Type::record([("what", Type::of("text")), ("minutes", Type::of("number"))]),
            examples: vec!["remind me to stretch in 20".into()],
            ..Default::default()
        };
        assert_eq!(parse(&remind, "remind me to call the bank in 45"), None, "`remind me` isn't glue, so it is part of the text");
        let simple = Ability { input: Type::record([("what", Type::of("text")), ("minutes", Type::of("number"))]), examples: vec!["stretch in 20".into()], ..Default::default() };
        assert_eq!(parse(&simple, "call the bank in 45"), Some(json!({ "what": "call the bank", "minutes": 45 })));
        let bad = Ability { input: Type::record([("n", Type::of("number"))]), examples: vec!["no number here".into()], ..Default::default() };
        assert_eq!(check(&bad), vec!["no number here".to_string()]);
    }
}
