//! `run.exec`: programs. A pack declares an exact binary and an argument template; a call fills
//! the template's slots. There is no shell and no command string, and a slot's value is always
//! one argument, so nothing a pack was handed can become a new argument or a second command.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Ctx, Done, Gate, Run, Spec, num, text};
use crate::packs::error::{PackError, Result};
use crate::packs::index::Kind;
use crate::packs::manifest::RunTemplate;

const MAX_OUTPUT: usize = 1 << 20;

/// The slot names in a template: `{fields}` in `["pr", "list", "--json", "{fields}"]`.
pub fn slots(template: &RunTemplate) -> Vec<String> {
    let mut out = Vec::new();
    for arg in &template.args {
        let mut rest = arg.as_str();
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else { break };
            let name = &rest[open + 1..open + close];
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !out.iter().any(|n| n == name) {
                out.push(name.to_string());
            }
            rest = &rest[open + close + 1..];
        }
    }
    out
}

/// Fills a template: each template argument stays exactly one argument.
pub fn fill(template: &RunTemplate, values: &Value) -> Result<Vec<String>> {
    let names = slots(template);
    if let Some(extra) = values.as_object().into_iter().flatten().map(|(k, _)| k).find(|k| !names.contains(k)) {
        return Err(PackError::new("E0404", format!("the template for {} has no slot {extra:?}", template.bin)).nearest(extra, names.iter().map(String::as_str)));
    }
    template.args.iter().map(|arg| {
        let mut out = arg.clone();
        for name in &names {
            let slot = format!("{{{name}}}");
            if out.contains(&slot) {
                let value = match values.get(name) {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Null) | None => return Err(PackError::new("E0404", format!("{} needs a value for {{{name}}}", template.bin))),
                    Some(other) => other.to_string(),
                };
                // A slot that is the whole argument may not turn into an option.
                if *arg == slot && value.starts_with('-') && !template.args.iter().any(|a| a == "--") {
                    return Err(PackError::new("E0404", format!("the value for {{{name}}} starts with '-', which {} would read as an option; put \"--\" before the slot in the template", template.bin)));
                }
                if value.contains('\0') {
                    return Err(PackError::new("E0404", format!("the value for {{{name}}} holds a NUL byte")));
                }
                out = out.replace(&slot, &value);
            }
        }
        Ok(out)
    }).collect()
}

fn exec(ctx: &Ctx, input: &Value) -> Run {
    let bin = text(input, "bin").to_string();
    let learn = "echospot dev docs programs";
    // The template names the program and the shape of its arguments; `args` names which one
    // when a pack declared several for one program.
    let candidates: Vec<&RunTemplate> = ctx.perms.run.iter().filter(|t| t.bin == bin).collect();
    if candidates.is_empty() {
        let known: Vec<&str> = ctx.perms.run.iter().map(|t| t.bin.as_str()).collect();
        return Run::err(PackError::new("E0404", format!("{} hasn't declared {bin:?}: add it to run in pack.toml, run = [{{ bin = \"{bin}\", args = [ … ] }}]", if ctx.pack.is_empty() { "the caller" } else { &ctx.pack })).nearest(&bin, known).learn(learn));
    }
    let values = input.get("args").cloned().unwrap_or(json!({}));
    let values = if values.as_array().is_some_and(|a| a.is_empty()) { json!({}) } else { values };
    let mut last = None;
    let argv = candidates.iter().find_map(|t| match fill(t, &values) {
        Ok(argv) => Some(argv),
        Err(e) => {
            last = Some(e);
            None
        }
    });
    let Some(argv) = argv else { return Run::err(last.expect("at least one candidate").learn(learn)) };
    let (stdin, timeout, cwd) = (text(input, "stdin").to_string(), num(input, "timeout").unwrap_or(60.0).clamp(1.0, 600.0), ctx.data_dir.clone());
    Run::Later(Box::new(move |_| {
        let mut command = Command::new(&bin);
        command.args(&argv).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if cwd.is_dir() {
            command.current_dir(&cwd);
        }
        let mut child = command.spawn().map_err(|e| PackError::new("E0510", format!("{bin} didn't start: {e}")))?;
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(stdin.as_bytes());
        }
        // Read both pipes off this thread, so a chatty program can't fill one and stall.
        let drain = |pipe: Option<Box<dyn Read + Send>>| std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(pipe) = pipe {
                let _ = pipe.take(MAX_OUTPUT as u64).read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).to_string()
        });
        let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let deadline = Instant::now() + Duration::from_secs_f64(timeout);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() > deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(PackError::new("E0510", format!("{bin} ran longer than {timeout} s and was stopped")));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(e) => return Err(PackError::new("E0510", format!("{bin}: {e}"))),
            }
        };
        let (stdout, stderr) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
        let parsed = serde_json::from_str::<Value>(stdout.trim()).unwrap_or(Value::Null);
        Done::value(json!({ "code": status.code().unwrap_or(-1), "ok": status.success(), "stdout": stdout, "stderr": stderr, "json": parsed }))
    }))
}

pub fn register(out: &mut Vec<Spec>) {
    out.push(Spec::new("run.exec", Kind::Effect, Gate::Run, "Run a declared program; `args` fills the template's {slots}", "bin:text, args:any?, stdin:text?, timeout:number?", "record", exec));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::manifest::Permissions;

    fn gh() -> RunTemplate {
        RunTemplate { bin: "/usr/bin/gh".into(), args: vec!["pr".into(), "list".into(), "--json".into(), "{fields}".into(), "--repo={repo}".into()] }
    }

    #[test]
    fn a_slot_is_always_one_argument() {
        assert_eq!(slots(&gh()), ["fields", "repo"]);
        let argv = fill(&gh(), &json!({ "fields": "title,url; rm -rf ~", "repo": "a b" })).unwrap();
        assert_eq!(argv, ["pr", "list", "--json", "title,url; rm -rf ~", "--repo=a b"]);
        assert_eq!(fill(&gh(), &json!({ "fields": "x" })).unwrap_err().code, "E0404");
        assert_eq!(fill(&gh(), &json!({ "fields": "x", "repo": "r", "evil": "--exec" })).unwrap_err().code, "E0404");
        // A whole-argument slot can't become an option the template didn't write.
        assert_eq!(fill(&gh(), &json!({ "fields": "--web", "repo": "r" })).unwrap_err().code, "E0404");
    }

    #[test]
    fn only_declared_programs_run() {
        let ctx = Ctx { pack: "p".into(), perms: Permissions { run: vec![RunTemplate { bin: "/bin/echo".into(), args: vec!["--".into(), "{word}".into()] }], ..Default::default() }, ..Default::default() };
        let run = |input: Value| match exec(&ctx, &input) {
            Run::Now(r) => r,
            Run::Later(job) => job(&|_| {}),
        };
        assert_eq!(run(json!({ "bin": "/bin/sh", "args": {} })).unwrap_err().code, "E0404");
        let done = run(json!({ "bin": "/bin/echo", "args": { "word": "$(id) && -n" } })).unwrap();
        assert_eq!((done.value["stdout"].as_str(), done.value["code"].as_i64()), (Some("-- $(id) && -n\n"), Some(0)));
    }
}
