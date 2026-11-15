// Launches every command of an installed extension in the host and reports what each one did:
// the view it reached, the host calls it made, and any error. Nothing is pressed or submitted.
//
//   node smoke.mjs <extension dir> [command …]        (--wait <seconds>, default 8)

import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const waitAt = args.indexOf("--wait");
const wait = waitAt >= 0 ? Number(args.splice(waitAt, 2)[1]) * 1000 : 8000;
const [extensionDir, ...only] = args;
if (!extensionDir) {
  console.error("usage: node smoke.mjs <extension dir> [command …] [--wait seconds]");
  process.exit(2);
}
const host = path.join(path.dirname(fileURLToPath(import.meta.url)), "dist/host.cjs");
const manifest = JSON.parse(fs.readFileSync(path.join(extensionDir, "package.json"), "utf8"));
const commands = (manifest.commands || []).filter((c) => !only.length || only.includes(c.name));

function runCommand(command) {
  return new Promise((resolve) => {
    const launch = { appearance: "dark", frontmost: null, arguments: {}, ai: false, launchType: "userInitiated" };
    const child = spawn("node", [host, extensionDir, command.name, JSON.stringify(launch)], { env: { ...process.env, NODE_NO_WARNINGS: "1" } });
    const seen = { errors: [], calls: new Set(), tree: null, exited: false };
    let buffered = "";
    child.stdout.on("data", (chunk) => {
      buffered += chunk;
      let newline;
      while ((newline = buffered.indexOf("\n")) >= 0) {
        const line = buffered.slice(0, newline);
        buffered = buffered.slice(newline + 1);
        if (!line.startsWith("\x1e")) continue;
        const message = JSON.parse(line.slice(1));
        if (message.t === "tree") seen.tree = message.tree;
        else if (message.t === "error") seen.errors.push(message.text);
        else if (message.t === "log" && message.level === "error") seen.errors.push(`console.error: ${message.text}`);
        else if (message.t === "exit") seen.exited = true;
        else if (message.t === "call") {
          seen.calls.add(message.method);
          // No one is here to sign in or confirm; say so rather than leave the command waiting.
          if (message.id) child.stdin.write(JSON.stringify({ t: "reply", id: message.id, ok: false, value: `smoke: ${message.method} declined` }) + "\n");
        }
      }
    });
    let stderr = "";
    child.stderr.on("data", (chunk) => (stderr += chunk));
    const timer = setTimeout(() => child.kill(), wait);
    child.on("close", () => {
      clearTimeout(timer);
      if (stderr.trim()) seen.errors.push(`stderr: ${stderr.trim()}`);
      resolve(seen);
    });
  });
}

function describe(tree) {
  const root = tree?.children?.[0];
  if (!root) return "nothing rendered";
  let nodes = 0;
  const types = new Set();
  (function walk(n) {
    nodes++;
    if (n.type) types.add(n.type);
    for (const child of n.children || []) walk(child);
    for (const value of Object.values(n.props || {})) if (value && typeof value === "object" && value.type) walk(value);
  })(root);
  const loading = root.props?.isLoading ? ", still loading" : "";
  return `${root.type} (${nodes} nodes${loading}) [${[...types].filter((t) => t !== root.type).slice(0, 6).join(", ")}]`;
}

let failed = 0;
for (const command of commands) {
  const seen = await runCommand(command);
  const bad = seen.errors.length > 0;
  if (bad) failed++;
  const calls = seen.calls.size ? ` calls: ${[...seen.calls].join(", ")}` : "";
  console.log(`${bad ? "FAIL" : "ok  "} ${command.name} (${command.mode}): ${command.mode === "no-view" ? (seen.exited ? "finished" : "still running") : describe(seen.tree)}${calls}`);
  for (const error of [...new Set(seen.errors)]) console.log(`       ${error.split("\n").slice(0, 4).join("\n       ")}`);
}
console.log(`\n${commands.length - failed}/${commands.length} commands ran clean`);
process.exit(failed ? 1 : 0);
