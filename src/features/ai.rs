//! Quick AI and chat. A question typed in the palette streams its answer in place, and the same
//! thread takes follow-ups. Claude through the Messages API, or any OpenAI-compatible endpoint
//! (OpenAI, OpenRouter, Ollama, a local server) — chosen in Settings.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

use rune::{El, ScrollSpec, Tx, Waker, col, el, icon, row, spacer, text};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::app::{App, Built, Tone};
use crate::icons;
use crate::model::*;
use crate::theme::{Palette, Tint, radius, size, space, type_};
use crate::util;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct AiSettings {
    /// "anthropic" or "openai" (any OpenAI-compatible endpoint).
    pub provider: String,
    pub model: String,
    /// The endpoint for an OpenAI-compatible provider, e.g. `http://localhost:11434/v1`.
    pub base_url: String,
    /// Left empty, the key is read from `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`.
    pub api_key: String,
    pub system_prompt: String,
    /// More models to offer in the chat's picker, beside the provider's own.
    pub models: Vec<String>,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: "anthropic".into(), model: "claude-opus-5-5".into(), base_url: "https://api.openai.com/v1".into(),
            api_key: String::new(), system_prompt: String::new(), models: Vec::new(),
        }
    }
}

/// Claude's current models, most capable first.
const CLAUDE_MODELS: [&str; 4] = ["claude-fable-5-2", "claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];

/// `claude-opus-5-5` as "Claude Opus 5.5"; any other id as it is written.
pub fn model_name(id: &str) -> String {
    let mut parts = id.split('-');
    if parts.next() != Some("claude") {
        return id.to_string();
    }
    let (words, numbers): (Vec<&str>, Vec<&str>) = parts.partition(|p| p.chars().any(|c| c.is_alphabetic()));
    let family = words.iter().map(|w| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }).collect::<Vec<_>>().join(" ");
    let version = numbers.iter().filter(|n| n.len() < 3).copied().collect::<Vec<_>>().join(".");
    format!("Claude {family} {version}").trim().to_string()
}

