//! Docs that cannot drift: every table here is printed from the same constants and
//! declarations the host runs on, for the version that is installed. `echospot dev docs`.

use super::blocks;
use super::error::CODES;
use super::runtime::Runtime;

pub const TOPICS: &[(&str, &str)] = &[
    ("start", "The loop: new, check, render, look, fix, test"),
    ("packs", "What a pack is, its folder, and its life"),
    ("pack.toml", "Identity and permissions"),
    ("abilities", "Typed functions with an address"),
    ("kinds", "read, effect and final"),
    ("types", "Schemas, and what each type brings"),
    ("verbs", "Actions attached to a type, and Send to…"),
    ("ui", "Components, primitives and modifiers"),
    ("design", "Tokens, Looks, intents, effects and the checker"),
    ("icons", "The icon spec and its check"),
    ("stores", "Live typed data with history"),
    ("triggers", "Schedules and events"),
    ("boards", "Dashboards of tiles"),
    ("surfaces", "Panels, windows, overlays, widgets, the bar"),
    ("ai", "The user's model, inside a pack"),
    ("network", "HTTPS to declared hosts, and secrets"),
    ("files", "The data folder, globs and the picker"),
    ("scenes", "Fixture states for rendering and tests"),
    ("tests", "tests/*.luau, mocks and recorded HTTP"),
    ("rust", "The Rust tier: abilities compiled to WebAssembly"),
    ("budgets", "Time and memory limits"),
    ("grants", "Who can grant what"),
    ("echo", "Every function of @echo and cx"),
    ("builtins", "Every built-in ability"),
    ("errors", "Every error code"),
    ("cli", "Every command"),
];

fn table(head: &[&str], rows: Vec<Vec<String>>) -> String {
    let mut out = format!("| {} |\n|{}|\n", head.join(" | "), head.iter().map(|_| "---").collect::<Vec<_>>().join("|"));
    for row in rows {
        out.push_str(&format!("| {} |\n", row.join(" | ")));
    }
    out
}

pub const COMMANDS: &[(&str, &str)] = &[
    ("echospot dev new <id> [--from currency|github|board|overlay|blank]", "Scaffold a pack from a template"),
    ("echospot dev api search <words>", "Find abilities, components, types, verbs"),
    ("echospot dev api show <name>", "One name in full: signature, kind, example"),
    ("echospot dev check [<id>] [--design]", "Compile, check declarations, lint, write type definitions; with --design, render every scene and measure it"),
    ("echospot dev render <surface> [--scene <name> | --inbox <report>] [--out <png>]", "Draw one surface: a PNG and a text tree; --inbox draws a report's state again through the code as it is now"),
    ("echospot dev scenes [<id>]", "Draw every surface in every scene, the host's stress scenes included"),
    ("echospot dev run <address> [--json '{…}'] [--dry]", "Call an ability; --dry lists the effects instead of doing them"),
    ("echospot dev test [<id>] [--record]", "Run tests/*.luau with effects stubbed and HTTP from fixtures"),
    ("echospot dev icon check <svg>", "Check an icon against the spec and write its sheet"),
    ("echospot dev eject <component>", "Copy a component's Luau source into the pack"),
    ("echospot dev logs [<id>] [--follow]", "Prints, warnings, errors and the audit log"),
    ("echospot dev inbox [<id>] [--resolve <report>]", "Point-and-say reports from the human, each with the state to draw it again in"),
    ("echospot dev grants [<id>]", "What the pack has and still needs (read-only)"),
    ("echospot dev docs [topic]", "This documentation"),
    ("echospot dev exemplar <name>", "Render one of EchoSpot's own screens, to compare with"),
    ("echospot dev list", "Every pack and its state"),
    ("echospot call <address> [--json '{…}']", "Call an ability as a person would"),
    ("echospot bar <address> [--json '{…}']", "A line of JSON per update, for Waybar"),
    ("echospot fork <id>", "Copy a system pack into your packs"),
    ("echospot mcp serve", "Every ability as an MCP tool, over stdio"),
    ("echospot agent install", "Write the skill for Claude Code and the AGENTS.md section"),
];

