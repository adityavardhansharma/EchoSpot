//! A client for Model Context Protocol servers that speak over stdio, so a chat can use their
//! tools. Servers are listed in `~/.config/echospot/mcp.json`, in the shape Claude Desktop and
//! Cursor use: `{ "mcpServers": { "name": { "command": "npx", "args": [...], "env": {...} } } }`.
//! A server starts the first time its tools are wanted and stays up while EchoSpot runs.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};

use crate::util;

#[derive(Clone, Debug)]
pub struct Tool {
    pub server: String,
    pub name: String,
    /// The name the model is given: `server__tool`, in the characters every provider allows.
    pub wire: String,
    pub description: String,
    pub schema: Value,
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
    tools: Vec<Tool>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn config_path() -> std::path::PathBuf {
    util::config_dir().join("mcp.json")
}

/// The servers `mcp.json` lists that can be started: name and its entry.
fn configured() -> Vec<(String, Value)> {
    let config: Value = util::read_json(&config_path());
    let servers = config.get("mcpServers").or_else(|| config.get("servers")).and_then(Value::as_object).cloned().unwrap_or_default();
    servers.into_iter().filter(|(_, entry)| entry.get("command").and_then(Value::as_str).is_some_and(|c| !c.is_empty())
        && entry.get("disabled").and_then(Value::as_bool) != Some(true)).collect()
}

/// Whether any server is set up: the chat offers its tools switch only then.
pub fn any_configured() -> bool {
    !configured().is_empty()
}

fn servers() -> &'static Mutex<HashMap<String, Server>> {
    static SERVERS: OnceLock<Mutex<HashMap<String, Server>>> = OnceLock::new();
    SERVERS.get_or_init(Default::default)
}

fn wire_name(server: &str, tool: &str) -> String {
    let clean = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect::<String>();
    format!("{}__{}", clean(server), clean(tool)).chars().take(64).collect()
}

impl Server {
    fn start(name: &str, entry: &Value) -> Result<Server, String> {
        let command = entry.get("command").and_then(Value::as_str).unwrap_or("");
        let args: Vec<String> = entry.get("args").and_then(Value::as_array).into_iter().flatten().filter_map(|a| a.as_str().map(String::from)).collect();
        let mut cmd = Command::new(command);
        cmd.args(&args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        for (key, value) in entry.get("env").and_then(Value::as_object).into_iter().flatten() {
            if let Some(value) = value.as_str() {
                cmd.env(key, value);
            }
        }
        if let Some(dir) = entry.get("cwd").and_then(Value::as_str) {
            cmd.current_dir(dir);
        }
        let mut child = cmd.spawn().map_err(|e| format!("{name}: could not start `{command}` ({e})"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        // A thread turns the server's output into lines, so a server that says nothing can be
        // given up on instead of waited on forever.
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut server = Server { child, stdin, lines, next_id: 1, tools: Vec::new() };
        server.request("initialize", json!({
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": { "name": "echospot", "version": env!("CARGO_PKG_VERSION") },
        }), Duration::from_secs(30)).map_err(|e| format!("{name}: {e}"))?;
        server.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))?;
        let listed = server.request("tools/list", json!({}), Duration::from_secs(30)).map_err(|e| format!("{name}: {e}"))?;
        server.tools = listed.get("tools").and_then(Value::as_array).into_iter().flatten().filter_map(|t| {
            let tool = t.get("name")?.as_str()?;
            Some(Tool {
                server: name.to_string(), name: tool.to_string(), wire: wire_name(name, tool),
                description: t.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                schema: t.get("inputSchema").cloned().filter(Value::is_object).unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
            })
        }).collect();
        Ok(server)
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        writeln!(self.stdin, "{message}").and_then(|_| self.stdin.flush()).map_err(|_| "the server closed its input".to_string())
    }

    /// One request and its answer; whatever else the server says meanwhile is passed over.
    fn request(&mut self, method: &str, params: Value, wait: Duration) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let deadline = std::time::Instant::now() + wait;
        loop {
            let left = deadline.checked_duration_since(std::time::Instant::now()).ok_or_else(|| format!("no answer to {method} in {} s", wait.as_secs()))?;
            let line = self.lines.recv_timeout(left).map_err(|_| format!("no answer to {method}"))?;
            let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(error.get("message").and_then(Value::as_str).unwrap_or("the server reported an error").to_string());
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

/// Every tool of every configured server, starting those not yet running. Blocking: a server
/// such as `npx …` can take a while to come up the first time. Also what went wrong, by server.
pub fn tools() -> (Vec<Tool>, Vec<String>) {
    let (mut out, mut problems) = (Vec::new(), Vec::new());
    let Ok(mut running) = servers().lock() else { return (out, problems) };
    let wanted = configured();
    running.retain(|name, _| wanted.iter().any(|(n, _)| n == name));
    for (name, entry) in wanted {
        if !running.contains_key(&name) {
            match Server::start(&name, &entry) {
                Ok(server) => {
                    running.insert(name.clone(), server);
                }
                Err(e) => {
                    problems.push(e);
                    continue;
                }
            }
        }
        out.extend(running[&name].tools.iter().cloned());
    }
    (out, problems)
}

/// Runs a tool by the name the model used. The text it returned, or why it failed.
pub fn call(wire: &str, arguments: &Value) -> Result<String, String> {
    let mut running = servers().lock().map_err(|_| "the tool servers are unavailable".to_string())?;
    let (server, tool) = running.iter().find_map(|(name, s)| s.tools.iter().find(|t| t.wire == wire).map(|t| (name.clone(), t.name.clone())))
        .ok_or_else(|| format!("no tool named {wire}"))?;
    let arguments = if arguments.is_object() { arguments.clone() } else { json!({}) };
    let result = running.get_mut(&server).ok_or("the server stopped")?
        .request("tools/call", json!({ "name": tool, "arguments": arguments }), Duration::from_secs(120));
    let result = match result {
        Ok(result) => result,
        Err(e) => {
            // A server that stopped answering is started afresh the next time.
            running.remove(&server);
            return Err(e);
        }
    };
    let text: Vec<String> = result.get("content").and_then(Value::as_array).into_iter().flatten().map(|block| {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => block.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
            Some(other) => format!("[{other} content]"),
            None => String::new(),
        }
    }).collect();
    let text = if text.is_empty() { result.get("structuredContent").map(|v| v.to_string()).unwrap_or_default() } else { text.join("\n") };
    if result.get("isError").and_then(Value::as_bool) == Some(true) { Err(if text.is_empty() { "the tool failed".into() } else { text }) } else { Ok(text) }
}

/// `echospot mcp`: the configured servers and what each offers.
pub fn cli() {
    if !any_configured() {
        return println!("No MCP servers. List them in {} as {{ \"mcpServers\": {{ \"name\": {{ \"command\": \"…\", \"args\": [] }} }} }}.", config_path().display());
    }
    let (tools, problems) = tools();
    for problem in problems {
        println!("! {problem}");
    }
    let mut server = String::new();
    for tool in tools {
        if tool.server != server {
            server = tool.server.clone();
            println!("{server}");
        }
        println!("  {:<28} {}", tool.name, util::truncate(tool.description.lines().next().unwrap_or(""), 80));
    }
}

/// `echospot mcp call <server__tool> '<json>'`: one tool call, for trying a server out.
pub fn cli_call(wire: &str, arguments: &str) {
    tools();
    match call(wire, &serde_json::from_str(arguments).unwrap_or(json!({}))) {
        Ok(text) => println!("{text}"),
        Err(e) => eprintln!("error: {e}"),
    }
}