impl AiSettings {
    pub fn key(&self) -> String {
        if !self.api_key.trim().is_empty() {
            return self.api_key.trim().to_string();
        }
        std::env::var(if self.provider == "anthropic" { "ANTHROPIC_API_KEY" } else { "OPENAI_API_KEY" }).unwrap_or_default()
    }
    /// Whether a request can be sent at all: a key, or a local endpoint that needs none.
    pub fn ready(&self) -> bool {
        !self.key().is_empty() || (self.provider != "anthropic" && (self.base_url.contains("localhost") || self.base_url.contains("127.0.0.1")))
    }
    pub fn provider_name(&self) -> &str {
        if self.provider == "anthropic" { "Claude" } else { "OpenAI-compatible" }
    }
    /// The models the chat's picker lists: the provider's, the user's own, and the one in use.
    pub fn choices(&self) -> Vec<String> {
        let mut out: Vec<String> = if self.provider == "anthropic" { CLAUDE_MODELS.iter().map(|m| m.to_string()).collect() } else { Vec::new() };
        for m in self.models.iter().chain(std::iter::once(&self.model)) {
            if !m.trim().is_empty() && !out.contains(m) {
                out.push(m.clone());
            }
        }
        out
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum Role {
    #[default]
    User,
    Assistant,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Message {
    pub role: Role,
    /// What the model is sent.
    pub text: String,
    /// What the thread shows in its place: a quick action's name, not its whole instruction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    /// The model that answered.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
}

impl Message {
    pub fn display(&self) -> &str {
        self.shown.as_deref().unwrap_or(&self.text)
    }
}

/// A conversation as it is kept on disk, under `~/.local/share/echospot/chats`.
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Thread {
    pub id: u64,
    pub title: String,
    pub updated: u64,
    pub messages: Vec<Message>,
}

fn chats_dir() -> std::path::PathBuf {
    let dir = util::data_dir().join("chats");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

const MAX_THREADS: usize = 40;

/// Saved conversations, newest first.
pub fn threads() -> Vec<Thread> {
    let mut out: Vec<Thread> = std::fs::read_dir(chats_dir()).into_iter().flatten().flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| util::read_json::<Thread>(&e.path())).filter(|t| t.id != 0 && !t.messages.is_empty()).collect();
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    // The store keeps the newest conversations only.
    for old in out.iter().skip(MAX_THREADS) {
        let _ = std::fs::remove_file(chats_dir().join(format!("{}.json", old.id)));
    }
    out.truncate(MAX_THREADS);
    out
}

pub fn delete_thread(id: u64) {
    let _ = std::fs::remove_file(chats_dir().join(format!("{id}.json")));
}

enum Chunk {
    Text(String),
    Error(String),
    Done,
}

#[derive(Default)]
pub struct Chat {
    /// The saved thread this is; 0 until the first question.
    pub id: u64,
    /// Quick AI: one question, one answer, the question left in the field to refine. Off, the
    /// screen is a chat that keeps its thread.
    pub quick: bool,
    pub messages: Vec<Message>,
    pub streaming: bool,
    pub error: Option<String>,
    rx: Option<Receiver<Chunk>>,
    child: Option<Child>,
    waker: Option<Waker>,
}

const DEFAULT_SYSTEM: &str = "You are answering inside a small command palette. Be direct and brief: lead with the answer, use short paragraphs, and use Markdown (lists, code fences) only where it helps.";

/// Parses one SSE `data:` payload from either wire format.
fn parse_event(anthropic: bool, payload: &str) -> Option<Chunk> {
    if payload == "[DONE]" {
        return Some(Chunk::Done);
    }
    let v: Value = serde_json::from_str(payload).ok()?;
    if let Some(message) = v.get("error").and_then(|e| e.get("message")).and_then(Value::as_str) {
        return Some(Chunk::Error(message.to_string()));
    }
    if anthropic {
        match v.get("type").and_then(Value::as_str)? {
            "content_block_delta" => {
                let delta = v.get("delta")?;
                (delta.get("type").and_then(Value::as_str) == Some("text_delta"))
                    .then(|| Chunk::Text(delta.get("text").and_then(Value::as_str).unwrap_or("").to_string()))
            }
            "message_delta" => match v.get("delta").and_then(|d| d.get("stop_reason")).and_then(Value::as_str) {
                // A decline is a normal 200 response; say so rather than show an empty answer.
                Some("refusal") => Some(Chunk::Error("The model declined this request.".into())),
                Some("max_tokens") => Some(Chunk::Text("\n\n*(The answer was cut off at the length limit.)*".into())),
                _ => None,
            },
            "message_stop" => Some(Chunk::Done),
            _ => None,
        }
    } else {
        let text = v.get("choices")?.get(0)?.get("delta")?.get("content")?.as_str()?;
        Some(Chunk::Text(text.to_string()))
    }
}

impl Chat {
    pub fn configure(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    /// Empties the thread; whether this is Quick AI or the chat is the caller's to change.
    pub fn clear(&mut self) {
        self.cancel();
        self.id = 0;
        self.messages.clear();
        self.error = None;
    }

    /// Picks a saved conversation back up.
    pub fn resume(&mut self, thread: Thread) {
        self.clear();
        self.id = thread.id;
        self.messages = thread.messages;
    }

    /// Writes the thread to disk; an unanswered question is not worth keeping.
    fn save(&self) {
        if self.id == 0 || !self.messages.iter().any(|m| m.role == Role::Assistant && !m.text.is_empty()) {
            return;
        }
        let title = self.messages.first().map(|m| util::truncate(m.display().lines().next().unwrap_or("").trim(), 80)).unwrap_or_default();
        let messages = self.messages.iter().filter(|m| !m.text.is_empty()).cloned().collect();
        let path = chats_dir().join(format!("{}.json", self.id));
        util::write_json(&path, &Thread { id: self.id, title, updated: util::now_secs(), messages });
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    /// Drops the last exchange and hands back its question, to ask again.
    pub fn take_last_question(&mut self) -> Option<Message> {
        self.cancel();
        self.error = None;
        while self.messages.last().is_some_and(|m| m.role == Role::Assistant) {
            self.messages.pop();
        }
        self.messages.pop()
    }

    pub fn cancel(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        self.rx = None;
        self.streaming = false;
    }

    pub fn last_answer(&self) -> Option<&str> {
        self.messages.iter().rev().find(|m| m.role == Role::Assistant && !m.text.is_empty()).map(|m| m.text.as_str())
    }

    /// Sends the thread plus `prompt` and starts streaming the reply.
    pub fn send(&mut self, settings: &AiSettings, prompt: &str) {
        self.send_shown(settings, prompt, None);
    }

    /// As [`Chat::send`], with the thread showing `shown` in place of the full prompt.
    pub fn send_shown(&mut self, settings: &AiSettings, prompt: &str, shown: Option<String>) {
        self.cancel();
        self.error = None;
        if self.id == 0 {
            self.id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1);
        }
        self.messages.push(Message { role: Role::User, text: prompt.to_string(), shown, model: String::new() });
        let history: Vec<Value> = self.messages.iter().filter(|m| !m.text.is_empty())
            .map(|m| json!({ "role": if m.role == Role::User { "user" } else { "assistant" }, "content": m.text })).collect();
        self.messages.push(Message { role: Role::Assistant, text: String::new(), shown: None, model: settings.model.clone() });
        if !settings.ready() {
            self.error = Some(format!("No API key. Add one in Settings, or set {}.", if settings.provider == "anthropic" { "ANTHROPIC_API_KEY" } else { "OPENAI_API_KEY" }));
            return;
        }
        let anthropic = settings.provider == "anthropic";
        let system = if settings.system_prompt.trim().is_empty() { DEFAULT_SYSTEM.to_string() } else { settings.system_prompt.clone() };
        let (url, body, mut headers) = if anthropic {
            let mut body = json!({ "model": settings.model, "max_tokens": 64000, "stream": true, "system": system, "messages": history });
            let mut headers = vec![format!("x-api-key: {}", settings.key()), "anthropic-version: 2026-10-21".to_string()];
            // Models that can decline a request route to a fallback model server-side.
            if ["claude-opus-5", "claude-fable-5-1", "claude-fable-5-2", "claude-sonnet-5-5"].iter().any(|m| settings.model.starts_with(m)) {
                body["fallbacks"] = json!("default");
                headers.push("anthropic-beta: server-side-fallback-2026-07-01".to_string());
            }
            ("https://api.anthropic.com/v1/messages".to_string(), body, headers)
        } else {
            let mut messages = vec![json!({ "role": "system", "content": system })];
            messages.extend(history);
            let mut headers = Vec::new();
            if !settings.key().is_empty() {
                headers.push(format!("Authorization: Bearer {}", settings.key()));
            }
            (format!("{}/chat/completions", settings.base_url.trim_end_matches('/')), json!({ "model": settings.model, "stream": true, "messages": messages }), headers)
        };
        headers.push("content-type: application/json".to_string());

        // The request body goes through a private file and the key through curl's stdin config,
        // so neither shows in the process list.
        let body_path = util::runtime_dir().join(format!("echospot-ai-{}.json", std::process::id()));
        if std::fs::write(&body_path, body.to_string()).is_err() {
            self.error = Some("Could not write the request.".into());
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&body_path, std::fs::Permissions::from_mode(0o600));
        let mut config = format!("url = \"{url}\"\ndata = \"@{}\"\n", body_path.display());
        for header in &headers {
            config.push_str(&format!("header = \"{}\"\n", header.replace('\\', "\\\\").replace('"', "\\\"")));
        }
        let spawned = Command::new("curl").args(["-sN", "--max-time", "600", "-K", "-"])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let Ok(mut child) = spawned else {
            self.error = Some("curl is needed to reach the model.".into());
            return;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(config.as_bytes());
        }
        let Some(stdout) = child.stdout.take() else { return };
        let (tx, rx) = channel();
        let waker = self.waker.clone();
        std::thread::Builder::new().name("echospot-ai".into()).spawn(move || {
            let wake = || {
                if let Some(waker) = &waker {
                    waker();
                }
            };
            let (mut any, mut raw) = (false, String::new());
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                match line.strip_prefix("data:") {
                    Some(payload) => {
                        if let Some(chunk) = parse_event(anthropic, payload.trim()) {
                            any = true;
                            let done = matches!(chunk, Chunk::Done | Chunk::Error(_));
                            let _ = tx.send(chunk);
                            wake();
                            if done {
                                break;
                            }
                        }
                    }
                    // An HTTP error arrives as a plain JSON body, not as events.
                    None if !line.starts_with("event:") && !line.is_empty() => raw.push_str(&line),
                    None => {}
                }
            }
            if !any {
                let message = serde_json::from_str::<Value>(&raw).ok()
                    .and_then(|v| v.get("error").and_then(|e| e.get("message").or(Some(e))).and_then(Value::as_str).map(String::from))
                    .unwrap_or_else(|| if raw.is_empty() { "No response. Check the connection and the endpoint.".into() } else { util::truncate(&raw, 200) });
                let _ = tx.send(Chunk::Error(message));
            }
            let _ = tx.send(Chunk::Done);
            let _ = std::fs::remove_file(&body_path);
            wake();
        }).ok();
        self.rx = Some(rx);
        self.child = Some(child);
        self.streaming = true;
    }

    /// Lands streamed text; true when anything changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let mut changed = false;
        let mut finished = false;
        while let Ok(chunk) = rx.try_recv() {
            changed = true;
            match chunk {
                Chunk::Text(t) => {
                    if let Some(last) = self.messages.last_mut() {
                        last.text.push_str(&t);
                    }
                }
                Chunk::Error(e) => self.error = Some(e),
                Chunk::Done => finished = true,
            }
        }
        if finished {
            self.cancel();
            self.save();
        }
        changed
    }
}

/// Asks the provider for its model list: proof the key and endpoint work, at no token cost.
/// Blocking; returns how many models answered.
pub fn probe(settings: &AiSettings) -> Result<usize, String> {
    if !settings.ready() {
        return Err("No API key yet.".into());
    }
    let anthropic = settings.provider == "anthropic";
    let url = if anthropic { "https://api.anthropic.com/v1/models".to_string() } else { format!("{}/models", settings.base_url.trim_end_matches('/')) };
    let mut config = format!("url = \"{url}\"\n");
    let mut header = |h: String| config.push_str(&format!("header = \"{}\"\n", h.replace('\\', "\\\\").replace('"', "\\\"")));
    if anthropic {
        header(format!("x-api-key: {}", settings.key()));
        header("anthropic-version: 2023-06-01".into());
    } else if !settings.key().is_empty() {
        header(format!("Authorization: Bearer {}", settings.key()));
    }
    // The key goes through curl's stdin config, so it never shows in the process list.
    let mut child = Command::new("curl").args(["-sS", "--max-time", "12", "-K", "-"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|_| "curl is needed to reach the model.".to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(config.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let reason = String::from_utf8_lossy(&out.stderr);
        return Err(util::truncate(reason.trim().trim_start_matches("curl: ").trim(), 90));
    }
    let body: Value = serde_json::from_slice(&out.stdout).map_err(|_| "The endpoint did not answer with JSON.".to_string())?;
    if let Some(error) = body.get("error") {
        return Err(util::truncate(error.get("message").and_then(Value::as_str).or(error.as_str()).unwrap_or("The provider refused the request."), 90));
    }
    Ok(body.get("data").and_then(Value::as_array).map(Vec::len).unwrap_or(0))
}

/// Text to act on: what is selected in the app behind the palette, else the clipboard.
pub fn selection() -> String {
    let primary = util::output("wl-paste", &["--primary", "--no-newline"]).unwrap_or_default();
    if !primary.trim().is_empty() { primary } else { util::output("wl-paste", &["--no-newline"]).unwrap_or_default() }
}

pub struct QuickAction {
    pub id: &'static str,
    pub title: &'static str,
    pub keywords: &'static str,
    pub icon: &'static rune::Icon,
    pub prompt: &'static str,
}

pub static QUICK_ACTIONS: &[QuickAction] = &[
    QuickAction { id: "fix", title: "Fix Spelling and Grammar", keywords: "ai proofread correct typo", icon: &icons::CHECK,
        prompt: "Fix the spelling and grammar of the text below. Keep its meaning, tone and formatting. Reply with the corrected text only." },
    QuickAction { id: "improve", title: "Improve Writing", keywords: "ai rewrite polish clearer", icon: &icons::PENCIL,
        prompt: "Rewrite the text below so it is clearer and reads well, keeping its meaning and roughly its length. Reply with the rewritten text only." },
    QuickAction { id: "summarize", title: "Summarize Selected Text", keywords: "ai summary tldr shorten", icon: &icons::TEXT,
        prompt: "Summarize the text below in a few sentences, then list the key points." },
    QuickAction { id: "explain", title: "Explain Selected Text", keywords: "ai explain meaning", icon: &icons::INFO,
        prompt: "Explain the text below plainly, for someone new to the subject." },
    QuickAction { id: "shorter", title: "Make Shorter", keywords: "ai concise trim", icon: &icons::FILTER,
        prompt: "Make the text below shorter without losing what matters. Reply with the shortened text only." },
    QuickAction { id: "translate", title: "Translate to English", keywords: "ai translation language", icon: &icons::GLOBE,
        prompt: "Translate the text below into natural English. Reply with the translation only." },
    QuickAction { id: "code", title: "Explain Code", keywords: "ai code review explain", icon: &icons::CODE,
        prompt: "Explain what the code below does, step by step, and point out anything that looks wrong." },
];

/// Runs a quick action on the current selection.
pub fn run_quick_action(app: &mut App, id: &str, tx: &mut Tx) {
    let Some(action) = QUICK_ACTIONS.iter().find(|a| a.id == id) else { return };
    let text = selection();
    if text.trim().is_empty() {
        return app.toast("Select or copy some text first", Tone::Neutral);
    }
    app.usage.record(&format!("ai:{id}"));
    app.chat.clear();
    app.chat.quick = false;
    app.push(Route::Ai, tx);
    let ai = app.settings.ai.clone();
    // The thread shows what was asked, not the whole instruction.
    let shown = format!("{}: {}", action.title, util::truncate(text.trim(), 240));
    app.chat.send_shown(&ai, &format!("{}\n\n---\n\n{}", action.prompt, text), Some(shown));
    app.refresh();
}

pub fn ask(app: &mut App, prompt: &str, tx: &mut Tx) {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return;
    }
    let ai = app.settings.ai.clone();
    if app.chat.quick {
        // Quick AI answers the line as it stands; asking again replaces the answer.
        app.chat.clear();
        app.chat.send(&ai, prompt);
        app.refresh();
        return tx.scroll_to_y("chat", 0.0);
    }
    app.chat.send(&ai, prompt);
    app.top_mut().query.clear();
    app.refresh();
    tx.scroll_to_bottom("chat");
}

/// The model picker's entries and which is in use.
pub fn model_filter(ai: &AiSettings) -> (Vec<String>, usize) {
    let ids = ai.choices();
    let at = ids.iter().position(|m| *m == ai.model).unwrap_or(0);
    (ids.iter().map(|m| model_name(m)).collect(), at)
}

/// The header's picker chose a model: it is the one every later question goes to.
pub fn set_model(app: &mut App, index: usize) {
    let Some(id) = app.settings.ai.choices().get(index).cloned() else { return };
    app.settings.ai.model = id;
    app.settings.save();
}

fn chat_actions(app: &App, asking: bool) -> Vec<Action> {
    let mut actions = Vec::new();
    // In Quick AI the question stays in the field: it is a new one only once it is edited.
    let asking = asking && !(app.chat.quick && app.chat.messages.first().is_some_and(|m| m.text.trim() == app.query().trim()));
    if app.chat.streaming {
        actions.push(Action::new("Stop", &icons::X, Act::Ai(AiOp::Stop)));
    } else if asking || app.chat.messages.is_empty() {
        actions.push(Action::new("Ask AI", &icons::AI, Act::Ai(AiOp::Send)));
    } else {
        actions.push(Action::new("Copy Answer", &icons::COPY, Act::Ai(AiOp::Copy)));
    }
    if let Some(answer) = app.chat.last_answer() {
        actions.push(Action::new("Copy Answer", &icons::COPY, Act::Copy(answer.to_string())).keys("ctrl+shift+c"));
        actions.push(Action::new(app.paste_target(), &icons::CLIPBOARD, Act::Paste(answer.to_string())).keys("ctrl+enter"));
    }
    if !app.chat.streaming && !app.chat.messages.is_empty() {
        if app.chat.quick {
            actions.push(Action::new("Continue in Chat", &icons::CHAT, Act::Ai(AiOp::Continue)).keys("ctrl+t"));
        }
        actions.push(Action::new("Ask Again", &icons::REFRESH, Act::Ai(AiOp::Retry)).keys("ctrl+r"));
    }
    actions.push(Action::new("New Chat", &icons::PLUS, Act::Ai(AiOp::New)).keys("ctrl+shift+n").section(""));
    actions.push(Action::new("AI Settings…", &icons::GEAR, Act::OpenSettings("ai".into())));
    actions
}

pub fn build(app: &App, query: &str) -> Built {
    let q = query.trim();
    let placeholder = if app.chat.messages.is_empty() || app.chat.quick { "Ask anything..." } else { "Ask a follow-up..." };
    let (models, _) = model_filter(&app.settings.ai);
    let model_names: Vec<&str> = models.iter().map(String::as_str).collect();
    if !app.chat.messages.is_empty() {
        // A thread is up: one unseen row carries its actions to the bottom bar.
        let mut item = Item::new("ai", "AI Chat", IconSrc::None);
        item.actions = chat_actions(app, !q.is_empty());
        let mut built = Built::list(Sections::default(), placeholder).keep().filters(&model_names);
        built.items = vec![item];
        built.rows = vec![Row::Item(0)];
        built.loading = app.chat.streaming;
        return built;
    }
    // Nothing asked yet: what could be, as rows. Typing narrows them and leads with the question.
    let mut sections = Sections::default();
    if !q.is_empty() {
        sections.push("", vec![Item::new("ai:ask", format!("Ask “{}”", util::truncate(q, 60)), IconSrc::Badge(&icons::AI, Tint::Violet))
            .trailing(&model_name(&app.settings.ai.model))
            .actions(chat_actions(app, true))]);
    }
    let matches = |title: &str, keys: &str| q.is_empty() || crate::features::fuzzy::score_item(q, title, keys).is_some();
    let recent: Vec<Item> = threads().into_iter().filter(|t| matches(&t.title, "")).take(if q.is_empty() { 5 } else { 12 }).map(|t| {
        let turns = t.messages.iter().filter(|m| m.role == Role::User).count();
        Item::new(format!("chat:{}", t.id), t.title.clone(), IconSrc::Glyph(&icons::CHAT))
            .subtitle(format!("{turns} question{}", if turns == 1 { "" } else { "s" }))
            .trailing(&util::ago(t.updated))
            .action(Action::new("Continue Chat", &icons::ARROW_UP_RIGHT, Act::Ai(AiOp::Resume(t.id))))
            .action(Action::new("Delete Chat", &icons::TRASH, Act::Ai(AiOp::DeleteChat(t.id))).keys("ctrl+x").destructive())
    }).collect();
    sections.push("Recent Chats", recent);
    let quick: Vec<Item> = QUICK_ACTIONS.iter().filter(|a| matches(a.title, a.keywords)).map(|a| {
        Item::new(format!("ai:{}", a.id), a.title, IconSrc::Badge(a.icon, Tint::Violet)).trailing("Selected Text")
            .action(Action::new("Run on Selected Text", &icons::AI, Act::Ai(AiOp::Quick(a.id.into()))))
    }).collect();
    sections.push("Quick Actions", quick);
    let ready = app.settings.ai.ready();
    let mut built = Built::list(sections, placeholder).filters(&model_names)
        .empty(if ready { "Ask Anything" } else { "No API Key Yet" }, if ready { "Type a question and press Return." } else { "Add one in Settings (Ctrl ,) to start." });
    if !ready {
        let (mut items, rows) = (std::mem::take(&mut built.items), std::mem::take(&mut built.rows));
        let setup = Item::new("ai:setup", "Add an API Key to Start", IconSrc::Badge(&icons::KEY, Tint::Orange)).trailing("Settings")
            .action(Action::new("Open AI Settings", &icons::GEAR, Act::OpenSettings("ai".into())));
        items.insert(0, setup);
        built.rows = std::iter::once(Row::Item(0)).chain(rows.into_iter().map(|r| match r {
            Row::Item(i) => Row::Item(i + 1),
            Row::Header { title, count, .. } => Row::Header { title, count, first: false },
        })).collect();
        built.items = items;
    }
    built
}

pub fn perform(app: &mut App, op: AiOp, tx: &mut Tx) {
    match op {
        AiOp::Send => {
            let prompt = app.top().query.text.clone();
            ask(app, &prompt, tx);
        }
        AiOp::Stop => {
            app.chat.cancel();
            app.refresh();
        }
        AiOp::Continue => {
            app.chat.quick = false;
            app.top_mut().query.clear();
            app.refresh();
            tx.scroll_to_bottom("chat");
        }
        AiOp::New => {
            app.chat.quick = false;
            app.chat.clear();
            app.top_mut().query.clear();
            app.refresh();
        }
        AiOp::Copy => {
            if let Some(answer) = app.chat.last_answer().map(String::from) {
                app.perform(Act::Copy(answer), tx);
            }
        }
        AiOp::Quick(id) => run_quick_action(app, &id, tx),
        AiOp::Open(prompt) => {
            app.usage.record("cmd:ai");
            // A question handed over from the launcher is Quick AI; the bare command is the chat.
            let quick = !prompt.trim().is_empty();
            if quick {
                app.chat.clear();
            }
            app.chat.quick = quick;
            app.push(Route::Ai, tx);
            if quick {
                app.top_mut().query.replace_text(prompt.trim());
            }
            ask(app, &prompt, tx);
        }
        AiOp::Retry => {
            if let Some(question) = app.chat.take_last_question() {
                let ai = app.settings.ai.clone();
                app.chat.send_shown(&ai, &question.text, question.shown);
                app.refresh();
                tx.scroll_to_bottom("chat");
            }
        }
        AiOp::Resume(id) => {
            if let Some(thread) = threads().into_iter().find(|t| t.id == id) {
                app.chat.resume(thread);
                app.chat.quick = false;
                app.top_mut().query.clear();
                app.refresh();
                tx.scroll_to_bottom("chat");
            }
        }
        AiOp::DeleteChat(id) => {
            delete_thread(id);
            app.refresh_keeping();
            app.toast("Chat Deleted", Tone::Success);
        }
    }
}

/// Three dots breathing in turn: the model is working and nothing has arrived yet.
fn thinking(p: &Palette) -> El {
    row().h(20.0).gap(space::XS).center_y().children((0..3).map(|i| {
        el().size(5.0, 5.0).radius(2.5).fill(p.text_secondary).looping(rune::LoopSpec::opacity(1.1, i as f64 * 0.18, 0.25, 1.0))
    }))
}

/// A quiet inline control under an answer.
fn chip(p: &Palette, key: String, glyph: &'static rune::Icon, label: &str, act: Act) -> El {
    row().key(key).h(24.0).px(space::MD).gap(space::SM).center_y().radius(12.0)
        .hover(p.row_hover).press(p.control_surface)
        .on_press_tx(move |s: &mut App, tx| s.perform(act.clone(), tx))
        .child(icon(glyph, 12.0).color(p.text_tertiary))
        .child(text(label.to_string()).font(type_::SECTION).color(p.text_secondary))
}

/// The thread: questions as quiet bubbles on the right, answers set as text under the model's
/// name, the newest at the bottom with the reply streaming in.
fn error_card(p: &Palette, error: &str) -> El {
    row().w_full().px(space::XL).py(space::LG).gap(space::LG).items_start().radius(radius::FORM_FIELD)
        .fill(p.destructive.alpha(0.10)).border(1.0, p.destructive.alpha(0.26))
        .child(icon(&icons::ALERT, 15.0).color(p.destructive))
        .child(col().grow(1.0).gap(space::XXS)
            .child(text("The model didn't answer").font(type_::ROW_TITLE).color(p.text))
            .child(text(error.to_string()).font(type_::ROW_TRAILING).color(p.text_secondary).wrap()))
}

fn page(key: &str, stick_bottom: bool, content: El) -> El {
    let fade = rune::ScrollFade::strength(size::DISSOLVE_TOP).full_at(4.0).with_end(rune::ScrollFade::strength(size::DISSOLVE_BOTTOM));
    col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).pt(size::LIST_CLIP_TOP)
        .child(col().key(key.to_string()).w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom }).scroll_fade(fade)
            .child(content.pb(size::BOTTOM_BAR_H + space::XL)))
}