/// One topic as Markdown. `None` when there is no such topic.
pub fn topic(rt: &Runtime, name: &str) -> Option<String> {
    let name = match name {
        "" | "index" => return Some(index()),
        "pack" | "toml" | "manifest" | "permissions" => "pack.toml",
        "ability" => "abilities",
        "kind" => "kinds",
        "type" => "types",
        "verb" | "send" => "verbs",
        "components" | "component" => "ui",
        "looks" | "tokens" | "intents" | "fx" | "checker" => "design",
        "icon" => "icons",
        "store" => "stores",
        "trigger" | "schedule" | "events" => "triggers",
        "board" => "boards",
        "surface" | "overlay" | "widget" | "bar" => "surfaces",
        "net" | "http" | "secrets" => "network",
        "scene" => "scenes",
        "test" => "tests",
        "budget" | "limits" => "budgets",
        "grant" | "security" => "grants",
        "cx" => "echo",
        "error" | "codes" => "errors",
        "commands" => "cli",
        "loop" => "start",
        other => other,
    };
    Some(match name {
        "start" => START.to_string(),
        "packs" => PACKS.to_string(),
        "pack.toml" => format!("{MANIFEST}\nReserved ids, which a pack may not take: {}.\n\nTints: {}.\n", super::manifest::RESERVED.join(", "), super::manifest::TINTS.join(", ")),
        "abilities" => ABILITIES.to_string(),
        "kinds" => KINDS.to_string(),
        "types" => format!("{TYPES}\n{}", table(&["Type", "A value looks like"], super::types::CORE.iter().map(|(n, shape)| vec![format!("`t.{n}()`"), format!("`{shape}`")]).collect())),
        "verbs" => format!("{VERBS}\n{}", table(&["Built-in verb", "On"], super::verbs::BUILTIN.iter().map(|v| vec![v.title.to_string(), v.on.join(", ")]).collect())),
        "ui" => format!(
            "{UI}\n## Components\n\n{}\n## Primitives\n\n{}\n{LAYOUT}\n## Modifiers\n\nProps every component takes.\n\n{}",
            table(&["Component", "For", "Props"], blocks::COMPONENTS.iter().map(|(n, about, props)| vec![format!("`ui.{n}`"), about.to_string(), props.to_string()]).collect()),
            table(&["Primitive", "For"], blocks::PRIMITIVES.iter().map(|(n, about)| vec![format!("`ui.{n}`"), about.to_string()]).collect()),
            table(&["Modifier", "For"], blocks::MODIFIERS.iter().map(|(n, about)| vec![format!("`{n}`"), about.to_string()]).collect()),
        ),
        "design" => format!(
            "{DESIGN}\n## Tokens\n\nSpace: {}.\nRadius: {}.\nType roles: {}.\nColours: {}, and the tints {}.\n\n## Looks\n\n{}\n## Intents\n\n{}\n## Effects\n\n`ui.fx({{ fire = \"heat\" }}, child)`. A surface runs {} effect layers at once; past that the costliest hold still.\n\n{}",
            super::looks::SPACE.iter().map(|(n, v)| format!("`{n}` {v}")).collect::<Vec<_>>().join(", "),
            super::looks::RADIUS.iter().map(|(n, v)| format!("`{n}` {v}")).collect::<Vec<_>>().join(", "),
            super::looks::TYPE.iter().map(|(n, size, ..)| format!("`{n}` {size}")).collect::<Vec<_>>().join(", "),
            super::looks::COLOURS.join(", "), super::manifest::TINTS.join(", "),
            table(&["Look", "What it is", "Effects at once", "Motion"], super::looks::LOOKS.iter().map(|l| vec![format!("`{}`", l.name), l.about.to_string(), l.fx_budget.to_string(), format!("{:.2} s{}", l.motion.duration, if l.motion.bounce > 0.0 { format!(", bounce {:.2}", l.motion.bounce) } else { String::new() })]).collect()),
            table(&["Intent", "glass", "ink", "ember"], blocks::INTENTS.iter().map(|i| {
                let show = |look: &str| {
                    let s = super::looks::get(look).intent(i);
                    let mut parts: Vec<String> = Vec::new();
                    if let Some(t) = s.tint { parts.push(format!("{t:?}").to_lowercase()); }
                    if !s.effect.is_empty() { parts.push(s.effect.to_string()); }
                    if s.quiet { parts.push("quiet".into()); }
                    if parts.is_empty() { "plain".into() } else { parts.join(" + ") }
                };
                vec![format!("`{i}`"), show("glass"), show("ink"), show("ember")]
            }).collect()),
            super::fx::BUDGET,
            table(&["Effect", "Presets"], super::fx::EFFECTS.iter().map(|(n, presets)| vec![format!("`{n}`"), presets.join(", ")]).collect()),
        ),
        "icons" => format!("{ICONS}\nA starting point that passes:\n\n```svg\n{}```\n", super::icon::TEMPLATE),
        "stores" => format!("{STORES}\n{}", table(&["Built-in store", "Reading it needs"], super::stores::BUILTIN.iter().map(|(n, a)| vec![format!("`{n}`"), if a.is_empty() { "nothing".into() } else { format!("`{a}` (or clipboard = \"read\")") }]).collect())),
        "triggers" => format!("{TRIGGERS}\nEvents: {}.\n", super::index::EVENTS.iter().map(|e| format!("`{e}`")).collect::<Vec<_>>().join(", ")),
        "boards" => BOARDS.to_string(),
        "surfaces" => format!("{SURFACES}\nPlaces an overlay or a widget can sit: {}.\n", super::index::PLACES.join(", ")),
        "ai" => AI.to_string(),
        "network" => NETWORK.to_string(),
        "files" => FILES.to_string(),
        "scenes" => SCENES.to_string(),
        "tests" => TESTS.to_string(),
        "rust" => format!(
            "{RUST}\n## Limits\n\nA call runs in a fresh instance with a {} MB heap and about ten seconds of compute ({} units of wasmtime fuel); waiting on an ability it called costs nothing. {}\n",
            super::wasm::HEAP >> 20, super::wasm::FUEL,
            match super::wasm::toolchain() {
                Ok(()) => format!("This machine can build Rust packs: `{}` is installed.", super::wasm::TARGET),
                Err(how) => format!("**This machine can't build Rust packs yet:** `{how}`."),
            },
        ),
        "budgets" => format!(
            "# Budgets\n\n{}",
            table(&["What", "Budget"], vec![
                vec!["A render".into(), format!("warning at {} ms in `dev logs`; stopped at {} ms", super::runtime::RENDER_WARN.as_millis(), super::runtime::RENDER_LIMIT.as_millis())],
                vec!["`query` per keystroke".into(), format!("{} ms per pack, 16 ms in all", super::runtime::QUERY_LIMIT.as_millis())],
                vec!["Any other entry".into(), "250 ms without waiting; `cx:call` and `task.wait` don't count".into()],
                vec!["A pack's heap".into(), "64 MB".into()],
                vec!["Effect layers on a surface".into(), super::fx::BUDGET.to_string()],
                vec!["An HTTP response".into(), "10 MB, 15 s".into()],
                vec!["A Rust pack's call".into(), format!("{} MB heap; about ten seconds of compute, waiting not counted", super::wasm::HEAP >> 20)],
            ])
        ),
        "grants" => GRANTS.to_string(),
        "echo" => format!(
            "# @echo and cx\n\n## echo\n\n{}\n## cx\n\nThe context every pack function is handed.\n\n{}",
            table(&["Name", "Use", "What it does"], super::api::ECHO.iter().map(|(n, sig, about)| vec![format!("`{n}`"), format!("`{sig}`"), about.to_string()]).collect()),
            table(&["Name", "Use", "What it does"], super::api::CX.iter().map(|(n, sig, about)| vec![format!("`{n}`"), format!("`{sig}`"), about.to_string()]).collect()),
        ),
        "builtins" => {
            let mut specs = rt.specs();
            specs.sort_by(|a, b| a.address.cmp(&b.address));
            format!("# Built-in abilities\n\nReserved packs. A free one needs no grant.\n\n{}", table(&["Signature", "Kind", "Needs", "What it does"], specs.iter().map(|s| vec![format!("`{}`", s.signature()), s.kind.name().into(), s.gate.label().into(), s.title.clone()]).collect()))
        }
        "errors" => format!("# Errors\n\nEvery error has a code, where it happened, the nearest valid names and a command to learn more.\n\n{}", table(&["Code", "Meaning"], CODES.iter().map(|(c, m)| vec![format!("`{c}`"), m.to_string()]).collect())),
        "cli" => format!("# Commands\n\nEvery `echospot dev` command takes `--json`.\n\n{}", table(&["Command", "Purpose"], COMMANDS.iter().map(|(c, p)| vec![format!("`{c}`"), p.to_string()]).collect())),
        _ => return None,
    })
}

