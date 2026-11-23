//! Deep links: `echospot://github.prs?repo=rune` opens a surface from anywhere: a browser, a
//! notification, another app.

use serde_json::{Map, Value};

/// A byte-wise percent decode; `+` is a space, as a form writes it. A `%` that isn't followed by
/// two hex digits stays a `%`.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%').then(|| bytes.get(i + 1..i + 3)).flatten().and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (_, Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

/// The address and arguments a link names. `None` when it isn't an `echospot://` link to an
/// ability: the address must be `<pack>.<name>`, in the characters an address can hold.
pub fn parse(link: &str) -> Option<(String, Value)> {
    let rest = link.trim().strip_prefix("echospot://")?;
    let (address, query) = rest.split_once('?').unwrap_or((rest, ""));
    let address = address.trim_end_matches('/');
    if !address.contains('.') || !address.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return None;
    }
    let mut args = Map::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        args.insert(decode(key), Value::String(decode(value)));
    }
    Some((address.to_string(), Value::Object(args)))
}

/// The link that opens an address with these arguments.
pub fn make(address: &str, args: &Value) -> String {
    let encode = |text: &str| text.bytes().map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    let query: Vec<String> = args.as_object().into_iter().flatten().map(|(k, v)| format!("{}={}", encode(k), encode(&v.as_str().map(String::from).unwrap_or_else(|| v.to_string())))).collect();
    if query.is_empty() { format!("echospot://{address}") } else { format!("echospot://{address}?{}", query.join("&")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_link_names_an_address_and_its_arguments() {
        assert_eq!(parse("echospot://github.prs?repo=rune"), Some(("github.prs".into(), json!({ "repo": "rune" }))));
        assert_eq!(parse("echospot://notes.create?title=Buy+milk%20%26%20eggs&body=%E2%9C%93"), Some(("notes.create".into(), json!({ "title": "Buy milk & eggs", "body": "✓" }))));
        assert_eq!(parse("echospot://github.prs"), Some(("github.prs".into(), json!({}))));
        assert_eq!(parse("https://github.prs"), None);
        assert_eq!(parse("echospot://../../etc"), None);
        assert_eq!(parse("echospot://x.y?a=100%"), Some(("x.y".into(), json!({ "a": "100%" }))));
        let link = make("notes.create", &json!({ "title": "Buy milk & eggs" }));
        assert_eq!(parse(&link), Some(("notes.create".into(), json!({ "title": "Buy milk & eggs" }))));
    }
}
