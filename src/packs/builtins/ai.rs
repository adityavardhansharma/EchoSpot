//! `ai.*`: the user's own model, for packs. The pack never sees a key; each call is charged to
//! the pack's monthly budget and its prompt and answer go to the audit log.

use std::sync::Arc;
use std::sync::mpsc::channel;

use serde_json::{Value, json};

use super::{Ctx, Done, Emit, Gate, Run, Spec, missing, text};
use crate::features::ai::{self, AiSettings};
use crate::features::ai_wire::{self, Chunk, Control, Toolbox};
use crate::features::mcp::Tool;
use crate::packs::error::{PackError, Result};
use crate::packs::index::Kind;
use crate::packs::types::Type;

/// Dollars per million tokens, in and out, by the start of a model's id. The providers bill
/// by tokens the stream doesn't always report, so spend is an estimate: four characters a token.
const PRICES: &[(&str, f64, f64)] = &[
    ("claude-haiku", 1.0, 5.0), ("claude-sonnet", 3.0, 15.0), ("claude-opus", 15.0, 75.0), ("claude-fable", 15.0, 75.0), ("claude", 3.0, 15.0),
    ("gpt-4o-mini", 0.15, 0.6), ("gpt-4o", 2.5, 10.0), ("gpt-5-mini", 0.25, 2.0), ("gpt-5", 1.25, 10.0), ("o3", 2.0, 8.0),
    ("gemini", 1.25, 10.0),
];

/// What a call of `chars_in` and `chars_out` characters cost, as nearly as can be told.
pub fn estimate_usd(model: &str, chars_in: usize, chars_out: usize) -> f64 {
    let name = model.rsplit('/').next().unwrap_or(model);
    let (price_in, price_out) = PRICES.iter().find(|(prefix, _, _)| name.starts_with(prefix)).map(|(_, i, o)| (*i, *o)).unwrap_or((3.0, 15.0));
    (chars_in as f64 / 4.0 * price_in + chars_out as f64 / 4.0 * price_out) / 1e6
}

fn settings() -> AiSettings {
    crate::settings::Settings::load().ai
}

/// A model on the user's own machine costs nothing.
fn local(settings: &AiSettings) -> bool {
    let url = settings.endpoint();
    url.contains("localhost") || url.contains("127.0.0.1") || url.contains("[::1]")
}

/// The checks every `ai.*` call makes before it spends anything.
fn allowed(ctx: &Ctx) -> Result<AiSettings> {
    if !ctx.user {
        if ctx.perms.ai.is_none() {
            return Err(PackError::new("E0400", format!("{} has no AI budget: add ai = {{ budget_usd_month = 1.00 }} under [permissions] in pack.toml, then grant it in EchoSpot", ctx.pack)).learn("echospot dev grants"));
        }
        if ctx.budget_left <= 0.0 {
            return Err(PackError::new("E0406", format!("{} has spent its AI budget for this month", ctx.pack)).learn("echospot dev grants"));
        }
    }
    let settings = settings();
    match settings.missing() {
        Some(why) => Err(PackError::new("E0520", why).learn("echospot settings ai")),
        None => Ok(settings),
    }
}

/// Runs one request to its end, handing each piece of text to `on_text`.
fn complete(settings: &AiSettings, system: &str, prompt: &str, tools: Toolbox, on_text: &dyn Fn(&str)) -> Result<(String, f64)> {
    let mut plain = settings.clone();
    plain.web_search = false;
    let job = ai::job_for(&plain, system.to_string(), vec![json!({ "role": "user", "content": prompt })], false);
    let (tx, rx) = channel();
    let worker = std::thread::Builder::new().name("echospot-pack-ai".into()).spawn(move || ai_wire::run_with(job, tx, || {}, Control::default(), tools))
        .map_err(|_| PackError::new("E0900", "couldn't start a worker for the model"))?;
    let (mut out, mut error) = (String::new(), None);
    for chunk in rx {
        match chunk {
            Chunk::Text(t) => {
                out.push_str(&t);
                on_text(&out);
            }
            Chunk::Error(e) => error = Some(e),
            Chunk::Done => break,
            Chunk::Status(_) => {}
        }
    }
    let _ = worker.join();
    if let Some(e) = error.filter(|_| out.is_empty()) {
        return Err(PackError::new("E0520", e).learn("echospot settings ai"));
    }
    let usd = if local(settings) { 0.0 } else { estimate_usd(&settings.model, system.len() + prompt.len(), out.len()) };
    Ok((out, usd))
}

const SYSTEM: &str = "You are a function inside a small desktop tool. Answer with the result only: no preamble, no closing remarks.";