pub fn index() -> String {
    let mut out = format!("# EchoSpot Packs (API {}, EchoSpot {})\n\nA pack is a folder of Luau under ~/.config/echospot/packs/<id>. `echospot dev docs <topic>`:\n\n", super::API, env!("CARGO_PKG_VERSION"));
    for (name, about) in TOPICS {
        out.push_str(&format!("  {name:<10} {about}\n"));
    }
    out
}

const START: &str = r#"# The loop

1. `echospot dev new <id>` writes a pack that already loads.
2. Edit `init.luau`. EchoSpot reloads it on save; a version that fails leaves the last good one running.
3. `echospot dev check` compiles it, checks every declaration, and lints. Fix what it says.
4. `echospot dev render <ability>` draws it: a PNG and a text tree. **Look at the PNG.**
5. `echospot dev check --design` renders every scene, the stress scenes included, and measures them.
6. `echospot dev run <address> --dry` shows what an effect would do, without doing it.
7. `echospot dev test` runs `tests/*.luau`.

Rules that save time:

- Say what, not how. Return `ui.List { items = … }`; never sizes or colours.
- Never write loading or empty-state code: give a component a Resource from `cx:load` and an `empty`.
- Everything that touches the world is `cx:call(address, input)`. Find addresses with `echospot dev api search`.
- A permission is a line in `pack.toml`, and only a person can grant it, in EchoSpot's window. `echospot dev grants` shows what is missing.
- Read `INTENT.md` before editing a pack, and add to it after.
"#;

