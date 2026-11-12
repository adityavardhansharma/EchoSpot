//! Quick AI and chat. A question typed in the palette streams its answer in place, and the same
//! thread takes follow-ups. Claude through the Messages API; OpenAI, OpenRouter, Vercel AI Gateway
//! and any other OpenAI-compatible endpoint through chat completions. Each provider keeps its own
//! key, chosen in Settings.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

use rune::{El, ScrollSpec, Tx, Waker, col, el, icon, row, spacer, text};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::app::{App, Built, Tone};
use crate::features::ai_wire::{self, Chunk, Control, Job};
use crate::features::mcp;
use crate::icons;
use crate::model::*;
use crate::theme::{Palette, Tint, radius, size, space, type_};
use crate::util;

/// A place a model answers from. Four are first-class and set up the same way: pick one, paste
/// its key. The fifth is any other OpenAI-compatible server (Ollama, LM Studio, a proxy).
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    /// The API's base URL; empty for `custom`, whose endpoint is the user's own.
    pub endpoint: &'static str,
    /// The environment variable read when no key is saved.
    pub env: &'static str,
    /// Where a key is made.
    pub keys_url: &'static str,
    pub key_hint: &'static str,
    pub default_model: &'static str,
    pub model_hint: &'static str,
    /// The letter on its tile.
    pub mark: &'static str,
    pub tint: Tint,
}

pub static PROVIDERS: [Provider; 5] = [
    Provider { id: "anthropic", name: "Claude", blurb: "Anthropic's API, direct", endpoint: "https://api.anthropic.com/v1", env: "ANTHROPIC_API_KEY",
        keys_url: "https://console.anthropic.com/settings/keys", key_hint: "sk-ant-…", default_model: "claude-opus-5-5", model_hint: "claude-opus-5-5", mark: "C", tint: Tint::Orange },
    Provider { id: "openai", name: "OpenAI", blurb: "GPT models, direct", endpoint: "https://api.openai.com/v1", env: "OPENAI_API_KEY",
        keys_url: "https://platform.openai.com/api-keys", key_hint: "sk-…", default_model: "gpt-5", model_hint: "gpt-5", mark: "O", tint: Tint::Teal },
    Provider { id: "openrouter", name: "OpenRouter", blurb: "Every major model behind one key", endpoint: "https://openrouter.ai/api/v1", env: "OPENROUTER_API_KEY",
        keys_url: "https://openrouter.ai/settings/keys", key_hint: "sk-or-v1-…", default_model: "openrouter/auto", model_hint: "vendor/model", mark: "R", tint: Tint::Indigo },
    Provider { id: "vercel", name: "Vercel AI Gateway", blurb: "Vercel's gateway to many providers", endpoint: "https://ai-gateway.vercel.sh/v1", env: "AI_GATEWAY_API_KEY",
        keys_url: "https://vercel.com/ai-gateway", key_hint: "Paste an AI Gateway key", default_model: "", model_hint: "vendor/model", mark: "V", tint: Tint::Graphite },
    Provider { id: "custom", name: "Custom Endpoint", blurb: "Ollama, LM Studio, any OpenAI-compatible server", endpoint: "", env: "OPENAI_API_KEY",
        keys_url: "", key_hint: "Optional for a local server", default_model: "", model_hint: "llama3.2", mark: "", tint: Tint::Blue },
];

pub fn provider(id: &str) -> &'static Provider {
    PROVIDERS.iter().find(|p| p.id == id).unwrap_or(&PROVIDERS[PROVIDERS.len() - 1])
}

/// Where a provider's key comes from.
#[derive(Clone, Copy, PartialEq)]
pub enum KeySource {
    Saved,
    Env,
    None,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct AiSettings {
    /// A [`Provider`] id: "anthropic", "openai", "openrouter", "vercel" or "custom".
    pub provider: String,
    pub model: String,
    /// The endpoint of the `custom` provider, e.g. `http://localhost:11434/v1`.
    pub base_url: String,
    /// One key per provider, so switching never asks for a key twice. A provider without one
    /// reads its environment variable.
    pub keys: std::collections::BTreeMap<String, String>,
    /// The models used with each provider, newest first: what its picker offers.
    pub picked: std::collections::BTreeMap<String, Vec<String>>,
    /// The single key older versions kept; [`AiSettings::normalize`] files it under its provider.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub api_key: String,
    pub system_prompt: String,
    /// Let the model search the web (Claude, and models through OpenRouter).
    pub web_search: bool,
    /// Offer the model the tools of the MCP servers in `mcp.json`.
    pub tools: bool,
    /// More models to offer in the chat's picker, beside the provider's own.
    pub models: Vec<String>,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: "anthropic".into(), model: "claude-opus-5-5".into(), base_url: String::new(), keys: Default::default(),
            picked: Default::default(), api_key: String::new(), system_prompt: String::new(), web_search: false, tools: false, models: Vec::new(),
        }
    }
}