fn ask(ctx: &Ctx, input: &Value) -> Run {
    let prompt = text(input, "prompt").to_string();
    if prompt.trim().is_empty() {
        return Run::err(missing("prompt"));
    }
    let settings = match allowed(ctx) {
        Ok(s) => s,
        Err(e) => return Run::err(e),
    };
    let system = Some(text(input, "system")).filter(|s| !s.is_empty()).unwrap_or(SYSTEM).to_string();
    Run::Later(Box::new(move |_| {
        let (out, usd) = complete(&settings, &system, &prompt, Toolbox::default(), &|_| {})?;
        Ok(Done { value: Value::String(out.trim().to_string()), usd, host: "ai".into(), ..Default::default() })
    }))
}

fn stream(ctx: &Ctx, input: &Value) -> Run {
    let prompt = text(input, "prompt").to_string();
    if prompt.trim().is_empty() {
        return Run::err(missing("prompt"));
    }
    let settings = match allowed(ctx) {
        Ok(s) => s,
        Err(e) => return Run::err(e),
    };
    let system = Some(text(input, "system")).filter(|s| !s.is_empty()).unwrap_or(SYSTEM).to_string();
    Run::Later(Box::new(move |emit: &Emit| {
        // Each piece lands in the resource a `ui.Stream` draws from, as it is written.
        let (out, usd) = complete(&settings, &system, &prompt, Toolbox::default(), &|so_far| emit(json!({ "text": so_far, "done": false })))?;
        Ok(Done { value: json!({ "text": out, "done": true }), usd, host: "ai".into(), ..Default::default() })
    }))
}

/// The JSON in a model's answer: the whole of it, or what sits inside a code fence or the
/// outermost braces.
pub fn json_in(answer: &str) -> Option<Value> {
    let trimmed = answer.trim();
    if let Ok(v) = serde_json::from_str(trimmed) {
        return Some(v);
    }
    if let Some(start) = trimmed.find("```") {
        let rest = &trimmed[start + 3..];
        let rest = rest.strip_prefix("json").unwrap_or(rest);
        if let Some(end) = rest.find("```")
            && let Ok(v) = serde_json::from_str(rest[..end].trim()) {
            return Some(v);
        }
    }
    for (open, close) in [('{', '}'), ('[', ']')] {
        if let (Some(a), Some(b)) = (trimmed.find(open), trimmed.rfind(close))
            && a < b
            && let Ok(v) = serde_json::from_str(&trimmed[a..=b]) {
            return Some(v);
        }
    }
    None
}

fn extract(ctx: &Ctx, input: &Value) -> Run {
    let source = text(input, "text").to_string();
    if source.trim().is_empty() {
        return Run::err(missing("text"));
    }
    let schema = match input.get("schema").filter(|s| !s.is_null()).map(Type::from_json) {
        Some(Ok(t)) => t,
        Some(Err(e)) => return Run::err(PackError::new("E0301", format!("ai.extract: schema: {e}")).learn("echospot dev api show ai.extract")),
        None => return Run::err(missing("schema")),
    };
    let settings = match allowed(ctx) {
        Ok(s) => s,
        Err(e) => return Run::err(e),
    };
    Run::Later(Box::new(move |_| {
        let system = "You extract structured data. Answer with one JSON value that matches the schema, and nothing else: no prose, no code fence.";
        let prompt = format!("JSON Schema:\n{}\n\nText:\n{source}", schema.json_schema());
        let (out, usd) = complete(&settings, system, &prompt, Toolbox::default(), &|_| {})?;
        let found = json_in(&out).ok_or_else(|| PackError::new("E0520", format!("the model didn't answer with JSON: {}", crate::util::truncate(&out, 120))))?;
        let value = schema.coerce(found).map_err(|e| PackError::new("E0302", format!("the model's answer doesn't fit the schema: {e}")))?;
        Ok(Done { value, usd, host: "ai".into(), ..Default::default() })
    }))
}

/// A tool's name as every provider accepts it: `github.prs` goes out as `github__prs`.
pub fn wire(address: &str) -> String {
    address.replace('.', "__").replace('-', "_")
}