const RUST: &str = r#"# Rust packs

For heavy compute or a crate Luau doesn't have. A pack folder with a `Cargo.toml` is a Rust
pack: EchoSpot builds it with cargo for `wasm32-wasip2`, compiles the component to native code
once and keeps that, and runs each call in a fresh instance. Start from the template:

```
echospot dev new primes --from rust
```

```
Cargo.toml       [lib] crate-type = ["cdylib"]; depends on wit-bindgen and serde_json
src/lib.rs       yours: `pub fn pack() -> echo::Pack`
src/echo.rs      EchoSpot's side of the API; rewritten before each build
src/calls.rs     every address this pack may call, as a typed function; rewritten before each build
wit/             the interface; rewritten before each build
init.luau        optional: surfaces and faces, drawn from the Rust abilities' output
tests/*.luau     tests, as for any pack: `t:call("nth", { n = 10 })`
```

```rust
mod calls;
mod echo;
use echo::{Ability, Cx, Pack, Result, Store, t};
use serde_json::{Value, json};

pub fn pack() -> Pack {
    Pack::new("Primes")
        .store("last", Store::new(t::number()))
        .ability("nth", Ability::read("Nth Prime")
            .input([("n", t::number())])
            .output(t::number())
            .examples(["nth prime 1000"])
            .run(nth))
}

fn nth(input: Value, cx: &Cx) -> Result {
    // JSON numbers may arrive as 1000.0: read them as f64.
    let n = input["n"].as_f64().ok_or("n must be a number")?;
    let rate = calls::calc_convert(cx, 5.0, "usd", "eur")?;   // a built-in, typed
    let prs = cx.call("github.prs", json!({}))?;               // another pack: needs calls = ["github.prs"]
    Ok(json!(n))
}
```

The same declarations as Luau, as builders: `Ability::read | effect | final_`, `.input`,
`.output`, `.examples`, `.run`, `.undo`, `.every`, `.at`, `.on`; `Verb::new(type, title)`;
`Store::new(type)`; `Trigger::every | at | on`. Types are `t::text()`, `t::list(t::url())`,
`t::record("pr", [("title", t::text())])`, with `.optional()`, `.default(v)` and `.describe(label)`.

`cx` is the same broker: `cx.call(address, input)`, `cx.try_call`, `cx.store(name)`,
`cx.store_at(name, time)`, `cx.put(name, value)`, `cx.dry()`, `cx.log(text)`. An ability runs
with this pack's grants and kinds hold: a read that calls an effect fails with E0310.

**Headless.** A Rust pack has no UI of its own. Its abilities get the default faces for their
output types; for more, add an `init.luau` with surfaces that `cx:load("<id>.<ability>")`.
A name is declared in one tier or the other, never both.

**Sandboxed.** The component sees no files, no network, no environment and no arguments: `std::fs`
and `std::net` fail. Reach the world with `cx.call`, as any pack does. `println!` goes to
`echospot dev logs`; a panic stops the call (E0208) and its message is logged.

