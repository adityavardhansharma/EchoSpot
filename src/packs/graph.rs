//! The dependency graph: which addresses a pack uses, and which packs use an address. Changing
//! or deleting an ability shows what depends on it; a caller of an address that is gone is
//! marked broken, with an error that names it.

use super::runtime::{Pack, Runtime};

/// One place a pack names an address.
#[derive(Clone, Debug, PartialEq)]
pub struct Use {
    pub address: String,
    /// `init.luau:31`, `pack.toml`, `boards.morning`.
    pub at: String,
    /// Written out in code or on a board, so it is certainly called. A `calls` entry alone is
    /// only a permission asked for.
    pub literal: bool,
}

/// Addresses as string literals after `:call(`, `:try(` or `:load(` in Luau source.
pub fn calls_in(source: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (n, line) in source.lines().enumerate() {
        let code = line.split("--").next().unwrap_or(line);
        let mut rest = code;
        while let Some(at) = [":call(", ":try(", ":load("].iter().filter_map(|m| rest.find(m).map(|i| i + m.len())).min() {
            rest = &rest[at..];
            let trimmed = rest.trim_start();
            let Some(quote) = trimmed.chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
            if let Some(end) = trimmed[1..].find(quote) {
                let address = &trimmed[1..1 + end];
                if address.contains('.') && address.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
                    out.push((address.to_string(), n + 1));
                }
            }
        }
    }
    out
}

/// The same in a Rust pack's source: `cx.call("…"` and `cx.try_call("…"`, and `calls::name(`
/// for a function `calls.rs` defines (`names` maps each to its address).
pub fn rust_calls_in(source: &str, names: &std::collections::HashMap<String, String>) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (n, line) in source.lines().enumerate() {
        let code = line.split("//").next().unwrap_or(line);
        for (marker, literal) in [(".call(", true), (".try_call(", true), ("calls::", false)] {
            let mut rest = code;
            while let Some(at) = rest.find(marker) {
                rest = &rest[at + marker.len()..];
                if literal {
                    let trimmed = rest.trim_start();
                    if let Some(body) = trimmed.strip_prefix('"')
                        && let Some(end) = body.find('"') {
                        let address = &body[..end];
                        if address.contains('.') && address.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
                            out.push((address.to_string(), n + 1));
                        }
                    }
                } else {
                    let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                    if let Some(address) = names.get(name.trim_start_matches("r#")) {
                        out.push((address.clone(), n + 1));
                    }
                }
            }
        }
    }
    out
}

/// What each function in a Rust pack's `calls.rs` calls.
fn calls_rs_names(text: &str) -> std::collections::HashMap<String, String> {
    let mut names = std::collections::HashMap::new();
    let mut current = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("pub fn ") {
            current = Some(rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<String>());
        } else if let (Some(name), Some(at)) = (&current, line.find("cx.call(\"")) {
            let rest = &line[at + 9..];
            if let Some(end) = rest.find('"') {
                names.insert(name.clone(), rest[..end].to_string());
            }
            current = None;
        }
    }
    names
}

/// Every address a pack names: in its code, on its boards, in `calls`.
pub fn uses(pack: &Pack) -> Vec<Use> {
    let mut out = Vec::new();
    if let Ok((sources, _)) = super::vm::sources(&pack.dir) {
        for file in sources.paths().filter(|f| !f.starts_with("__") && f.ends_with(".luau")) {
            let text = sources.get(file).unwrap_or("");
            out.extend(calls_in(text).into_iter().map(|(address, line)| Use { address, at: format!("{file}:{line}"), literal: true }));
        }
    }
    // A Rust pack's own source; `echo.rs` and `calls.rs` are EchoSpot's.
    if super::wasm::is_rust(&pack.dir) {
        let names = calls_rs_names(&std::fs::read_to_string(pack.dir.join("src/calls.rs")).unwrap_or_default());
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        let mut stack = vec![pack.dir.join("src")];
        while let Some(dir) = stack.pop() {
            for path in std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()) {
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|x| x == "rs") && !path.ends_with("src/echo.rs") && !path.ends_with("src/calls.rs") {
                    files.push(path);
                }
            }
        }
        files.sort();
        for path in files {
            let rel = path.strip_prefix(&pack.dir).map(|p| p.display().to_string()).unwrap_or_default();
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            out.extend(rust_calls_in(&text, &names).into_iter().map(|(address, line)| Use { address, at: format!("{rel}:{line}"), literal: true }));
        }
    }
    for (name, board) in &pack.index.boards {
        out.extend(board.tiles.iter().map(|tile| Use { address: tile.address.clone(), at: format!("boards.{name}"), literal: true }));
    }
    for call in &pack.manifest.permissions.calls {
        if !out.iter().any(|u| &u.address == call) {
            out.push(Use { address: call.clone(), at: "pack.toml".into(), literal: false });
        }
    }
    // A pack's own abilities aren't dependencies.
    let own = format!("{}.", pack.id);
    out.retain(|u| !u.address.starts_with(&own));
    out
}

/// The packs that use `address`, and where.
pub fn dependents(rt: &Runtime, address: &str) -> Vec<(String, String)> {
    rt.packs.values().flat_map(|p| uses(p).into_iter().filter(|u| u.address == address).map(|u| (p.id.clone(), u.at)).collect::<Vec<_>>()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_names_its_addresses() {
        let source = "local a = cx:call(\"github.prs\", { repo = \"rune\" })\n-- cx:call(\"old.gone\")\nlocal ok = cx:try('scripts.pi', {}) local r = cx:load( \"calendar.next\" )\ncx:call(address, {})";
        assert_eq!(calls_in(source), vec![("github.prs".to_string(), 1), ("scripts.pi".to_string(), 3), ("calendar.next".to_string(), 3)]);
    }

    #[test]
    fn rust_code_names_its_addresses() {
        let names = calls_rs_names("pub fn calc_convert(cx: &Cx, amount: f64) -> Result {\n    let mut input = Map::new();\n    cx.call(\"calc.convert\", Value::Object(input))\n}\n");
        let source = "let a = cx.call(\"github.prs\", json!({}))?;\n// cx.call(\"old.gone\", x)\nlet b = calls::calc_convert(cx, 1.0)?; let c = cx.try_call(\"scripts.pi\", v);\ncx.call(address, v)";
        assert_eq!(rust_calls_in(source, &names), vec![("github.prs".to_string(), 1), ("scripts.pi".to_string(), 3), ("calc.convert".to_string(), 3)]);
    }
}