/// Quick AI: the question is the line in the field, so the page is the answer alone, set from
/// its top like a document, under a caption naming who wrote it.
fn quick_body(app: &App, p: &Palette) -> El {
    let chat = &app.chat;
    let answer = chat.messages.iter().rev().find(|m| m.role == Role::Assistant);
    let model = answer.map(|m| m.model.as_str()).filter(|m| !m.is_empty()).unwrap_or(app.settings.ai.model.as_str());
    let mut sheet = col().w_full().px(space::XXL + space::XS).pt(space::LG).gap(space::LG)
        .child(row().gap(space::MD).center_y()
            .child(icon(&icons::AI, 13.0).color(Tint::Violet.stops().0))
            .child(text("Quick AI").font(type_::SECTION).color(p.text_secondary))
            .child(text("·").font(type_::SECTION).color(p.text_tertiary))
            .child(text(model_name(model)).font(type_::SECTION).color(p.text_tertiary)));
    match answer {
        Some(m) if !m.text.is_empty() => {
            sheet = sheet.child(rune_kit::markdown::Markdown::new(m.text.clone()).streaming(chat.streaming).render(&crate::ui::markdown_style(p)));
        }
        _ if chat.error.is_none() => sheet = sheet.child(thinking(p)),
        _ => {}
    }
    if let Some(error) = &chat.error {
        sheet = sheet.child(error_card(p, error));
    }
    page("chat", false, sheet)
}

