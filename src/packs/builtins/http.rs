//! `http.*`: the network. HTTPS only, to hosts the pack declared and was granted; a response is
//! capped in size and time; a redirect to any other host is refused; a secret is added by name
//! and only for the hosts it is bound to.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Map, Value, json};

use super::{Ctx, Done, Gate, Run, Spec, text};
use crate::packs::error::{PackError, Result};
use crate::packs::index::Kind;
use crate::packs::secrets;
use crate::packs::vm::hash;

pub const MAX_BYTES: u64 = 10 << 20;
pub const TIMEOUT_SECS: u64 = 15;
const MAX_REDIRECTS: usize = 5;

/// The host of an https URL, lowercased; nothing for any other scheme, a userinfo or a port
/// other than 443.
pub fn https_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.contains('@') {
        return None;
    }
    let (host, port) = authority.split_once(':').unwrap_or((authority, "443"));
    (port == "443" && !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')).then(|| host.to_lowercase())
}

fn allowed(ctx: &Ctx, host: &str) -> bool {
    ctx.user || ctx.perms.net.iter().any(|h| h.eq_ignore_ascii_case(host))
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', ""))
}

struct Request {
    method: String,
    url: String,
    headers: Vec<String>,
    body: Option<String>,
    auth: String,
}

fn fixture_path(dir: &Path, req: &Request) -> PathBuf {
    let host = https_host(&req.url).unwrap_or_default();
    dir.join(format!("{}_{host}_{:016x}.json", req.method, hash([req.method.as_bytes(), req.url.as_bytes(), req.body.as_deref().unwrap_or("").as_bytes()])))
}

/// One request, without following redirects: status, headers, body.
fn once(req: &Request, url: &str, secret: Option<&str>) -> Result<(u16, Map<String, Value>, Vec<u8>)> {
    let dir = std::env::temp_dir().join(format!("echospot-http-{}-{:x}", std::process::id(), hash([url.as_bytes(), &std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0).to_le_bytes()])));
    std::fs::create_dir_all(&dir).map_err(|e| PackError::new("E0500", format!("can't make a scratch folder: {e}")))?;
    let (head, body) = (dir.join("head"), dir.join("body"));
    // Everything goes to curl on stdin: a secret is never in an argument list for `ps` to show.
    let mut config = format!("url = {}\nrequest = {}\nsilent\nshow-error\nproto = \"=https\"\nmax-time = {TIMEOUT_SECS}\nmax-filesize = {MAX_BYTES}\ndump-header = {}\noutput = {}\n", quoted(url), quoted(&req.method), quoted(&head.display().to_string()), quoted(&body.display().to_string()));
    config.push_str("user-agent = \"EchoSpot\"\n");
    for header in req.headers.iter().map(String::as_str).chain(secret) {
        config.push_str(&format!("header = {}\n", quoted(header)));
    }
    if let Some(data) = &req.body {
        let file = dir.join("data");
        std::fs::write(&file, data).map_err(|e| PackError::new("E0500", format!("can't stage the request body: {e}")))?;
        config.push_str(&format!("data-binary = {}\n", quoted(&format!("@{}", file.display()))));
    }
    let run = || -> std::io::Result<std::process::Output> {
        let mut child = Command::new("curl").args(["--disable", "--config", "-"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(config.as_bytes())?;
        }
        child.wait_with_output()
    };
    let out = run();
    let read = (std::fs::read_to_string(&head).unwrap_or_default(), std::fs::read(&body).unwrap_or_default());
    let _ = std::fs::remove_dir_all(&dir);
    let out = out.map_err(|e| PackError::new("E0500", format!("curl didn't start: {e}")))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().trim_start_matches("curl: ").to_string();
        return Err(match out.status.code() {
            Some(63) => PackError::new("E0501", format!("the response from {url} is larger than {} MB", MAX_BYTES >> 20)),
            Some(28) => PackError::new("E0500", format!("{url} didn't answer within {TIMEOUT_SECS} s")),
            _ => PackError::new("E0500", if why.is_empty() { format!("the request to {url} failed") } else { why }),
        });
    }
    if read.1.len() as u64 > MAX_BYTES {
        return Err(PackError::new("E0501", format!("the response from {url} is larger than {} MB", MAX_BYTES >> 20)));
    }
    // The last header block is the response's own (a 100 Continue may come first).
    let block = read.0.split("\r\n\r\n").filter(|b| b.starts_with("HTTP/")).last().unwrap_or("");
    let status = block.lines().next().and_then(|l| l.split_whitespace().nth(1)).and_then(|s| s.parse().ok()).unwrap_or(0);
    let headers = block.lines().skip(1).filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), Value::String(v.trim().to_string()))).collect();
    Ok((status, headers, read.1))
}