fn agent(ctx: &Ctx, input: &Value) -> Run {
    let goal = text(input, "goal").to_string();
    if goal.trim().is_empty() {
        return Run::err(missing("goal"));
    }
    let addresses: Vec<String> = input.get("tools").and_then(Value::as_array).into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect();
    let settings = match allowed(ctx) {
        Ok(s) => s,
        Err(e) => return Run::err(e),
    };
    let Some(caller) = ctx.caller.clone() else { return Run::err(PackError::new("E0900", "ai.agent needs the broker to call tools")) };
    Run::Later(Box::new(move |_| {
        // Only the abilities the pack listed, each described by the host, each call through the broker.
        let mut tools = Vec::new();
        for address in &addresses {
            let about = (caller.0)("echo.describe", json!({ "address": address }))?;
            tools.push(Tool {
                server: "echospot".into(), name: address.clone(), wire: wire(address),
                description: format!("{} ({})", about["title"].as_str().unwrap_or(address), about["kind"].as_str().unwrap_or("read")),
                schema: about.get("schema").cloned().unwrap_or_else(|| json!({ "type": "object" })),
            });
        }
        let steps = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let (names, log, call) = (tools.iter().map(|t| (t.wire.clone(), t.name.clone())).collect::<Vec<_>>(), steps.clone(), caller.clone());
        let toolbox = Toolbox {
            tools,
            call: Some(Arc::new(move |wire: &str, arguments: &Value| {
                let Some((_, address)) = names.iter().find(|(w, _)| w == wire) else { return Err(format!("no tool named {wire}")) };
                let outcome = (call.0)(address, arguments.clone());
                log.lock().unwrap_or_else(|e| e.into_inner()).push(json!({ "address": address, "input": arguments, "ok": outcome.is_ok() }));
                match outcome {
                    Ok(value) => Ok(value.as_str().map(String::from).unwrap_or_else(|| value.to_string())),
                    Err(e) => Err(e.to_string()),
                }
            })),
        };
        let system = "You complete a goal for a small desktop tool by using the tools you are given. Use only what they return. When the goal is met, answer with the result alone.";
        let (out, usd) = complete(&settings, system, &goal, toolbox, &|_| {})?;
        let steps = steps.lock().unwrap_or_else(|e| e.into_inner()).clone();
        Ok(Done { value: json!({ "text": out.trim(), "steps": steps }), usd, host: "ai".into(), ..Default::default() })
    }))
}

pub fn register(out: &mut Vec<Spec>) {
    out.push(Spec::new("ai.ask", Kind::Read, Gate::Ai, "Ask the user's model a question and get its answer as text", "prompt:text, system:text?", "text", ask));
    out.push(Spec::new("ai.extract", Kind::Read, Gate::Ai, "Pull structured data out of text, to match a schema", "text:text, schema:any", "any", extract));
    out.push(Spec::new("ai.stream", Kind::Read, Gate::Ai, "An answer that arrives as it is written: load it and give it to ui.Stream", "prompt:text, system:text?", "record", stream));
    out.push(Spec::new("ai.agent", Kind::Read, Gate::Ai, "Reach a goal with a tool loop over the abilities listed, under the pack's own grants", "goal:text, tools:list<text>", "record", agent));
}

/// The user's AI presets as abilities: `ai.preset.<name>`, text in, text out.
pub fn presets(out: &mut Vec<Spec>) {
    for preset in crate::features::ai_library::Library::load().presets {
        let name = super::wrap::slug(&preset.name);
        if name.is_empty() {
            continue;
        }
        out.push(Spec::new(format!("ai.preset.{name}"), Kind::Read, Gate::Ai, format!("Ask the “{}” preset", preset.name), "text:text", "text", move |ctx, input| {
            let prompt = text(input, "text").to_string();
            if prompt.trim().is_empty() {
                return Run::err(missing("text"));
            }
            let mut settings = match allowed(ctx) {
                Ok(s) => s,
                Err(e) => return Run::err(e),
            };
            if !preset.model.is_empty() {
                settings.model = preset.model.clone();
            }
            let system = preset.instructions.clone();
            Run::Later(Box::new(move |_| {
                let (out, usd) = complete(&settings, &system, &prompt, Toolbox::default(), &|_| {})?;
                Ok(Done { value: Value::String(out.trim().to_string()), usd, host: "ai".into(), ..Default::default() })
            }))
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_give_up_their_json() {
        assert_eq!(json_in("{\"a\": 1}"), Some(json!({ "a": 1 })));
        assert_eq!(json_in("Here you go:\n```json\n[1, 2]\n```"), Some(json!([1, 2])));
        assert_eq!(json_in("The result is {\"title\": \"x\"} as asked."), Some(json!({ "title": "x" })));
        assert_eq!(json_in("no data here"), None);
    }

    #[test]
    fn spend_is_estimated_from_length() {
        assert!((estimate_usd("claude-haiku-5-5", 4_000_000, 0) - 1.0).abs() < 1e-9);
        assert!(estimate_usd("openrouter/anthropic/claude-opus-5", 0, 400) > estimate_usd("gpt-4o-mini", 0, 400));
        assert_eq!(wire("github.prs"), "github__prs");
    }

    #[test]
    fn a_pack_without_a_budget_is_refused() {
        let ctx = Ctx { pack: "x".into(), ..Default::default() };
        assert_eq!(allowed(&ctx).err().map(|e| e.code), Some("E0400"));
        let spent = Ctx { pack: "x".into(), perms: crate::packs::manifest::Permissions { ai: Some(crate::packs::manifest::AiBudget { budget_usd_month: 1.0 }), ..Default::default() }, ..Default::default() };
        assert_eq!(allowed(&spent).err().map(|e| e.code), Some("E0406"));
    }
}
