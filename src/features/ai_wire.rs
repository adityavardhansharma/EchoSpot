//! What goes over the wire to a model and what comes back: the two request shapes (Anthropic's
//! Messages API, and OpenAI-style chat completions), their streams, attachments, and the loop
//! that runs a model's tool calls and hands it the results.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::features::mcp;
use crate::util;

pub enum Chunk {
    Text(String),
    /// What the model is doing while no text arrives: searching, running a tool.
    Status(String),
    Error(String),
    Done,
}

/// The most rounds of tool calls one question may take.
const MAX_ROUNDS: usize = 12;
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_TEXT_CHARS: usize = 200_000;

pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

fn image_type(path: &Path) -> Option<&'static str> {
    Some(match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

/// Whether a file can be attached at all, and if not, why.
pub fn attachable(path: &Path) -> Result<(), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let size = std::fs::metadata(path).map_err(|_| format!("{name} is not a file that can be read."))?.len();
    if image_type(path).is_some() {
        return if size <= MAX_IMAGE_BYTES { Ok(()) } else { Err(format!("{name} is over 5 MB; too large to attach.")) };
    }
    if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
        return if util::has_program("pdftotext") { Ok(()) } else { Err("Reading a PDF needs pdftotext (poppler).".into()) };
    }
    match std::fs::read(path) {
        Ok(bytes) if std::str::from_utf8(&bytes).is_ok() => Ok(()),
        _ => Err(format!("{name} is not text, an image or a PDF.")),
    }
}

/// A file as text for the model: a PDF through `pdftotext`, anything else as it is.
fn file_text(path: &Path) -> Result<String, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let text = if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
        util::output("pdftotext", &["-layout", &path.display().to_string(), "-"]).ok_or_else(|| format!("Could not read {name}."))?
    } else {
        std::fs::read_to_string(path).map_err(|_| format!("Could not read {name}."))?
    };
    // A file too long to send whole is said to be, not quietly cut.
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!("{name} is too long to attach whole ({} characters). Attach the part you need.", text.chars().count()));
    }
    Ok(format!("<file name=\"{name}\">\n{text}\n</file>"))
}