/// A redirect's target, resolved against where it came from.
fn resolve(from: &str, location: &str) -> String {
    if location.contains("://") {
        return location.to_string();
    }
    let origin_end = from.find("://").map(|i| i + 3).and_then(|s| from[s..].find('/').map(|e| s + e)).unwrap_or(from.len());
    if location.starts_with('/') {
        format!("{}{location}", &from[..origin_end])
    } else {
        let base = from.split(['?', '#']).next().unwrap_or(from);
        format!("{}/{location}", base.rsplit_once('/').map_or(base, |(dir, _)| dir).trim_end_matches('/'))
    }
}

fn send(ctx: &Ctx, req: Request) -> Result<Done> {
    let learn = "echospot dev docs network";
    let Some(first_host) = https_host(&req.url) else {
        return Err(PackError::new("E0402", format!("{:?} isn't an https URL; packs speak HTTPS only, on port 443", req.url)).learn(learn));
    };
    if !allowed(ctx, &first_host) {
        return Err(PackError::new("E0402", format!("{} hasn't declared {first_host}: add it to net in pack.toml, net = [\"{first_host}\"]", if ctx.pack.is_empty() { "the caller" } else { &ctx.pack })).nearest(&first_host, ctx.perms.net.iter().map(String::as_str)).learn(learn));
    }
    if !req.auth.is_empty() && !ctx.user && !ctx.perms.secrets.get(&req.auth).is_some_and(|hosts| hosts.iter().any(|h| h.eq_ignore_ascii_case(&first_host))) {
        return Err(PackError::new("E0405", format!("{} may not send the secret {:?} to {first_host}: declare it, secrets = {{ {} = [\"{first_host}\"] }}", ctx.pack, req.auth, req.auth)).nearest(&req.auth, ctx.perms.secrets.keys().map(String::as_str)).learn(learn));
    }
    // A test answers from what was recorded, and never touches the network.
    let fixture = ctx.fixtures.as_ref().map(|dir| fixture_path(dir, &req));
    if let Some(path) = fixture.as_ref().filter(|_| !ctx.record) {
        let Some(saved) = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else {
            return Err(PackError::new("E0500", format!("no recorded response for {} {}; record one with `echospot dev test --record`", req.method, req.url)).learn("echospot dev docs tests"));
        };
        return Ok(Done { value: saved, host: first_host, ..Default::default() });
    }
    let mut url = req.url.clone();
    let mut host = first_host.clone();
    let mut hops = 0;
    let (status, headers, body) = loop {
        let secret = match req.auth.is_empty() {
            true => None,
            false => {
                // A redirect within the declared hosts carries the secret only where it is bound.
                let bound = ctx.user || ctx.perms.secrets.get(&req.auth).is_some_and(|hosts| hosts.iter().any(|h| h.eq_ignore_ascii_case(&host)));
                match (bound, secrets::header(&req.auth, &host)) {
                    (true, Ok(header)) => Some(header),
                    (true, Err(why)) if host == first_host => return Err(PackError::new("E0405", why).learn(learn)),
                    _ => None,
                }
            }
        };
        let (status, headers, body) = once(&req, &url, secret.as_deref())?;
        let location = headers.get("location").and_then(Value::as_str).filter(|_| (300..400).contains(&status));
        let Some(location) = location else { break (status, headers, body) };
        let next = resolve(&url, location);
        let Some(next_host) = https_host(&next) else {
            return Err(PackError::new("E0502", format!("{url} redirects to {next}, which isn't https")).learn(learn));
        };
        if !allowed(ctx, &next_host) {
            return Err(PackError::new("E0502", format!("{url} redirects to {next_host}, which {} didn't declare", if ctx.pack.is_empty() { "the caller" } else { &ctx.pack })).learn(learn));
        }
        hops += 1;
        if hops > MAX_REDIRECTS {
            return Err(PackError::new("E0500", format!("{} redirects too many times", req.url)));
        }
        url = next;
        host = next_host;
    };
    let body = String::from_utf8_lossy(&body).to_string();
    let parsed = serde_json::from_str::<Value>(&body).unwrap_or(Value::Null);
    let value = json!({ "status": status, "ok": (200..300).contains(&status), "headers": headers, "body": body, "json": parsed });
    if let Some(path) = fixture.filter(|_| ctx.record) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, serde_json::to_string_pretty(&value).unwrap_or_default());
    }
    Ok(Done { value, host: first_host, ..Default::default() })
}

/// `{ q = "x", page = 2 }` as a query string, appended to a URL.
fn with_query(url: &str, query: Option<&Value>) -> String {
    let Some(map) = query.and_then(Value::as_object).filter(|m| !m.is_empty()) else { return url.to_string() };
    let encode = |s: &str| s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        _ => format!("%{b:02X}"),
    }).collect::<String>();
    let pairs: Vec<String> = map.iter().map(|(k, v)| format!("{}={}", encode(k), encode(&v.as_str().map(String::from).unwrap_or_else(|| v.to_string())))).collect();
    format!("{url}{}{}", if url.contains('?') { '&' } else { '?' }, pairs.join("&"))
}