/// Claude's current models, most capable first.
const CLAUDE_MODELS: [&str; 4] = ["claude-fable-5-2", "claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];
const PICKED_MAX: usize = 6;

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

fn is_local(url: &str) -> bool {
    ["localhost", "127.0.0.1", "[::1]"].iter().any(|h| url.contains(h))
}

impl AiSettings {
    /// Brings a file written by an older version up to date: "openai" used to mean any
    /// OpenAI-compatible endpoint, and there was one key for whichever provider was chosen.
    pub fn normalize(&mut self) {
        let base = self.base_url.trim().trim_end_matches('/').to_string();
        if self.provider == "openai" && !base.is_empty() {
            match PROVIDERS.iter().find(|p| !p.endpoint.is_empty() && p.endpoint == base) {
                Some(known) => self.provider = known.id.into(),
                None => self.provider = "custom".into(),
            }
        }
        if !PROVIDERS.iter().any(|p| p.id == self.provider) {
            self.provider = "custom".into();
        }
        if self.provider != "custom" && PROVIDERS.iter().any(|p| p.endpoint == base) {
            self.base_url.clear();
        }
        let legacy = std::mem::take(&mut self.api_key);
        if !legacy.trim().is_empty() {
            self.keys.entry(self.provider.clone()).or_insert(legacy.trim().to_string());
        }
    }
    pub fn info(&self) -> &'static Provider {
        provider(&self.provider)
    }
    pub fn key_source(&self, id: &str) -> KeySource {
        if self.keys.get(id).is_some_and(|k| !k.trim().is_empty()) {
            KeySource::Saved
        } else if std::env::var(provider(id).env).is_ok_and(|k| !k.trim().is_empty()) {
            KeySource::Env
        } else {
            KeySource::None
        }
    }
    pub fn key_for(&self, id: &str) -> String {
        match self.keys.get(id).map(|k| k.trim()).filter(|k| !k.is_empty()) {
            Some(key) => key.to_string(),
            None => std::env::var(provider(id).env).map(|k| k.trim().to_string()).unwrap_or_default(),
        }
    }
    pub fn key(&self) -> String {
        self.key_for(&self.provider)
    }
    /// The base URL requests go to.
    pub fn endpoint(&self) -> String {
        let known = self.info().endpoint;
        if known.is_empty() { self.base_url.trim().trim_end_matches('/').to_string() } else { known.to_string() }
    }
    /// Whether a provider can be asked at all: a key, or a local endpoint that needs none.
    pub fn ready_for(&self, id: &str) -> bool {
        !self.key_for(id).is_empty() || (id == "custom" && is_local(&self.base_url))
    }
    pub fn ready(&self) -> bool {
        self.ready_for(&self.provider)
    }
    /// Why a question cannot be sent, in words for the person asking.
    pub fn missing(&self) -> Option<String> {
        let info = self.info();
        if !self.ready() {
            return Some(if info.id == "custom" && self.base_url.trim().is_empty() { "No endpoint yet. Add one in Settings.".into() }
                else { format!("No API key for {}. Add one in Settings, or set {}.", info.name, info.env) });
        }
        if info.id == "custom" && self.base_url.trim().is_empty() {
            return Some("No endpoint yet. Add one in Settings.".into());
        }
        self.model.trim().is_empty().then(|| format!("No model chosen for {}. Pick one in Settings.", info.name))
    }
    /// Makes `model` the one in use and remembers it among this provider's.
    pub fn use_model(&mut self, model: &str) {
        let model = model.trim();
        self.model = model.to_string();
        if model.is_empty() {
            return;
        }
        let list = self.picked.entry(self.provider.clone()).or_default();
        list.retain(|m| m != model);
        list.insert(0, model.to_string());
        list.truncate(PICKED_MAX);
    }
    /// Switches provider, coming back to the model last used with it.
    pub fn set_provider(&mut self, id: &str) {
        if self.provider == id {
            return;
        }
        let current = self.model.clone();
        self.use_model(&current);
        self.provider = provider(id).id.to_string();
        self.model = self.picked.get(id).and_then(|l| l.first()).cloned().unwrap_or_else(|| provider(id).default_model.to_string());
    }
    /// A provider's models worth offering without asking it: Claude's own, then the ones used.
    fn known_models(&self, id: &str) -> Vec<String> {
        let mut out: Vec<String> = if id == "anthropic" { CLAUDE_MODELS.iter().map(|m| m.to_string()).collect() } else { Vec::new() };
        fn add(out: &mut Vec<String>, m: &str) {
            if !m.trim().is_empty() && !out.iter().any(|o| o == m) {
                out.push(m.to_string());
            }
        }
        self.picked.get(id).into_iter().flatten().for_each(|m| add(&mut out, m));
        if id == self.provider {
            self.models.iter().for_each(|m| add(&mut out, m));
            add(&mut out, &self.model);
        } else if out.is_empty() {
            add(&mut out, provider(id).default_model);
        }
        out
    }
    /// What the chat's picker lists, as `(provider, model)`: the provider in use first, then
    /// every other one that has a key, so a model anywhere is one choice away.
    pub fn choices(&self) -> Vec<(String, String)> {
        let mut ids = vec![self.provider.as_str()];
        ids.extend(PROVIDERS.iter().map(|p| p.id).filter(|id| *id != self.provider && self.ready_for(id)));
        ids.into_iter().flat_map(|id| self.known_models(id).into_iter().map(move |m| (id.to_string(), m))).collect()
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
    /// Files sent with a question, by path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
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
    /// The instructions of the preset the chat was started from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub system: String,
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

/// A request as the provider in use wants it, ready to stream.
pub fn job_for(settings: &AiSettings, system: String, mut history: Vec<Value>, tools: bool) -> Job {
    let anthropic = settings.provider == "anthropic";
    let (url, body, headers) = if anthropic {
        let mut body = json!({ "model": settings.model, "max_tokens": 64000, "stream": true, "system": system });
        let mut headers = vec![format!("x-api-key: {}", settings.key()), "anthropic-version: 2026-10-21".to_string()];
        // Models that can decline a request route to a fallback model server-side.
        if ["claude-opus-5", "claude-fable-5-1", "claude-fable-5-2", "claude-sonnet-5-5"].iter().any(|m| settings.model.starts_with(m)) {
            body["fallbacks"] = json!("default");
            headers.push("anthropic-beta: server-side-fallback-2026-07-01".to_string());
        }
        if settings.web_search {
            // The search runs on Anthropic's side; its results arrive in the same stream.
            let older = ["claude-3", "claude-haiku-4", "claude-sonnet-4-5", "claude-opus-4-5", "claude-opus-4-1", "claude-sonnet-4-0", "claude-opus-4-0"].iter().any(|m| settings.model.starts_with(m));
            body["tools"] = json!([{ "type": if older { "web_search_20250305" } else { "web_search_20260209" }, "name": "web_search", "max_uses": 5 }]);
        }
        ("https://api.anthropic.com/v1/messages".to_string(), body, headers)
    } else {
        history.insert(0, json!({ "role": "system", "content": system }));
        let mut headers = Vec::new();
        if !settings.key().is_empty() {
            headers.push(format!("Authorization: Bearer {}", settings.key()));
        }
        let mut body = json!({ "model": settings.model, "stream": true });
        if settings.provider == "openrouter" {
            headers.push("X-Title: EchoSpot".to_string());
            if settings.web_search {
                body["plugins"] = json!([{ "id": "web" }]);
            }
        }
        (format!("{}/chat/completions", settings.endpoint()), body, headers)
    };
    Job { anthropic, url, headers, body, messages: history, tools }
}

/// One question, answered whole: what an extension's `AI.ask` gets. Blocking.
pub fn one_shot(settings: &AiSettings, prompt: &str) -> Result<String, String> {
    if let Some(why) = settings.missing() {
        return Err(why);
    }
    let mut plain = settings.clone();
    plain.web_search = false;
    let system = if settings.system_prompt.trim().is_empty() { "Answer directly and concisely.".to_string() } else { settings.system_prompt.clone() };
    let job = job_for(&plain, system, vec![json!({ "role": "user", "content": prompt })], false);
    let (tx, rx) = channel();
    ai_wire::run(job, tx, || {}, Control::default());
    let (mut text, mut error) = (String::new(), None);
    for chunk in rx.try_iter() {
        match chunk {
            Chunk::Text(t) => text.push_str(&t),
            Chunk::Error(e) => error = Some(e),
            _ => {}
        }
    }
    match error {
        Some(e) if text.is_empty() => Err(e),
        _ => Ok(text),
    }
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
    /// What the model is doing while no text arrives: searching, using a tool.
    pub status: Option<String>,
    /// Files waiting to go with the next question.
    pub pending: Vec<String>,
    /// The instructions this chat runs under when it was started from a preset, and its name.
    pub system: String,
    pub preset: String,
    rx: Option<Receiver<Chunk>>,
    control: Control,
    waker: Option<Waker>,
}

const DEFAULT_SYSTEM: &str = "You are answering inside a small command palette. Be direct and brief: lead with the answer, use short paragraphs, and use Markdown (lists, code fences) only where it helps.";

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
        self.pending.clear();
        self.system.clear();
        self.preset.clear();
    }

    /// Picks a saved conversation back up.
    pub fn resume(&mut self, thread: Thread) {
        self.clear();
        self.id = thread.id;
        self.messages = thread.messages;
        self.system = thread.system;
    }

    /// Writes the thread to disk; an unanswered question is not worth keeping.
    fn save(&self) {
        if self.id == 0 || !self.messages.iter().any(|m| m.role == Role::Assistant && !m.text.is_empty()) {
            return;
        }
        let title = self.messages.first().map(|m| util::truncate(m.display().lines().next().unwrap_or("").trim(), 80)).unwrap_or_default();
        let messages = self.messages.iter().filter(|m| !m.text.is_empty()).cloned().collect();
        let path = chats_dir().join(format!("{}.json", self.id));
        util::write_json(&path, &Thread { id: self.id, title, updated: util::now_secs(), messages, system: self.system.clone() });
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
        self.control.cancel();
        self.rx = None;
        self.streaming = false;
        self.status = None;
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
        let attachments = std::mem::take(&mut self.pending);
        self.messages.push(Message { role: Role::User, text: prompt.to_string(), shown, model: String::new(), attachments });
        let anthropic = settings.provider == "anthropic";
        let last = self.messages.len() - 1;
        let (mut history, mut unreadable) = (Vec::new(), None);
        for (i, m) in self.messages.iter().enumerate().filter(|(_, m)| !m.text.is_empty()) {
            let content = match m.role {
                Role::User => match ai_wire::user_content(&m.text, &m.attachments, anthropic) {
                    Ok(content) => content,
                    // A file of an earlier question may be gone by now; the question still stands.
                    Err(_) if i != last => json!(m.text),
                    Err(e) => {
                        unreadable = Some(e);
                        json!(m.text)
                    }
                },
                Role::Assistant => json!(m.text),
            };
            history.push(json!({ "role": if m.role == Role::User { "user" } else { "assistant" }, "content": content }));
        }
        self.messages.push(Message { role: Role::Assistant, text: String::new(), shown: None, model: settings.model.clone(), attachments: Vec::new() });
        if let Some(why) = unreadable.or_else(|| settings.missing()) {
            self.error = Some(why);
            return;
        }
        let system = if !self.system.trim().is_empty() { self.system.clone() }
            else if settings.system_prompt.trim().is_empty() { DEFAULT_SYSTEM.to_string() } else { settings.system_prompt.clone() };
        let job = job_for(settings, system, history, settings.tools && mcp::any_configured());
        let (tx, rx) = channel();
        let waker = self.waker.clone();
        self.control = Control::default();
        let control = self.control.clone();
        let started = std::thread::Builder::new().name("echospot-ai".into()).spawn(move || {
            ai_wire::run(job, tx, || {
                if let Some(waker) = &waker {
                    waker();
                }
            }, control);
        });
        if started.is_err() {
            self.error = Some("Could not start the request.".into());
            return;
        }
        self.rx = Some(rx);
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
                    self.status = None;
                    if let Some(last) = self.messages.last_mut() {
                        last.text.push_str(&t);
                    }
                }
                Chunk::Status(text) => self.status = Some(text),
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

/// What a provider said when asked whether it is there.
pub struct Probed {
    /// The chat models it offers, in its own order.
    pub models: Vec<String>,
    /// Something worth a few words beside "Connected": credit left, a key that could not be checked.
    pub note: String,
}

/// One GET, the key in a header. The key goes through curl's stdin config, so it never shows in
/// the process list.
fn fetch(url: &str, headers: &[String]) -> Result<Value, String> {
    let mut config = format!("url = \"{url}\"\n");
    for h in headers {
        config.push_str(&format!("header = \"{}\"\n", h.replace('\\', "\\\\").replace('"', "\\\"")));
    }
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
    Ok(body)
}

/// Ids that are not for chatting, on a list that mixes everything a provider sells.
fn chat_model(entry: &Value, id: &str) -> bool {
    if let Some(kind) = entry.get("type").and_then(Value::as_str) {
        if kind != "language" && kind != "model" && kind != "chat" {
            return false;
        }
    }
    !["embed", "tts", "whisper", "dall-e", "moderation", "transcribe", "realtime", "audio", "image", "sora"].iter().any(|w| id.contains(w))
}

/// Asks the provider for its model list, and a gateway whose list is public whether it knows the
/// key: proof the key and endpoint work, at no token cost. Blocking.
pub fn probe(settings: &AiSettings) -> Result<Probed, String> {
    if let Some(why) = settings.missing().filter(|_| !settings.ready()) {
        return Err(why);
    }
    let (id, base, key) = (settings.provider.as_str(), settings.endpoint(), settings.key());
    if base.is_empty() {
        return Err("No endpoint yet.".into());
    }
    let headers: Vec<String> = match id {
        "anthropic" => vec![format!("x-api-key: {key}"), "anthropic-version: 2023-06-01".into()],
        _ if key.is_empty() => Vec::new(),
        _ => vec![format!("Authorization: Bearer {key}")],
    };
    let listing = fetch(&format!("{base}/models{}", if id == "anthropic" { "?limit=100" } else { "" }), &headers)?;
    let models: Vec<String> = listing.get("data").and_then(Value::as_array).into_iter().flatten()
        .filter_map(|m| m.get("id").and_then(Value::as_str).filter(|id| chat_model(m, id)).map(String::from)).collect();
    // These two list their models to anyone, so the list proves nothing about the key.
    let note = match id {
        "openrouter" => {
            fetch(&format!("{base}/key"), &headers)?;
            String::new()
        }
        "vercel" => match fetch(&format!("{base}/credits"), &headers) {
            Ok(v) => match v.get("balance").and_then(|b| b.as_str().map(String::from).or(b.as_f64().map(|f| f.to_string()))).and_then(|b| b.parse::<f64>().ok()) {
                Some(balance) => format!("${balance:.2} credit"),
                None => String::new(),
            },
            Err(e) if e.contains("JSON") => "key not checked".into(),
            Err(e) => return Err(e),
        },
        _ => String::new(),
    };
    Ok(Probed { models, note })
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

/// A quick action as the screens list it: one of EchoSpot's own, or one a person wrote.
pub struct Quick {
    pub id: String,
    pub title: String,
    pub keywords: String,
    pub icon: &'static rune::Icon,
    pub prompt: String,
    /// The id in the library, for an action that can be edited.
    pub custom: Option<String>,
}

/// Every quick action: the built-in ones, then the person's own.
pub fn quick_actions(app: &App) -> Vec<Quick> {
    QUICK_ACTIONS.iter().map(|a| Quick { id: a.id.into(), title: a.title.into(), keywords: a.keywords.into(), icon: a.icon, prompt: a.prompt.into(), custom: None })
        .chain(app.ai_library.actions.iter().map(|a| Quick {
            id: format!("custom-{}", a.id), title: a.title.clone(), keywords: "ai custom action".into(), icon: &icons::BOLT, prompt: a.prompt.clone(), custom: Some(a.id.clone()),
        })).collect()
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
    let Some(action) = quick_actions(app).into_iter().find(|a| a.id == id) else { return };
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

/// The model picker's entries and which is in use. A model of another provider says whose it is.
pub fn model_filter(ai: &AiSettings) -> (Vec<String>, usize) {
    let choices = ai.choices();
    let at = choices.iter().position(|(p, m)| *p == ai.provider && *m == ai.model).unwrap_or(0);
    (choices.iter().map(|(p, m)| if *p == ai.provider { model_name(m) } else { format!("{} · {}", model_name(m), provider(p).name) }).collect(), at)
}

/// The header's picker chose a model: it, and its provider, take every later question.
pub fn set_model(app: &mut App, index: usize) {
    let Some((provider, model)) = app.settings.ai.choices().get(index).cloned() else { return };
    app.settings.ai.set_provider(&provider);
    app.settings.ai.use_model(&model);
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
    let ai = &app.settings.ai;
    actions.push(Action::new("Attach from Clipboard", &icons::UPLOAD, Act::Ai(AiOp::AttachClipboard)).keys("ctrl+shift+a").section("This Question"));
    if !app.chat.pending.is_empty() {
        actions.push(Action::new("Remove Attachments", &icons::X, Act::Ai(AiOp::ClearAttachments)));
    }
    actions.push(Action::new(if ai.web_search { "Turn Web Search Off" } else { "Turn Web Search On" }, &icons::GLOBE, Act::Ai(AiOp::ToggleWeb)).keys("ctrl+shift+w"));
    if mcp::any_configured() {
        actions.push(Action::new(if ai.tools { "Turn Tools Off" } else { "Turn Tools On" }, &icons::BOLT, Act::Ai(AiOp::ToggleTools)).keys("ctrl+shift+m"));
    }
    actions.push(Action::new("New Chat", &icons::PLUS, Act::Ai(AiOp::New)).keys("ctrl+shift+n").section(""));
    actions.push(Action::new("AI Settings…", &icons::GEAR, Act::OpenSettings("ai".into())));
    actions
}

pub fn build(app: &App, query: &str) -> Built {
    let q = query.trim();
    let asking_preset = format!("Ask {}...", app.chat.preset);
    let placeholder = if !app.chat.preset.is_empty() && app.chat.messages.is_empty() { asking_preset.as_str() }
        else if app.chat.messages.is_empty() || app.chat.quick { "Ask anything..." } else { "Ask a follow-up..." };
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
    // What the next question carries: a preset's instructions, files, the web, tools.
    let mut carrying: Vec<Item> = Vec::new();
    if !app.chat.preset.is_empty() {
        carrying.push(Item::new("ai:preset", app.chat.preset.clone(), IconSrc::Badge(&icons::PERSON, Tint::Violet)).trailing("Preset")
            .action(Action::new("Start Without the Preset", &icons::X, Act::Ai(AiOp::New))));
    }
    for (i, path) in app.chat.pending.iter().enumerate() {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());
        carrying.push(Item::new(format!("ai:attached:{i}"), name, IconSrc::Badge(&icons::FILE, Tint::Blue)).trailing("Attached")
            .action(Action::new("Remove Attachments", &icons::X, Act::Ai(AiOp::ClearAttachments))));
    }
    if app.settings.ai.web_search {
        carrying.push(Item::new("ai:web", "Web Search", IconSrc::Badge(&icons::GLOBE, Tint::Blue)).trailing("On")
            .action(Action::new("Turn Web Search Off", &icons::GLOBE, Act::Ai(AiOp::ToggleWeb))));
    }
    if app.settings.ai.tools && mcp::any_configured() {
        carrying.push(Item::new("ai:tools", "Tools from MCP Servers", IconSrc::Badge(&icons::BOLT, Tint::Orange)).trailing("On")
            .action(Action::new("Turn Tools Off", &icons::BOLT, Act::Ai(AiOp::ToggleTools))));
    }
    if q.is_empty() {
        sections.push("This Chat", carrying);
    }
    let presets: Vec<Item> = app.ai_library.presets.iter().filter(|p| matches(&p.name, "preset chat")).map(|p| {
        let mut item = Item::new(format!("preset:{}", p.id), p.name.clone(), IconSrc::Badge(&icons::PERSON, Tint::Violet))
            .subtitle(util::truncate(p.instructions.lines().next().unwrap_or(""), 60)).trailing(&if p.model.is_empty() { "Preset".to_string() } else { model_name(&p.model) })
            .action(Action::new("New Chat with Preset", &icons::CHAT, Act::Ai(AiOp::UsePreset(p.id.clone()))))
            .action(Action::new("Edit Preset", &icons::PENCIL, Act::Ai(AiOp::EditPreset(p.id.clone()))).keys("ctrl+e"))
            .action(Action::new("Delete Preset", &icons::TRASH, Act::Confirm(Box::new(Confirm {
                title: "Delete Preset?".into(), message: p.name.clone(), icon: &icons::TRASH, confirm: "Delete".into(), destructive: true, act: Act::Ai(AiOp::DeletePreset(p.id.clone())),
            }))).keys("ctrl+x").destructive());
        item.accessories.clear();
        item
    }).collect();
    sections.push("Presets", presets);
    let recent: Vec<Item> = threads().into_iter().filter(|t| matches(&t.title, "")).take(if q.is_empty() { 5 } else { 12 }).map(|t| {
        let turns = t.messages.iter().filter(|m| m.role == Role::User).count();
        Item::new(format!("chat:{}", t.id), t.title.clone(), IconSrc::Glyph(&icons::CHAT))
            .subtitle(format!("{turns} question{}", if turns == 1 { "" } else { "s" }))
            .trailing(&util::ago(t.updated))
            .action(Action::new("Continue Chat", &icons::ARROW_UP_RIGHT, Act::Ai(AiOp::Resume(t.id))))
            .action(Action::new("Delete Chat", &icons::TRASH, Act::Ai(AiOp::DeleteChat(t.id))).keys("ctrl+x").destructive())
    }).collect();
    sections.push("Recent Chats", recent);
    let quick: Vec<Item> = quick_actions(app).into_iter().filter(|a| matches(&a.title, &a.keywords)).map(|a| {
        let mut item = Item::new(format!("ai:{}", a.id), a.title.clone(), IconSrc::Badge(a.icon, Tint::Violet)).trailing("Selected Text")
            .action(Action::new("Run on Selected Text", &icons::AI, Act::Ai(AiOp::Quick(a.id.clone()))));
        if let Some(id) = a.custom {
            item = item.action(Action::new("Edit AI Action", &icons::PENCIL, Act::Ai(AiOp::EditAction(id.clone()))).keys("ctrl+e"))
                .action(Action::new("Delete AI Action", &icons::TRASH, Act::Confirm(Box::new(Confirm {
                    title: "Delete AI Action?".into(), message: a.title.clone(), icon: &icons::TRASH, confirm: "Delete".into(), destructive: true, act: Act::Ai(AiOp::DeleteAction(id)),
                }))).keys("ctrl+x").destructive());
        }
        item
    }).collect();
    sections.push("Quick Actions", quick);
    let create: Vec<Item> = [("ai:new-action", "Create AI Action", "new custom quick action prompt", AiOp::NewAction), ("ai:new-preset", "Create Chat Preset", "new preset instructions persona", AiOp::NewPreset)]
        .into_iter().filter(|(_, title, keys, _)| matches(title, keys))
        .map(|(id, title, _, op)| Item::new(id, title, IconSrc::Badge(&icons::PLUS, Tint::Violet)).trailing("Command").action(Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Ai(op)))).collect();
    sections.push("Make Your Own", create);
    // A provider with a key but no model chosen cannot answer either.
    let missing = app.settings.ai.missing();
    let ready = app.settings.ai.ready();
    let mut built = Built::list(sections, placeholder).filters(&model_names)
        .empty(if ready { "Ask Anything" } else { "No Provider Connected" }, if ready { "Type a question and press Return." } else { "Add a key in Settings (Ctrl ,) to start." });
    if ready && let Some(why) = &missing {
        let (mut items, rows) = (std::mem::take(&mut built.items), std::mem::take(&mut built.rows));
        items.insert(0, Item::new("ai:setup", "Choose a Model to Start", IconSrc::Badge(&icons::ALERT, Tint::Orange)).subtitle(why.clone()).trailing("Settings")
            .action(Action::new("Open AI Settings", &icons::GEAR, Act::OpenSettings("ai".into()))));
        built.rows = std::iter::once(Row::Item(0)).chain(rows.into_iter().map(|r| match r {
            Row::Item(i) => Row::Item(i + 1),
            Row::Header { title, count, .. } => Row::Header { title, count, first: false },
        })).collect();
        built.items = items;
    }
    if !ready {
        let (mut items, rows) = (std::mem::take(&mut built.items), std::mem::take(&mut built.rows));
        let setup = Item::new("ai:setup", format!("Connect {} to Start", app.settings.ai.info().name), IconSrc::Badge(&icons::KEY, Tint::Orange))
            .subtitle("Claude, OpenAI, OpenRouter or Vercel AI Gateway").trailing("Settings")
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
        AiOp::NewAction => crate::features::ai_library::open_action(app, None, tx),
        AiOp::EditAction(id) => crate::features::ai_library::open_action(app, Some(id), tx),
        AiOp::DeleteAction(id) => crate::features::ai_library::delete_action(app, &id),
        AiOp::NewPreset => crate::features::ai_library::open_preset(app, None, tx),
        AiOp::EditPreset(id) => crate::features::ai_library::open_preset(app, Some(id), tx),
        AiOp::DeletePreset(id) => crate::features::ai_library::delete_preset(app, &id),
        AiOp::UsePreset(id) => {
            let Some(preset) = app.ai_library.presets.iter().find(|p| p.id == id).cloned() else { return };
            app.chat.clear();
            app.chat.quick = false;
            app.chat.system = preset.instructions;
            app.chat.preset = preset.name;
            if !preset.model.is_empty() {
                app.settings.ai.use_model(&preset.model);
                app.settings.save();
            }
            if app.top().route != Route::Ai {
                app.push(Route::Ai, tx);
            }
            app.top_mut().query.clear();
            app.top_mut().filter = model_filter(&app.settings.ai).1;
            app.refresh();
        }
        AiOp::Attach(path) => {
            if let Err(why) = ai_wire::attachable(&path) {
                return app.toast(why, Tone::Danger);
            }
            if app.top().route != Route::Ai {
                app.chat.clear();
                app.chat.quick = false;
                app.push(Route::Ai, tx);
            }
            let path = path.display().to_string();
            if !app.chat.pending.contains(&path) {
                app.chat.pending.push(path);
            }
            app.refresh();
            app.toast("Attached. Ask a question about it.", Tone::Success);
        }
        AiOp::AttachClipboard => match clipboard_file() {
            Ok(path) => perform(app, AiOp::Attach(path), tx),
            Err(why) => app.toast(why, Tone::Neutral),
        },
        AiOp::ClearAttachments => {
            app.chat.pending.clear();
            app.refresh();
        }
        AiOp::ToggleWeb => {
            let ai = &mut app.settings.ai;
            if !ai.web_search && !matches!(ai.provider.as_str(), "anthropic" | "openrouter") {
                return app.toast("Web search works with Claude and with OpenRouter models", Tone::Neutral);
            }
            ai.web_search = !ai.web_search;
            let on = ai.web_search;
            app.settings.save();
            app.refresh_keeping();
            app.toast(if on { "Web Search On" } else { "Web Search Off" }, Tone::Success);
        }
        AiOp::ToggleTools => {
            app.settings.ai.tools = !app.settings.ai.tools;
            let on = app.settings.ai.tools;
            app.settings.save();
            app.refresh_keeping();
            app.toast(if on { "Tools On" } else { "Tools Off" }, Tone::Success);
        }
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

/// What is on the clipboard as a file to attach: a copied image, or the path of a copied file.
fn clipboard_file() -> Result<std::path::PathBuf, String> {
    let types = util::output("wl-paste", &["--list-types"]).unwrap_or_default();
    if let Some(kind) = ["image/png", "image/jpeg", "image/webp"].into_iter().find(|k| types.lines().any(|l| l.trim() == *k)) {
        let dir = util::data_dir().join("attachments");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("clipboard-{}.{}", util::now_secs(), kind.rsplit('/').next().unwrap_or("png").replace("jpeg", "jpg")));
        let saved = util::sh_ok(&format!("wl-paste --type {kind} > {}", util::quote(&path.display().to_string())));
        return if saved { Ok(path) } else { Err("Could not read the image on the clipboard".into()) };
    }
    let text = util::output("wl-paste", &["--no-newline"]).unwrap_or_default();
    let first = text.lines().next().unwrap_or("").trim();
    let path = match first.strip_prefix("file://") {
        Some(rest) => String::from_utf8_lossy(&crate::ext::net::percent_decode(rest)).to_string(),
        None => first.replacen("~/", &format!("{}/", util::home().display()), 1),
    };
    if path.starts_with('/') && std::path::Path::new(&path).is_file() { Ok(path.into()) } else { Err("Copy an image or a file first".into()) }
}

/// What a question carries beside its words, as a quiet line: files, the web, tools.
fn carrying_line(app: &App) -> Option<String> {
    let mut parts: Vec<String> = app.chat.pending.iter().map(|p| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.clone())).collect();
    if app.settings.ai.web_search {
        parts.push("Web search on".into());
    }
    if app.settings.ai.tools && mcp::any_configured() {
        parts.push("Tools on".into());
    }
    (!parts.is_empty()).then(|| parts.join("  ·  "))
}

fn carrying_row(app: &App, p: &Palette) -> Option<El> {
    carrying_line(app).map(|line| row().gap(space::SM).center_y()
        .child(icon(if !app.chat.pending.is_empty() { &icons::FILE } else if app.settings.ai.web_search { &icons::GLOBE } else { &icons::BOLT }, 12.0).color(p.text_tertiary))
        .child(text(line).font(type_::SECTION).color(p.text_tertiary).truncate()))
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
    if let (true, Some(status)) = (chat.streaming, &chat.status) {
        sheet = sheet.child(text(status.clone()).font(type_::SECTION).color(p.text_secondary));
    }
    if let Some(line) = carrying_row(app, p) {
        sheet = sheet.child(line);
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
                    .child(text(match message.attachments.as_slice() {
                        [] => message.display().to_string(),
                        files => format!("{}\n📎 {}", message.display(), files.iter().map(|f| std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).collect::<Vec<_>>().join(", ")),
                    }).font(type_::ROW_TITLE).color(p.text).wrap().max_lines(12))),
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
    if let (true, Some(status)) = (chat.streaming, &chat.status) {
        thread = thread.child(text(status.clone()).font(type_::SECTION).color(p.text_secondary).ml(22.0 + space::LG));
    }
    if let Some(error) = &chat.error {
        thread = thread.child(error_card(p, error));
    }
    if let Some(line) = carrying_row(app, p) {
        thread = thread.child(line.ml(22.0 + space::LG));
    }
    page("chat", true, thread)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn old(json: &str) -> AiSettings {
        let mut settings: AiSettings = serde_json::from_str(json).unwrap();
        settings.normalize();
        settings
    }

    /// A settings file from before each provider had its own key must come through with its key.
    #[test]
    fn older_settings_keep_their_key() {
        // The first default: Claude, with the unused OpenAI endpoint beside it.
        let s = old(r#"{"provider":"anthropic","model":"claude-opus-5-5","base_url":"https://api.openai.com/v1","api_key":"sk-ant-1"}"#);
        assert_eq!((s.provider.as_str(), s.key_for("anthropic").as_str(), s.base_url.as_str(), s.api_key.as_str()), ("anthropic", "sk-ant-1", "", ""));
        // "openai" used to mean any compatible endpoint: OpenAI itself stays OpenAI,
        let s = old(r#"{"provider":"openai","model":"gpt-5","base_url":"https://api.openai.com/v1/","api_key":"sk-1"}"#);
        assert_eq!((s.provider.as_str(), s.key_for("openai").as_str(), s.endpoint().as_str()), ("openai", "sk-1", "https://api.openai.com/v1"));
        // OpenRouter's address becomes OpenRouter,
        let s = old(r#"{"provider":"openai","model":"x/y","base_url":"https://openrouter.ai/api/v1","api_key":"sk-or-1"}"#);
        assert_eq!((s.provider.as_str(), s.key_for("openrouter").as_str()), ("openrouter", "sk-or-1"));
        // and anything else is a custom endpoint that keeps its address.
        let s = old(r#"{"provider":"openai","model":"llama3.2","base_url":"http://localhost:11434/v1","api_key":""}"#);
        assert_eq!((s.provider.as_str(), s.endpoint().as_str(), s.ready()), ("custom", "http://localhost:11434/v1", true));
        // A key already filed under its provider is not overwritten by the old single key.
        let s = old(r#"{"provider":"anthropic","keys":{"anthropic":"new"},"api_key":"old"}"#);
        assert_eq!(s.key_for("anthropic"), "new");
        // A provider that no longer exists falls back to the custom one rather than failing.
        assert_eq!(old(r#"{"provider":"mystery"}"#).provider, "custom");
    }

    #[test]
    fn switching_providers_remembers_models() {
        let mut s = AiSettings::default();
        s.use_model("claude-sonnet-5-5");
        s.set_provider("openai");
        assert_eq!(s.model, "gpt-5");
        s.use_model("gpt-5-mini");
        s.set_provider("anthropic");
        assert_eq!(s.model, "claude-sonnet-5-5");
        s.set_provider("openai");
        assert_eq!(s.model, "gpt-5-mini");
    }
}