pub fn body(app: &App, p: &Palette) -> El {
    if app.chat.quick {
        return quick_body(app, p);
    }
    let chat = &app.chat;
    let style = crate::ui::markdown_style(p);
    let mut thread = col().w_full().px(space::XXL).pt(space::LG).gap(space::XXL - 2.0);
    let last = chat.messages.len().saturating_sub(1);
    for (i, message) in chat.messages.iter().enumerate() {
        let live = i == last && chat.streaming;
        let turn = match message.role {
            Role::User => row().w_full().child(spacer())
                .child(col().max_w(540.0).px(space::XL + 2.0).py(space::MD).radius(radius::CODE_BLOCK + 2.0)
                    .fill(p.selection).border(1.0, p.border.alpha(0.06))
                    .child(text(message.display().to_string()).font(type_::ROW_TITLE).color(p.text).wrap().max_lines(12))),
            Role::Assistant => {
                if message.text.is_empty() && i == last && chat.error.is_some() {
                    continue;
                }
                let name = if message.model.is_empty() { app.settings.ai.model.as_str() } else { message.model.as_str() };
                let mut answer = col().grow(1.0).min_w(0.0).gap(space::SM)
                    .child(row().h(20.0).center_y().child(text(model_name(name)).font(type_::SECTION).color(p.text_secondary)));
                answer = answer.child(if message.text.is_empty() { thinking(p) } else {
                    rune_kit::markdown::Markdown::new(message.text.clone()).streaming(live).render(&style)
                });
                if !message.text.is_empty() && !live {
                    let text_ = message.text.clone();
                    let mut chips = row().ml(-space::MD).gap(space::XXS)
                        .child(chip(p, format!("copy-{i}"), &icons::COPY, "Copy", Act::Copy(text_.clone())))
                        .child(chip(p, format!("paste-{i}"), &icons::CLIPBOARD, "Paste", Act::Paste(text_)));
                    if i == last {
                        chips = chips.child(chip(p, format!("retry-{i}"), &icons::REFRESH, "Ask Again", Act::Ai(AiOp::Retry)));
                    }
                    answer = answer.child(chips);
                }
                row().w_full().gap(space::LG).items_start()
                    .child(crate::ui::badge_tile(&icons::AI, 22.0, Tint::Violet))
                    .child(answer)
            }
        };
        thread = thread.child(turn);
    }
    if let Some(error) = &chat.error {
        thread = thread.child(error_card(p, error));
    }
    page("chat", true, thread)
}
