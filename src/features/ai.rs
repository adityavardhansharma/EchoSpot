//! Quick AI and chat. A question typed in the palette streams its answer in place, and the same
//! thread takes follow-ups. Claude through the Messages API, or any OpenAI-compatible endpoint
//! (OpenAI, OpenRouter, Ollama, a local server) — chosen in Settings.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

use rune::{Color, El, ScrollSpec, Tx, Waker, col, el, icon, row, spacer, text};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::app::{App, Built, Tone};
use crate::icons;
use crate::model::*;
use crate::theme::{Palette, radius, size, space, type_};
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
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: "anthropic".into(), model: "claude-opus-5-5".into(), base_url: "https://api.openai.com/v1".into(),
            api_key: String::new(), system_prompt: String::new(),
        }
    }
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
}

#[derive(Clone, PartialEq)]
pub enum Role {
    User,
    Assistant,
}

pub struct Message {
    pub role: Role,
    pub text: String,
}

enum Chunk {
    Text(String),
    Error(String),
    Done,
}

#[derive(Default)]
pub struct Chat {
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

    pub fn clear(&mut self) {
        self.cancel();
        self.messages.clear();
        self.error = None;
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
        self.cancel();
        self.error = None;
        self.messages.push(Message { role: Role::User, text: prompt.to_string() });
        let history: Vec<Value> = self.messages.iter().filter(|m| !m.text.is_empty())
            .map(|m| json!({ "role": if m.role == Role::User { "user" } else { "assistant" }, "content": m.text })).collect();
        self.messages.push(Message { role: Role::Assistant, text: String::new() });
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
        }
        changed
    }
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
    app.push(Route::Ai, tx);
    let ai = app.settings.ai.clone();
    app.chat.send(&ai, &format!("{}\n\n---\n\n{}", action.prompt, text));
    // The thread shows what was asked, not the whole instruction.
    if let Some(first) = app.chat.messages.first_mut() {
        first.text = format!("{}: {}", action.title, util::truncate(text.trim(), 240));
    }
    app.refresh();
}

pub fn ask(app: &mut App, prompt: &str, tx: &mut Tx) {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return;
    }
    let ai = app.settings.ai.clone();
    app.chat.send(&ai, prompt);
    app.top_mut().query.clear();
    app.refresh();
    tx.scroll_to_bottom("chat");
}

pub fn build(app: &App, query: &str) -> Built {
    // One unseen row carries the screen's actions to the bottom bar.
    let mut item = Item::new("ai", "Quick AI", IconSrc::None);
    let asking = !query.trim().is_empty();
    if app.chat.streaming {
        item.actions.push(Action::new("Stop", &icons::X, Act::Ai(AiOp::Stop)));
    } else {
        item.actions.push(Action::new(if asking || app.chat.messages.is_empty() { "Ask AI" } else { "Copy Answer" }, &icons::SPARKLES,
            if asking || app.chat.messages.is_empty() { Act::Ai(AiOp::Send) } else { Act::Ai(AiOp::Copy) }));
    }
    if let Some(answer) = app.chat.last_answer() {
        item.actions.push(Action::new("Copy Answer", &icons::COPY, Act::Copy(answer.to_string())).keys("ctrl+shift+c"));
        item.actions.push(Action::new(app.paste_target(), &icons::CLIPBOARD, Act::Paste(answer.to_string())).keys("ctrl+enter"));
    }
    item.actions.push(Action::new("New Chat", &icons::PLUS, Act::Ai(AiOp::New)).keys("ctrl+shift+n").section(""));
    item.actions.push(Action::new("AI Settings…", &icons::GEAR, Act::OpenSettings("ai".into())));
    let mut built = Built::list(Sections::default(), if app.chat.messages.is_empty() { "Ask anything..." } else { "Ask a follow-up..." }).keep();
    built.items = vec![item];
    built.rows = vec![Row::Item(0)];
    built.loading = app.chat.streaming;
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
        AiOp::New => {
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
            if !prompt.trim().is_empty() {
                app.chat.clear();
            }
            app.push(Route::Ai, tx);
            ask(app, &prompt, tx);
        }
    }
}

/// The chat's page: the thread, newest at the bottom, the reply streaming in.
pub fn body(app: &App, p: &Palette) -> El {
    let chat = &app.chat;
    if chat.messages.is_empty() {
        let ai = &app.settings.ai;
        let hint = if ai.ready() { format!("{} · {}", ai.provider_name(), ai.model) } else { "Add an API key in Settings (Ctrl ,) to start.".to_string() };
        return el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).child(col().w_full().h_full().center().gap(space::MD)
            .child(el().size(size::DIALOG_SYMBOL_BOX, size::DIALOG_SYMBOL_BOX).radius(radius::DIALOG_SYMBOL).fill(p.control_surface).center()
                .child(icon(&icons::SPARKLES, 24.0).color(p.text_secondary)))
            .child(text("Ask Anything").font(type_::PANEL_TITLE).color(p.text))
            .child(text(hint).font(type_::ROW_TRAILING).color(p.text_tertiary)));
    }
    let mut thread = col().w_full().px(space::XXL + space::XS).pt(space::SM).pb(space::XXL).gap(space::XL);
    let last = chat.messages.len() - 1;
    for (i, message) in chat.messages.iter().enumerate() {
        thread = thread.child(match message.role {
            Role::User => row().w_full().child(spacer())
                .child(col().max_w(520.0).px(space::XL).py(space::MD).radius(radius::CODE_BLOCK).fill(p.control_surface)
                    .child(text(message.text.clone()).font(type_::ROW_TITLE).color(p.text).wrap())),
            Role::Assistant if message.text.is_empty() && i == last => {
                if chat.error.is_some() { el() } else {
                    row().gap(space::MD).center_y()
                        .child(icon(&icons::REFRESH, 13.0).color(p.text_tertiary).looping(rune::LoopSpec::rotation(0.9)))
                        .child(text("Thinking…").font(type_::ROW_TRAILING).color(p.text_tertiary))
                }
            }
            Role::Assistant => col().w_full().child(
                rune_kit::markdown::Markdown::new(message.text.clone()).streaming(chat.streaming && i == last)
                    .render(&rune_kit::markdown::MarkdownStyle::neutral(p.dark))),
        });
    }
    if let Some(error) = &chat.error {
        thread = thread.child(row().w_full().px(space::XL).py(space::MD).gap(space::MD).center_y().radius(radius::CARD)
            .fill(p.destructive.alpha(0.12)).border(1.0, p.destructive.alpha(0.3))
            .child(icon(&icons::ALERT, 14.0).color(p.destructive))
            .child(text(error.clone()).font(type_::ROW_TRAILING).color(p.text).wrap().grow(1.0)));
    }
    let _ = Color::WHITE;
    col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).pt(size::HEADER_BAND).pb(size::BOTTOM_BAR_H)
        .child(col().key("chat").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: true }).scroll_edge_fade(28.0).child(thread))
}