**Building.** `echospot dev check` builds and prints cargo's first error at its line (E0206). In
EchoSpot a build runs on a thread: the row shows its progress and the last good version keeps
answering until the new one loads. Build scripts and proc macros run as you, as `cargo build`
always does; only the component is sandboxed.

**32-bit.** A component is wasm32: `usize` is 32 bits. Do arithmetic that can pass 4 billion in
`u64`.
"#;

const PACKS: &str = r#"# Packs

```
~/.config/echospot/packs/<id>/
  pack.toml        identity, API version, permissions (read without running code)
  init.luau        entry: returns echo.pack { … }
  *.luau           more modules: require("@self/x") from init.luau
  boards/*.luau    one board each: return echo.board { … }
  icons/*.svg      one per ability or surface
  scenes/*.luau    fixture states for render and test
  tests/*.luau     run by `echospot dev test`
  fixtures/http/   recorded responses, for offline tests
  INTENT.md        what the human asked for, and decisions made
  inbox/           point-and-say reports from the human
```

| State | What the user sees |
|---|---|
| `drafting` | pack.toml exists, init.luau doesn't: rows say "Being built" |
| `checking` | compiling and reading declarations |
| `needs-grant` | compiled; a row says "Review permissions" |
| `ready` | normal |
| `broken` | the newest version failed; **the last good one keeps running** |

One VM per pack, sandboxed: no `io`, no `os.execute`, no network, `require` only inside the pack.
The only way out is `cx:call`, and every call goes through the broker: grant check, audit log, undo record.
"#;

const MANIFEST: &str = r#"# pack.toml

```toml
id = "github"                 # lowercase letters, digits, dashes
name = "GitHub"
api = 1
tint = "graphite"

[permissions]
net = ["api.github.com"]                       # hosts only; https only
secrets = { github = ["api.github.com"] }      # secret name → hosts it may be sent to
clipboard = "write"                            # none | write | read
files = ["~/Projects/**"]                      # globs outside the pack's own data folder
run = [{ bin = "/usr/bin/gh", args = ["pr", "list", "--json", "{fields}"] }]
ai = { budget_usd_month = 2.00 }
calls = ["scripts.pi", "calendar.next"]        # other packs' abilities, and built-ins that aren't free
background = true                              # schedules and event triggers
```

Every line under `[permissions]` is shown to the person before any of the pack's code runs.
Ask for the least that works: a pack with no permissions runs at once, with no grant screen.
"#;

const ABILITIES: &str = r#"# Abilities

```luau
local echo = require("@echo")
local t = require("@echo/types")

return echo.pack {
    abilities = {
        convert = echo.ability {
            title = "Convert Currency",
            icon = "icons/convert.svg",
            kind = "read",
            input = { amount = t.number(), from = t.currency(), to = t.currency() },
            output = t.money(),
            examples = { "50 usd to inr", "€20 in yen" },
            run = function(input, cx)
                return cx:call("calc.convert", input)
            end,
        },
    },
}
```

- The address is `<pack id>.<name>`: this one is `currency.convert`.
- `input` is real data: it makes the form, the CLI arguments, the AI tool schema and the types.
- `output` picks the default faces: a row in root search, a panel, a board tile, a bar line.
- `examples` teach root search the shape of what people type; each is checked by `dev check`.
- `run` may wait: `cx:call` and `task.wait` suspend it without blocking a frame.
- `query = function(text, cx)` answers as the user types, in 8 ms; return a value or nil.
- `faces = { panel = function(view, cx) … end, tile = function(out, ui) … end }` overrides a default.
- `undo = function(input, output, cx)` takes an effect back.
- `every = "15m"`, `at = "09:00 weekdays"`, `on = "clipboard.changed"` run it unasked (needs `background = true`).
- `refresh = "30m"` is how long a result stays fresh for surfaces that load it.
- `ai = true` lets the AI call an effect as a tool. Reads always are.
- `hidden = true` keeps it out of root search; `keywords = { … }` helps it be found.
- `hotkey = "super+alt+p"` suggests a global hotkey. Settings → Commands offers it; only a person binds it.
- Changing an ability's input so a caller can't meet it (a field it now needs, a type it no
  longer takes) marks every pack calling it broken (E0303), at the line that calls it.
"#;

const KINDS: &str = r#"# Kinds

