// EchoSpot's extension host. One Node process runs one Raycast command:
//
//   node host.cjs <extension dir> <command name> <launch json>
//
// It renders the command's React tree into plain JSON and speaks to EchoSpot in lines over
// stdio. Lines EchoSpot should read start with a record separator, so anything an extension
// prints on its own never reads as protocol.

import fs from "node:fs";
import path from "node:path";
import Module from "node:module";
import React from "react";
import * as jsxRuntime from "react/jsx-runtime";
import { Surface } from "./reconciler.js";
import { createApi } from "./api.js";

const [extensionDir, commandName, launchJson] = process.argv.slice(2);
const launch = JSON.parse(launchJson || "{}");

const MARK = "\x1e";
const realWrite = process.stdout.write.bind(process.stdout);
export function send(message) {
  realWrite(MARK + JSON.stringify(message) + "\n");
}

// An extension's own output is its log.
function logLine(level, args) {
  const text = args.map((a) => (typeof a === "string" ? a : a instanceof Error ? a.stack || a.message : safeJson(a))).join(" ");
  send({ t: "log", level, text });
}
function safeJson(value) {
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}
for (const level of ["log", "info", "warn", "error", "debug"]) {
  console[level] = (...args) => logLine(level, args);
}
process.stdout.write = (chunk, encoding, cb) => {
  logLine("log", [String(chunk).replace(/\n$/, "")]);
  if (typeof encoding === "function") encoding();
  else if (typeof cb === "function") cb();
  return true;
};

function fail(error) {
  send({ t: "error", text: error instanceof Error ? error.stack || error.message : String(error) });
}
process.on("uncaughtException", fail);
process.on("unhandledRejection", fail);

// Host calls EchoSpot answers later: confirmAlert, and anything whose outcome matters.
let nextCall = 1;
const pending = new Map();
export function call(method, args) {
  return new Promise((resolve, reject) => {
    const id = nextCall++;
    pending.set(id, { resolve, reject });
    send({ t: "call", id, method, args });
  });
}
export function notify(method, args) {
  send({ t: "call", id: 0, method, args });
}

const manifest = JSON.parse(fs.readFileSync(path.join(extensionDir, "package.json"), "utf8"));
const command = (manifest.commands || []).find((c) => c.name === commandName);
if (!command) {
  fail(new Error(`Command "${commandName}" is not in ${manifest.name}'s manifest.`));
  process.exit(1);
}

// One mounted view per navigation level; only the top renders to EchoSpot.
const stack = [];
function top() {
  return stack[stack.length - 1];
}
let flushQueued = false;
function emit() {
  if (flushQueued) return;
  flushQueued = true;
  setImmediate(() => {
    flushQueued = false;
    const view = top();
    if (view) send({ t: "tree", depth: stack.length, tree: view.tree || { children: [] } });
  });
}
function pushView(element) {
  const view = { tree: null, surface: null };
  view.surface = new Surface((tree) => {
    view.tree = tree;
    if (top() === view) emit();
  }, fail, emit);
  stack.push(view);
  view.surface.render(element);
}
function popView() {
  if (stack.length <= 1) return false;
  const view = stack.pop();
  view.surface.unmount();
  emit();
  return true;
}

const api = createApi({
  extensionDir, manifest, command, launch, call, notify, send,
  navigation: {
    push: (element) => pushView(React.createElement(NavigationRoot, null, element)),
    pop: () => {
      if (!popView()) notify("popToRoot", []);
    },
  },
});

function NavigationRoot({ children }) {
  return children;
}

// The extension's bundle asks for these by name; everything else resolves as usual.
const provided = {
  "@raycast/api": api,
  react: React,
  "react/jsx-runtime": jsxRuntime,
  "react/jsx-dev-runtime": jsxRuntime,
};
const originalLoad = Module._load;
Module._load = function load(request, parent, isMain) {
  if (Object.prototype.hasOwnProperty.call(provided, request)) return provided[request];
  return originalLoad.call(this, request, parent, isMain);
};

process.stdin.setEncoding("utf8");
let buffered = "";
process.stdin.on("data", (chunk) => {
  buffered += chunk;
  let newline;
  while ((newline = buffered.indexOf("\n")) >= 0) {
    const line = buffered.slice(0, newline);
    buffered = buffered.slice(newline + 1);
    if (line.trim()) handle(JSON.parse(line));
  }
});
process.stdin.on("end", () => process.exit(0));

function handle(message) {
  switch (message.t) {
    case "event": {
      const view = top();
      if (view && !view.surface.dispatch(message.handler, message.args || [])) {
        // The tree moved on since the press; the newest render has no such handler.
      }
      break;
    }
    case "reply": {
      const entry = pending.get(message.id);
      if (!entry) break;
      pending.delete(message.id);
      if (message.ok) entry.resolve(message.value);
      else entry.reject(new Error(String(message.value || "Host call failed")));
      break;
    }
    case "pop":
      if (!popView()) send({ t: "exit" });
      break;
    case "quit":
      process.exit(0);
  }
}

async function run() {
  const file = path.join(extensionDir, `${command.name}.js`);
  const loaded = require(file);
  const entry = loaded && loaded.default ? loaded.default : loaded;
  if (typeof entry !== "function") throw new Error(`${command.name}.js exports no command.`);
  const props = { arguments: launch.arguments || {}, fallbackText: launch.fallbackText, launchContext: launch.context, launchType: launch.launchType || "userInitiated" };
  if (command.mode === "view" || command.mode === "menu-bar") {
    pushView(React.createElement(NavigationRoot, null, React.createElement(entry, props)));
    if (launch.launchType === "background") {
      // A scheduled run has no one looking: once the menu has loaded (or after a while), it ends.
      const started = Date.now();
      const done = () => {
        const tree = top()?.tree;
        const root = tree?.children?.[0];
        const loaded = root && !root.props?.isLoading;
        if (loaded || Date.now() - started > 20000) {
          api.__settle().then(() => {
            send({ t: "exit" });
            setTimeout(() => process.exit(0), 50);
          });
        } else {
          setTimeout(done, 250);
        }
      };
      setTimeout(done, 400);
    }
  } else {
    // A no-view command runs to completion and the host goes with it.
    await entry(props);
    await api.__settle();
    send({ t: "exit" });
    setTimeout(() => process.exit(0), 50);
  }
}

run().catch((error) => {
  fail(error);
  send({ t: "exit", failed: true });
  setTimeout(() => process.exit(1), 50);
});