/// A user turn as each wire format writes it: plain text, or text with its attachments.
pub fn user_content(text: &str, attachments: &[String], anthropic: bool) -> Result<Value, String> {
    if attachments.is_empty() {
        return Ok(json!(text));
    }
    let mut parts = Vec::new();
    for path in attachments.iter().map(Path::new) {
        match image_type(path) {
            Some(media) => {
                attachable(path)?;
                let data = base64(&std::fs::read(path).map_err(|_| format!("Could not read {}.", path.display()))?);
                parts.push(if anthropic {
                    json!({ "type": "image", "source": { "type": "base64", "media_type": media, "data": data } })
                } else {
                    json!({ "type": "image_url", "image_url": { "url": format!("data:{media};base64,{data}") } })
                });
            }
            None => parts.push(json!({ "type": "text", "text": file_text(path)? })),
        }
    }
    parts.push(json!({ "type": "text", "text": text }));
    Ok(Value::Array(parts))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// One answer from the Messages API as its stream builds it: the content blocks whole, so they
/// can be sent back unchanged when the model asks for tools.
#[derive(Default)]
pub struct AnthropicTurn {
    pub blocks: Vec<Value>,
    partial: Vec<String>,
    pub stop: String,
    pub done: bool,
}

impl AnthropicTurn {
    pub fn event(&mut self, v: &Value) -> Vec<Chunk> {
        let mut out = Vec::new();
        if let Some(message) = v.get("error").and_then(|e| e.get("message")).and_then(Value::as_str) {
            self.done = true;
            return vec![Chunk::Error(message.to_string())];
        }
        let index = v.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "content_block_start" => {
                let block = v.get("content_block").cloned().unwrap_or(Value::Null);
                while self.blocks.len() <= index {
                    self.blocks.push(Value::Null);
                    self.partial.push(String::new());
                }
                match block.get("type").and_then(Value::as_str) {
                    Some("server_tool_use") => out.push(Chunk::Status(match block.get("name").and_then(Value::as_str) {
                        Some("web_search") => "Searching the web…".into(),
                        Some("web_fetch") => "Reading a page…".into(),
                        _ => "Working…".into(),
                    })),
                    Some("tool_use") => out.push(Chunk::Status(format!("Using {}…", pretty_tool(block.get("name").and_then(Value::as_str).unwrap_or("a tool"))))),
                    _ => {}
                }
                self.blocks[index] = block;
            }
            "content_block_delta" => {
                let (Some(block), Some(delta)) = (self.blocks.get_mut(index), v.get("delta")) else { return out };
                let piece = |key: &str| delta.get(key).and_then(Value::as_str).unwrap_or("").to_string();
                let append = |block: &mut Value, key: &str, more: &str| {
                    let had = block.get(key).and_then(Value::as_str).unwrap_or("").to_string();
                    block[key] = json!(had + more);
                };
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        append(block, "text", &piece("text"));
                        out.push(Chunk::Text(piece("text")));
                    }
                    Some("thinking_delta") => append(block, "thinking", &piece("thinking")),
                    Some("signature_delta") => block["signature"] = json!(piece("signature")),
                    Some("input_json_delta") => self.partial[index].push_str(&piece("partial_json")),
                    Some("citations_delta") => {
                        if let Some(citation) = delta.get("citation") {
                            match block.get_mut("citations").and_then(Value::as_array_mut) {
                                Some(list) => list.push(citation.clone()),
                                None => block["citations"] = json!([citation]),
                            }
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some(block) = self.blocks.get_mut(index)
                    && matches!(block.get("type").and_then(Value::as_str), Some("tool_use" | "server_tool_use")) {
                    let partial = std::mem::take(&mut self.partial[index]);
                    // Input cut short or malformed runs as an empty call, and the tool says what it lacks.
                    block["input"] = serde_json::from_str::<Value>(&partial).ok().filter(Value::is_object).unwrap_or_else(|| json!({}));
                }
            }
            "message_delta" => {
                if let Some(stop) = v.get("delta").and_then(|d| d.get("stop_reason")).and_then(Value::as_str) {
                    self.stop = stop.to_string();
                    match stop {
                        // A decline is a normal 200 response; say so rather than show an empty answer.
                        "refusal" => out.push(Chunk::Error("The model declined this request.".into())),
                        "max_tokens" => out.push(Chunk::Text("\n\n*(The answer was cut off at the length limit.)*".into())),
                        _ => {}
                    }
                }
            }
            "message_stop" => self.done = true,
            _ => {}
        }
        out
    }

    pub fn calls(&self) -> Vec<ToolCall> {
        if self.stop != "tool_use" {
            return Vec::new();
        }
        self.blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use")).map(|b| ToolCall {
            id: b.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
            name: b.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            input: b.get("input").cloned().unwrap_or_else(|| json!({})),
        }).collect()
    }

    /// The blocks to send back as the assistant's turn: all of them, as they came.
    pub fn content(&self) -> Vec<Value> {
        self.blocks.iter().filter(|b| !b.is_null()).cloned().collect()
    }
}

/// One answer from a chat-completions stream.
#[derive(Default)]
pub struct OpenAiTurn {
    pub text: String,
    /// `(id, name, arguments)` as they accumulate, by index.
    calls: Vec<(String, String, String)>,
    pub stop: String,
    pub done: bool,
}

impl OpenAiTurn {
    pub fn event(&mut self, v: &Value) -> Vec<Chunk> {
        let mut out = Vec::new();
        if let Some(error) = v.get("error") {
            self.done = true;
            return vec![Chunk::Error(error.get("message").and_then(Value::as_str).or(error.as_str()).unwrap_or("The provider reported an error.").to_string())];
        }
        let Some(choice) = v.get("choices").and_then(|c| c.get(0)) else { return out };
        if let Some(delta) = choice.get("delta") {
            if let Some(text) = delta.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                self.text.push_str(text);
                out.push(Chunk::Text(text.to_string()));
            }
            for call in delta.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                while self.calls.len() <= index {
                    self.calls.push(Default::default());
                }
                let slot = &mut self.calls[index];
                if let Some(id) = call.get("id").and_then(Value::as_str).filter(|i| !i.is_empty()) {
                    slot.0 = id.to_string();
                }
                if let Some(function) = call.get("function") {
                    if let Some(name) = function.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()) {
                        if slot.1.is_empty() {
                            out.push(Chunk::Status(format!("Using {}…", pretty_tool(name))));
                        }
                        slot.1 = name.to_string();
                    }
                    slot.2.push_str(function.get("arguments").and_then(Value::as_str).unwrap_or(""));
                }
            }
        }
        if let Some(stop) = choice.get("finish_reason").and_then(Value::as_str) {
            self.stop = stop.to_string();
            if stop == "length" {
                out.push(Chunk::Text("\n\n*(The answer was cut off at the length limit.)*".into()));
            }
        }
        out
    }

    pub fn calls(&self) -> Vec<ToolCall> {
        self.calls.iter().filter(|(_, name, _)| !name.is_empty()).enumerate().map(|(i, (id, name, arguments))| ToolCall {
            id: if id.is_empty() { format!("call_{i}") } else { id.clone() }, name: name.clone(),
            input: serde_json::from_str::<Value>(arguments).ok().filter(Value::is_object).unwrap_or_else(|| json!({})),
        }).collect()
    }

    /// The assistant's turn to send back before the tools' results.
    pub fn message(&self) -> Value {
        let calls: Vec<Value> = self.calls().iter().map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.input.to_string() } })).collect();
        json!({ "role": "assistant", "content": if self.text.is_empty() { Value::Null } else { json!(self.text) }, "tool_calls": calls })
    }
}