| Kind | Meaning | Rule |
|---|---|---|
| `read` | No effect on the world | May only call reads, all the way down. A read that calls an effect fails with E0310, naming both. |
| `effect` | Changes something, and can be undone | Give it `undo`, or call only effects that can be taken back: those with an undo, and built-ins that record their own (clipboard, notes, files) or leave nothing lasting (a notification, a page opened, a window focused or moved). Calling anything else, `run.exec`, `http.request` or another pack's effect with no undo, fails with E0311; so does calling a final. |
| `final` | Can't be undone | EchoSpot asks first, unless the person turned that off for this address. `confirm = false` skips the question. |

Drawing never changes anything: a face or a surface's `render` can only `cx:load`, which is a read.
`echospot dev run <address> --dry` runs an ability with every effect stubbed, and lists them.
"#;

const TYPES: &str = r#"# Types

`local t = require("@echo/types")`. A type is data: `t.text()`, `t.list(t.url())`,
`t.record("pr", { title = t.text(), url = t.url() })`, `t.enum({ "open", "closed" })`.
Modifiers: `:optional()`, `:default(value)`, `:describe("Label")`.

A typed field brings its verbs with it: a `url` inside a record offers Open and Copy wherever
that record is shown, with no code. A named record (`"pr"`) is a type verbs and Send to… match on.
"#;

const VERBS: &str = r#"# Verbs

```luau
verbs = {
    shorten = echo.verb { on = "url", title = "Shorten URL", icon = "icons/short.svg", kind = "read",
        run = function(value, cx) return cx:call("http.get", { url = "https://is.gd/create.php", query = { format = "simple", url = value } }).text end },
},
```

Now every `url` anywhere offers "Shorten URL". `on` is a type name or a record's name.
Order: the type's primary verb, then what was used lately, then the rest. A pack can't put itself first.

**Send to…** (Tab on any result) lists every ability whose input accepts that value's type.
"#;

/// What `row`, `col`, `box` and `glass` take, beside their children.
const LAYOUT: &str = "`row`, `col`, `box` and `glass` take `gap`, `padding`, `px` and `py` (spaces), `radius`, `fill`, `align` (`center`, `end`), `justify` (`between`, `center`, `end`) and `wrap` (a row's children onto new lines). `grow = true` fills the width; `grow = 1` takes a share of what the row leaves, so `grow = 2` beside `grow = 1` is two thirds. `width` and `height` are points, or `width = \"40%\"` of the parent's. `press = function(payload, cx) … end` makes one pressable, with `payload` handed to it. `ui.input { key = \"name\", value = \"…\", placeholder = \"…\", change = function(text, cx) … end, submit = function(text, cx) … end }` is a field people type in; what is typed is kept under its `key` while the surface is drawn again. Every component can be ejected to these: `echospot dev eject <Component>`. Raw `@rune/ui` isn't there for a pack: a pack describes, and `@echo/ui` is how.\n";

const UI: &str = r#"# @echo/ui

`local ui = require("@echo/ui")`. Every function returns a block: plain data naming a component
and what it holds. The host owns spacing, type, motion, focus and effects.

A component takes a Resource (what `cx:load` returns) wherever it takes data: pending shows a
skeleton, an error shows the reason with Retry, ok shows the data. Write no loading code.

```luau
panel = function(view, cx)
    return ui.List {
        items = cx:load("github.prs", {}),
        title = "title", subtitle = "repo",
        intent = function(pr) return pr.failing and "urgent" or nil end,
        empty = { title = "No open pull requests", hint = "Nice." },
        actions = function(pr) return { { title = "Approve", run = function(item, cx) cx:call("github.approve", { url = item.url }) end } } end,
    }
end
```

A row names what it shows by field (`title = "name"`) or by function. An action is
`{ title, icon?, keys?, run = function(item, cx) … end }`; the first answers Return.
"#;

const DESIGN: &str = r#"# Design

Packs say what something means; the Look decides how it shows. Use names, never raw values.
Raw numbers and hex colours still work in primitives, and the checker flags each one.

`echospot dev check --design` renders every scene and reports:

- clipped or overflowing text, with the rectangle
- edges within a point of each other that should be the same
- text a pack coloured itself that falls below WCAG AA contrast
- more than three type sizes on one surface
- raw values where a token belongs
- press targets under 24 points
- effect layers over the Look's budget
- props and names that don't exist

It returns JSON and a PNG with each measured problem boxed.
"#;

const ICONS: &str = r#"# Icons

Every ability, verb, board and pack gets one, in `icons/*.svg`.

