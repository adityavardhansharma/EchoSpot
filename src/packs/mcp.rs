//! `echospot mcp serve`: every ability as a tool for outside agents, over stdio. Each pack an
//! agent builds makes the next agent more capable on this machine. A call made here is a call
//! through the broker like any other: reads are open, effects only where a pack marked them
//! `ai = true`, and everything is checked against grants and written to the audit log.

use std::io::{BufRead, Write};
use std::time::Duration;

use serde_json::{Value, json};

use super::builtins::Gate;
use super::builtins::ai::wire;
use super::index::Kind;
use super::runtime::{Origin, Responder, Runtime, State};

/// The tools on offer: `(wire name, address, description, input schema)`.
pub fn tools(rt: &Runtime) -> Vec<(String, String, String, Value)> {
    let mut out = Vec::new();
    for pack in rt.packs.values().filter(|p| p.state == State::Ready) {
        for (name, a) in pack.index.abilities.iter().filter(|(_, a)| a.has_run && (a.kind == Kind::Read || a.ai)) {
            let address = format!("{}.{name}", pack.id);
            let mut about = if a.description.is_empty() { a.title.clone() } else { format!("{}: {}", a.title, a.description) };
            if a.kind != Kind::Read {
                about.push_str(&format!(" ({}: changes something)", a.kind.name()));
            }
            out.push((wire(&address), address, about, a.input.json_schema()));
        }
    }
    for spec in rt.specs() {
        let open = matches!(spec.gate, Gate::Free | Gate::Calls | Gate::Clipboard(_)) && (spec.kind == Kind::Read || spec.gate == Gate::Free);
        if open && !spec.address.starts_with("files.pick") {
            out.push((wire(&spec.address), spec.address.clone(), spec.title.clone(), spec.input.json_schema()));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Answers one JSON-RPC message. `None` for a notification, which takes no answer.
pub fn handle(rt: &mut Runtime, message: &Value) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18"),
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "echospot", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Abilities of EchoSpot packs and EchoSpot's own built-ins. Reads are open; an effect is offered only where its pack marked it for AI use. Every call is checked against the pack's grants and logged.",
        })),
        "ping" => Ok(json!({})),
        "tools/list" => {
            rt.load_all();
            Ok(json!({ "tools": tools(rt).into_iter().map(|(name, address, about, schema)| json!({ "name": name, "title": address, "description": about, "inputSchema": schema })).collect::<Vec<_>>() }))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            match tools(rt).into_iter().find(|(w, address, ..)| w == name || address == name) {
                Some((_, address, ..)) => {
                    let arguments = params.get("arguments").cloned().filter(Value::is_object).unwrap_or_else(|| json!({}));
                    let token = rt.start(Origin::Mcp, &address, arguments, Responder::Keep, false);
                    Ok(match rt.wait(token, Duration::from_secs(120)) {
                        Ok(value) => json!({ "content": [{ "type": "text", "text": value.as_str().map(String::from).unwrap_or_else(|| serde_json::to_string_pretty(&value).unwrap_or_default()) }], "structuredContent": if value.is_object() { value.clone() } else { json!({ "result": value }) }, "isError": false }),
                        Err(e) => json!({ "content": [{ "type": "text", "text": e.to_string() }], "isError": true }),
                    })
                }
                None => Err((-32602, format!("no tool named {name}"))),
            }
        }
        _ if id.is_none() => return None,
        other => Err((-32601, format!("no method {other}"))),
    };
    let id = id?;
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
    })
}

/// Serves until stdin closes: one JSON-RPC message a line, as MCP's stdio transport carries them.
pub fn serve() {
    let mut rt = Runtime::for_cli();
    rt.load_all();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle(&mut rt, &message),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("not JSON: {e}") } })),
        };
        if let Some(answer) = answer {
            let _ = writeln!(stdout, "{answer}");
            let _ = stdout.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::tests::Fixture;

    #[test]
    fn abilities_are_tools_and_effects_stay_shut() {
        let mut f = Fixture::new("mcp");
        f.pack("m", "", "local echo = require(\"@echo\")\nlocal t = require(\"@echo/types\")\nreturn echo.pack { abilities = {\n look = echo.ability { title = \"Look\", kind = \"read\", input = { n = t.number() }, run = function(i) return i.n * 2 end },\n act = echo.ability { kind = \"effect\", run = function() return 2 end },\n} }");
        let init = handle(&mut f.rt, &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } })).unwrap();
        assert_eq!(init["result"]["protocolVersion"], json!("2025-03-26"));
        assert!(handle(&mut f.rt, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let listed = tools(&f.rt);
        assert!(listed.iter().any(|t| t.0 == "m__look") && !listed.iter().any(|t| t.0 == "m__act"));
        assert!(listed.iter().any(|t| t.1 == "calc.eval") && !listed.iter().any(|t| t.1 == "http.request" || t.1 == "run.exec"));
        let look = listed.iter().find(|t| t.0 == "m__look").unwrap();
        assert_eq!(look.3["properties"]["n"]["type"], json!("number"));
        let called = handle(&mut f.rt, &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "m__look", "arguments": { "n": 21 } } })).unwrap();
        assert_eq!((called["result"]["content"][0]["text"].as_str(), called["result"]["isError"].as_bool()), (Some("42"), Some(false)));
        let refused = handle(&mut f.rt, &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "m__act" } })).unwrap();
        assert_eq!(refused["error"]["code"], json!(-32602));
        assert_eq!(handle(&mut f.rt, &json!({ "jsonrpc": "2.0", "id": 4, "method": "nope" })).unwrap()["error"]["code"], json!(-32601));
    }
}