fn request(ctx: &Ctx, input: &Value, method: &str) -> Run {
    let mut headers: Vec<String> = input.get("headers").and_then(Value::as_object).into_iter().flatten()
        .filter(|(k, _)| !k.eq_ignore_ascii_case("authorization") || input.get("auth").is_none())
        .map(|(k, v)| format!("{k}: {}", v.as_str().map(String::from).unwrap_or_else(|| v.to_string()))).collect();
    // A header can't smuggle a second one in.
    headers.retain(|h| !h.contains(['\n', '\r']));
    let body = match (input.get("json").filter(|j| !j.is_null()), input.get("body").and_then(Value::as_str)) {
        (Some(json), _) => {
            headers.push("Content-Type: application/json".into());
            Some(json.to_string())
        }
        (None, Some(body)) => Some(body.to_string()),
        _ => None,
    };
    let req = Request { method: method.to_uppercase(), url: with_query(text(input, "url"), input.get("query")), headers, body, auth: text(input, "auth").to_string() };
    if !matches!(req.method.as_str(), "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE") {
        return Run::err(PackError::new("E0301", format!("{:?} isn't an HTTP method", req.method)));
    }
    let ctx = ctx.clone();
    Run::Later(Box::new(move |_| send(&ctx, req)))
}

pub fn register(out: &mut Vec<Spec>) {
    out.push(Spec::new("http.get", Kind::Read, Gate::Net, "GET an https URL on a declared host; `auth` names a secret", "url:url, query:record?, headers:record?, auth:text?", "record", |ctx, i| request(ctx, i, "GET")));
    out.push(Spec::new("http.request", Kind::Effect, Gate::Net, "Send a request to a declared host: POST, PUT, PATCH or DELETE", "url:url, method:text=POST, query:record?, headers:record?, json:any?, body:text?, auth:text?", "record", |ctx, i| {
        request(ctx, i, text(i, "method"))
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::manifest::Permissions;

    fn ctx(hosts: &[&str]) -> Ctx {
        Ctx { pack: "gh".into(), perms: Permissions { net: hosts.iter().map(|h| h.to_string()).collect(), ..Default::default() }, ..Default::default() }
    }

    fn run(ctx: &Ctx, input: Value) -> Result<Done> {
        match request(ctx, &input, "GET") {
            Run::Now(r) => r,
            Run::Later(job) => job(&|_| {}),
        }
    }

    #[test]
    fn only_https_to_declared_hosts() {
        let c = ctx(&["api.github.com"]);
        assert_eq!(https_host("https://API.github.com/x?y"), Some("api.github.com".into()));
        assert_eq!(https_host("http://api.github.com"), None);
        assert_eq!(https_host("https://api.github.com:8443/"), None);
        assert_eq!(https_host("https://api.github.com@evil.dev/"), None);
        assert_eq!(run(&c, json!({ "url": "http://api.github.com/" })).unwrap_err().code, "E0402");
        let e = run(&c, json!({ "url": "https://api.githab.com/" })).unwrap_err();
        assert_eq!((e.code, e.nearest[0].as_str()), ("E0402", "api.github.com"));
        // A secret the pack didn't declare for this host is refused before anything is sent.
        assert_eq!(run(&c, json!({ "url": "https://api.github.com/", "auth": "github" })).unwrap_err().code, "E0405");
        assert_eq!(resolve("https://a.dev/x/y?z", "/next"), "https://a.dev/next");
        assert_eq!(resolve("https://a.dev/x/y?z", "other"), "https://a.dev/x/other");
        assert_eq!(with_query("https://a.dev/s", Some(&json!({ "q": "is:pr author:@me" }))), "https://a.dev/s?q=is%3Apr%20author%3A%40me");
    }

    #[test]
    fn a_test_answers_from_recorded_responses() {
        let dir = std::env::temp_dir().join(format!("echospot-http-fixtures-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = ctx(&["api.github.com"]);
        c.fixtures = Some(dir.clone());
        let input = json!({ "url": "https://api.github.com/user" });
        assert_eq!(run(&c, input.clone()).unwrap_err().code, "E0500");
        let req = Request { method: "GET".into(), url: "https://api.github.com/user".into(), headers: vec![], body: None, auth: String::new() };
        std::fs::write(fixture_path(&dir, &req), r#"{"status":200,"ok":true,"json":{"login":"me"}}"#).unwrap();
        let done = run(&c, input).unwrap();
        assert_eq!((done.value["json"]["login"].as_str(), done.host.as_str()), (Some("me"), "api.github.com"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