- 24 × 24 grid; the drawing stays inside 2–22.
- `stroke="currentColor"`, `stroke-width="1.9"`, round caps and joins, `fill="none"`.
- Fills only for dots 3 units across or less.
- No text, gradients, images, transforms or style attributes. At most 8 shapes.
- Shown on a tile tinted with a palette tint: `tint = "green"` on the ability or in pack.toml.

`echospot dev icon check icons/x.svg` checks the form, weighs the icon against the median of
EchoSpot's own, compares it with every one of them, and writes a sheet (16, 20, 24, 32 and 76
pixels, light and dark, on its tile, among ten neighbours). Look at the sheet.

Draw the idea, not a generic shape. A living icon: `faces = { icon = function(out) return { badge = 3 } end }`,
also `{ progress = 0.4 }` and `{ pulse = true }`.
"#;

const STORES: &str = r#"# Stores

```luau
stores = { builds = echo.store { type = t.list(build), keep = "30d", persist = true, initial = {} } },
```

- `cx:store("builds")` reads it; a surface that reads one is drawn again when it changes.
- `cx:put("builds", value)` writes it. Only the owner writes.
- `cx:storeAt("builds", echo.now() - 86400)` reads the past, for as long as `keep` retains.
- Another pack reads it with `calls = ["ci.stores.builds"]` and `cx:call("ci.stores.builds", {})`.
"#;

const TRIGGERS: &str = r#"# Triggers

```luau
triggers = {
    poll = echo.trigger { every = "15m", run = function(event, cx) cx:put("builds", cx:call("ci.fetch", {})) end },
    morning = echo.trigger { at = "09:00 weekdays", run = function(event, cx) cx:call("system.notify", { title = "Stand-up" }) end },
    clip = echo.trigger { on = "clipboard.changed", run = function(event, cx) … end },
},
```

An ability takes `every`, `at` and `on` too. All of it needs `background = true` under
`[permissions]`, shown in Packs with its schedule and when it last ran.
`on = "file.changed:~/Notes/*.md"` takes a glob; the event carries `path`.
"#;

const BOARDS: &str = r#"# Boards

```luau
-- boards/morning.luau
local echo = require("@echo")
return echo.board {
    title = "Morning",
    look = "ink",
    tiles = {
        { "calendar.next", size = "2x1" },
        { "github.prs", args = { repo = "rune" }, size = "2x2" },
        { "weather.now", size = "1x1", refresh = "30m" },
    },
}
```

Each tile is an ability's `tile` face on a four-column grid; `size` is columns x rows.
Only reads can be tiles. A tile that fails shows its own error; the rest of the board stands.

A tile can take what an earlier tile loaded: `inputs` fills its input fields by tile number.
A text field gets the value as lines, which is what a model reads best:

```luau
{ "calendar.range", size = "2x1" },
{ "github.prs", size = "2x1" },
{ "ai.ask", args = { prompt = "Summarise my day" }, inputs = { system = 1 }, size = "4x1" },
```

A board opens in the palette, in its own window, or on the desktop as a widget: its row in
root search has Open in Window and Show on Desktop.
"#;

const SURFACES: &str = r#"# Surfaces

| Surface | How |
|---|---|
| panel | an ability's `panel` face, or `surfaces = { x = echo.panel { render = … } }` |
| row | an ability's answer in root search: `query`, or its `row` face |
| tile | an ability's `tile` face, on a board |
| overlay | `echo.overlay { place = "top-right", input = "none", size = "220x80", render = … }` |
| widget | `echo.widget { place = "bottom-left", render = … }`, on the desktop |
| window | `echo.window { title = "…", render = … }` |
| bar | `echospot bar <address>` prints one JSON line per update, for Waybar |
| toast, hud | `cx:toast(text, intent)`, `cx:hud(text)` |

Navigation: `cx:push(address, args)`, `cx:pop()`, `cx:replace(address, args)`. Back and Escape are the host's.
A link opens a surface from anywhere: `echospot://github.prs?repo=rune`.
"#;

const AI: &str = r#"# AI

| Call | Use |
|---|---|
| `cx:call("ai.ask", { prompt = … })` | a text answer |
| `cx:call("ai.extract", { text = …, schema = t.record({ … }) })` | data that fits a schema |
| `cx:load("ai.stream", { prompt = … })` into `ui.Stream { source = … }` | text as it is written |
| `cx:call("ai.agent", { goal = …, tools = { "github.prs" } })` | a tool loop over the abilities listed, under the pack's grants |