/// `files__read_file` as a person would say it: "read file (files)".
pub fn pretty_tool(wire: &str) -> String {
    match wire.split_once("__") {
        Some((server, tool)) => format!("{} ({server})", tool.replace('_', " ")),
        None => wire.replace('_', " "),
    }
}

pub struct Job {
    pub anthropic: bool,
    pub url: String,
    /// Headers, the key among them.
    pub headers: Vec<String>,
    /// The request without its messages: model, system prompt, stream, server tools.
    pub body: Value,
    pub messages: Vec<Value>,
    /// Offer the MCP servers' tools.
    pub tools: bool,
}

/// What is shared with the chat so it can stop a request in flight.
#[derive(Clone, Default)]
pub struct Control {
    pub cancelled: Arc<AtomicBool>,
    pub child: Arc<Mutex<Option<Child>>>,
}

impl Control {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Ok(mut slot) = self.child.lock()
            && let Some(mut child) = slot.take() {
            let _ = child.kill();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

/// Starts one streaming request. The body goes through a private file and the headers through
/// curl's stdin config, so neither the conversation nor the key shows in the process list.
fn post(job: &Job, body: &Value, control: &Control) -> Result<(std::process::ChildStdout, std::path::PathBuf), String> {
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let body_path = util::runtime_dir().join(format!("echospot-ai-{}-{}.json", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&body_path, body.to_string()).map_err(|_| "Could not write the request.".to_string())?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&body_path, std::fs::Permissions::from_mode(0o600));
    let mut config = format!("url = \"{}\"\ndata = \"@{}\"\n", job.url, body_path.display());
    for header in job.headers.iter().map(String::as_str).chain(["content-type: application/json"]) {
        config.push_str(&format!("header = \"{}\"\n", header.replace('\\', "\\\\").replace('"', "\\\"")));
    }
    let mut child = Command::new("curl").args(["-sN", "--max-time", "900", "-K", "-"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_| "curl is needed to reach the model.".to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(config.as_bytes());
    }
    let stdout = child.stdout.take().ok_or("Could not read the response.")?;
    if let Ok(mut slot) = control.child.lock() {
        *slot = Some(child);
    }
    Ok((stdout, body_path))
}

enum Turn {
    Anthropic(AnthropicTurn),
    OpenAi(OpenAiTurn),
}

/// Runs a question to its end on a worker thread: stream the answer, and for as long as the
/// model asks for tools, run them and ask again with what they returned.
pub fn run(mut job: Job, tx: Sender<Chunk>, wake: impl Fn(), control: Control) {
    let send = |chunk: Chunk| {
        let _ = tx.send(chunk);
        wake();
    };
    let mut tools: Vec<mcp::Tool> = Vec::new();
    if job.tools {
        send(Chunk::Status("Starting tools…".into()));
        let (found, problems) = mcp::tools();
        tools = found;
        for problem in problems {
            send(Chunk::Text(format!("*Tool server unavailable: {problem}*\n\n")));
        }
        let definitions: Vec<Value> = tools.iter().map(|t| if job.anthropic {
            json!({ "name": t.wire, "description": t.description, "input_schema": t.schema })
        } else {
            json!({ "type": "function", "function": { "name": t.wire, "description": t.description, "parameters": t.schema } })
        }).collect();
        if !definitions.is_empty() {
            match job.body.get_mut("tools").and_then(Value::as_array_mut) {
                Some(list) => list.extend(definitions),
                None => job.body["tools"] = Value::Array(definitions),
            }
        }
    }
    for round in 0..MAX_ROUNDS {
        if control.cancelled.load(Ordering::Relaxed) {
            break;
        }
        let mut body = job.body.clone();
        body["messages"] = Value::Array(job.messages.clone());
        let (stdout, body_path) = match post(&job, &body, &control) {
            Ok(started) => started,
            Err(e) => {
                send(Chunk::Error(e));
                break;
            }
        };
        let mut turn = if job.anthropic { Turn::Anthropic(AnthropicTurn::default()) } else { Turn::OpenAi(OpenAiTurn::default()) };
        let (mut any, mut failed, mut raw) = (false, false, String::new());
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Some(payload) = line.strip_prefix("data:").map(str::trim) else {
                // An HTTP error arrives as a plain JSON body, not as events.
                if !line.starts_with("event:") && !line.starts_with(':') && !line.is_empty() {
                    raw.push_str(&line);
                }
                continue;
            };
            if payload == "[DONE]" {
                break;
            }
            let Ok(event) = serde_json::from_str::<Value>(payload) else { continue };
            any = true;
            let (chunks, done) = match &mut turn {
                Turn::Anthropic(t) => (t.event(&event), t.done),
                Turn::OpenAi(t) => (t.event(&event), t.done),
            };
            for chunk in chunks {
                failed |= matches!(chunk, Chunk::Error(_));
                send(chunk);
            }
            if done || failed {
                break;
            }
        }
        let _ = std::fs::remove_file(&body_path);
        if let Ok(mut slot) = control.child.lock()
            && let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if control.cancelled.load(Ordering::Relaxed) || failed {
            break;
        }
        if !any {
            let message = serde_json::from_str::<Value>(&raw).ok()
                .and_then(|v| v.get("error").and_then(|e| e.get("message").or(Some(e))).and_then(Value::as_str).map(String::from))
                .unwrap_or_else(|| if raw.is_empty() { "No response. Check the connection and the endpoint.".into() } else { util::truncate(&raw, 200) });
            send(Chunk::Error(message));
            break;
        }
        let (calls, paused) = match &turn {
            Turn::Anthropic(t) => (t.calls(), t.stop == "pause_turn"),
            Turn::OpenAi(t) => (t.calls(), false),
        };
        if calls.is_empty() && !paused {
            break;
        }
        if round + 1 == MAX_ROUNDS {
            send(Chunk::Text("\n\n*(Stopped: too many tool calls in one answer.)*".into()));
            break;
        }
        // The model's own turn goes back exactly as it came, then what each tool returned.
        match &turn {
            Turn::Anthropic(t) => job.messages.push(json!({ "role": "assistant", "content": t.content() })),
            Turn::OpenAi(t) => job.messages.push(t.message()),
        }
        let mut results = Vec::new();
        for call in &calls {
            send(Chunk::Status(format!("Using {}…", pretty_tool(&call.name))));
            let known = tools.iter().any(|t| t.wire == call.name);
            let outcome = if known { mcp::call(&call.name, &call.input) } else { Err(format!("There is no tool named {}.", call.name)) };
            send(Chunk::Text(format!("{}*{} {}*\n\n", if round == 0 && results.is_empty() { "" } else { "\n\n" }, if outcome.is_ok() { "Used" } else { "Could not use" }, pretty_tool(&call.name))));
            let (text, is_error) = match outcome {
                Ok(text) => (if text.is_empty() { "(no output)".to_string() } else { util::truncate(&text, 100_000) }, false),
                Err(e) => (e, true),
            };
            results.push(if job.anthropic {
                json!({ "type": "tool_result", "tool_use_id": call.id, "content": text, "is_error": is_error })
            } else {
                json!({ "role": "tool", "tool_call_id": call.id, "content": text })
            });
        }
        if job.anthropic {
            // Every result of one turn travels in one user message. A paused turn has none: the
            // assistant's content alone tells the API to carry on.
            if !results.is_empty() {
                job.messages.push(json!({ "role": "user", "content": results }));
            }
        } else {
            job.messages.extend(results);
        }
    }
    send(Chunk::Done);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(chunks: &[Chunk]) -> String {
        chunks.iter().filter_map(|c| if let Chunk::Text(t) = c { Some(t.as_str()) } else { None }).collect()
    }

    #[test]
    fn encodes() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn anthropic_tool_turn() {
        let events = [
            r#"{"type":"message_start","message":{"id":"m"}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig=="}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Let me "}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"look."}}"#,
            r#"{"type":"content_block_stop","index":1}"#,
            r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"files__read_file","input":{}}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"path\": \"/tmp"}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"/a.txt\"}"}}"#,
            r#"{"type":"content_block_stop","index":2}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#,
            r#"{"type":"message_stop"}"#,
        ];
        let mut turn = AnthropicTurn::default();
        let mut chunks = Vec::new();
        for e in events {
            chunks.extend(turn.event(&serde_json::from_str(e).unwrap()));
        }
        assert!(turn.done);
        assert_eq!(text_of(&chunks), "Let me look.");
        assert_eq!(turn.calls(), vec![ToolCall { id: "toolu_1".into(), name: "files__read_file".into(), input: json!({ "path": "/tmp/a.txt" }) }]);
        // The thinking block goes back with its signature, untouched, ahead of the rest.
        let content = turn.content();
        assert_eq!(content.len(), 3);
        assert_eq!(content[0]["signature"], "sig==");
        assert_eq!(content[1]["text"], "Let me look.");
        assert_eq!(content[2]["input"]["path"], "/tmp/a.txt");
    }

    #[test]
    fn anthropic_plain_and_refusal() {
        let mut turn = AnthropicTurn::default();
        turn.event(&json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }));
        let chunks = turn.event(&json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Hi" } }));
        assert_eq!(text_of(&chunks), "Hi");
        turn.event(&json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" } }));
        assert!(turn.calls().is_empty());
        let mut refused = AnthropicTurn::default();
        assert!(matches!(refused.event(&json!({ "type": "message_delta", "delta": { "stop_reason": "refusal" } })).as_slice(), [Chunk::Error(_)]));
        // A tool call with no input still carries an object.
        let mut bare = AnthropicTurn::default();
        bare.event(&json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "tool_use", "id": "t", "name": "n", "input": {} } }));
        bare.event(&json!({ "type": "content_block_stop", "index": 0 }));
        bare.event(&json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use" } }));
        assert_eq!(bare.calls()[0].input, json!({}));
    }

    #[test]
    fn openai_tool_turn() {
        let events = [
            r#"{"choices":[{"delta":{"role":"assistant","content":""}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"files__list","arguments":""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"dir\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"/tmp\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
        ];
        let mut turn = OpenAiTurn::default();
        for e in events {
            turn.event(&serde_json::from_str(e).unwrap());
        }
        assert_eq!(turn.calls(), vec![ToolCall { id: "call_a".into(), name: "files__list".into(), input: json!({ "dir": "/tmp" }) }]);
        let message = turn.message();
        assert_eq!(message["tool_calls"][0]["function"]["name"], "files__list");
        assert!(message["content"].is_null());
        let mut plain = OpenAiTurn::default();
        let chunks = plain.event(&json!({ "choices": [{ "delta": { "content": "Hello" } }] }));
        assert_eq!(text_of(&chunks), "Hello");
        assert!(plain.calls().is_empty());
    }

    #[test]
    fn attachments() {
        let dir = std::env::temp_dir().join(format!("echospot-wire-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (note, picture) = (dir.join("note.txt"), dir.join("dot.png"));
        std::fs::write(&note, "remember the milk").unwrap();
        std::fs::write(&picture, [0x89, b'P', b'N', b'G']).unwrap();
        let paths = vec![note.display().to_string(), picture.display().to_string()];
        let a = user_content("What is this?", &paths, true).unwrap();
        assert!(a[0]["text"].as_str().unwrap().contains("remember the milk"));
        assert_eq!(a[1]["source"]["media_type"], "image/png");
        assert_eq!(a[2]["text"], "What is this?");
        let o = user_content("What is this?", &paths, false).unwrap();
        assert!(o[1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,iVBORw"));
        assert_eq!(user_content("plain", &[], true).unwrap(), json!("plain"));
        assert!(user_content("x", &[dir.join("missing.txt").display().to_string()], true).is_err());
        assert_eq!(pretty_tool("files__read_file"), "read file (files)");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