Needs `ai = { budget_usd_month = 2.00 }`. The user's provider and key are used; the pack never
sees the key. Spend is estimated per call, charged to the pack's month, and shown in Packs.
Every `read` ability is a tool for EchoSpot's own AI and over MCP; an effect only with `ai = true`.
"#;

const NETWORK: &str = r#"# Network and secrets

```luau
local res = cx:call("http.get", { url = "https://api.github.com/user", auth = "github" })
-- res.status, res.headers, res.text, res.json
```

- HTTPS only, to hosts in `net`. A redirect to an undeclared host is refused (E0502).
- 10 MB and 15 seconds, at most.
- `auth = "github"` names a secret. The host adds the header; the pack never sees the value, and
  the host won't send it anywhere but the hosts it is bound to.
- `http.request` (POST, PUT, PATCH, DELETE) is an effect.
"#;

const FILES: &str = r#"# Files

- A relative path is inside the pack's own data folder (`cx.data`), always available.
- Anything else needs a glob in `files`, granted, or a path the person picked:
  `cx:call("files.pick", { title = "Choose a folder", folder = true })`. The choice is the permission.
- `files.read`, `files.write`, `files.list`, `files.remove`. A write and a remove can be undone.
- `run.exec` runs an exact binary with an argument template from `run`; slots are filled as
  separate arguments, so nothing a value holds can become a new argument.
"#;

const SCENES: &str = r#"# Scenes

```luau
-- scenes/failing.luau
return {
    load = { ["github.prs"] = { { title = "Fix CI", failing = true, url = "https://x.dev/1" } } },
    stores = { remaining = 42 },
    errors = { ["weather.now"] = "The server didn't answer" },
    pending = { "ci.builds" },
    args = { repo = "rune" }, query = "fix", dark = true,
}
```

A scene replaces what abilities return and what stores hold. `echospot dev render <surface> --scene failing`.
The host makes more for every surface: nothing, one, a thousand, long titles, missing icons,
slow load, error, light, dark and the narrowest tile. `echospot dev scenes` draws them all.
"#;

const TESTS: &str = r#"# Tests

```luau
-- tests/convert.luau
return {
    ["converts dollars"] = function(t)
        t:mock("calc.convert", { amount = 4170.5, currency = "INR" })
        local out = t:call("currency.convert", { amount = 50, from = "usd", to = "inr" })
        t:equal(out.currency, "INR")
    end,
    ["shows failing ones as urgent"] = function(t)
        local tree = t:tree("prs", "failing")       -- a surface in a scene, as text
        t:match(tree, "Fix CI")
        t:match(tree, "%[urgent%]")
    end,
}
```

`t:call`, `t:try`, `t:mock(address, value)`, `t:fail(address, message)`, `t:store(name, value)`,
`t:render(surface, scene?)`, `t:tree(surface, scene?)`, `t:effects()`, `t:equal`, `t:match`, `t:ok`.

Effects are stubbed and listed (`t:effects()`); HTTP answers from `fixtures/http/`.
`echospot dev test --record` captures real responses once, with the pack's real grants.
"#;

const GRANTS: &str = r#"# Grants

A permission is asked for in `pack.toml` and granted by a person, in EchoSpot's own window:
open EchoSpot, find the pack, choose Review Permissions. Nothing an agent can run grants one:
no command, no socket message, no file a pack can write.

- `echospot dev grants` shows what a pack holds and what it still needs. It can't change either.
- A new version that asks for more keeps running with what it had, and shows the difference.
- A callee always runs with its own grants, never the caller's.
- Everything that isn't a free read goes to the audit log: `echospot dev logs`.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_has_a_page() {
        let rt = Runtime::ephemeral();
        for (name, _) in TOPICS {
            let page = topic(&rt, name).unwrap_or_else(|| panic!("no page for {name}"));
            assert!(page.len() > 120, "{name} is nearly empty");
        }
        assert!(topic(&rt, "nope").is_none());
        // Tables come from the host's own lists, so they can't fall behind.
        assert!(topic(&rt, "builtins").unwrap().contains("calc.convert(amount: number, from: text, to: text)"));
        assert!(topic(&rt, "errors").unwrap().contains("E0310"));
        assert!(topic(&rt, "ui").unwrap().contains("ui.Timeline"));
        assert!(topic(&rt, "design").unwrap().contains("fire"));
        assert!(topic(&rt, "component").is_some());
    }
}
