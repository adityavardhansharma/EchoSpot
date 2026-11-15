# EchoSpot Packs: API Design

Status: design proposal. Nothing here is built yet. Written so that someone who was not in the
discussion can build it.

---

## 1. The idea on one page

EchoSpot becomes a small operating system for personal software, and coding agents are its
developers.

A person tells Claude Code, Codex or any other coding agent "make me a command that…". The agent
writes a **pack** (a folder of Luau code, icons and test scenes) using a documented, sandboxed
API. EchoSpot loads it in milliseconds. The result looks as if it shipped with EchoSpot, because
the design system makes it hard to build anything that doesn't.

A pack can be any mix of:

- **Abilities:** typed functions that anything can call: other packs, the palette, the CLI, EchoSpot's AI, outside agents.
- **Surfaces:** where pixels go: palette panel, root-search row, window, screen overlay, desktop widget, bar item, board tile, toast.
- **Triggers:** when code runs: typing, hotkey, schedule, clipboard change, window focus, file change, CLI, AI tool call.
- **Stores:** live, typed, versioned data that surfaces react to.

What makes it different from Raycast and every other launcher API:

1. **Everything is callable from everything.** Your Pi script, a snippet, a built-in calculator function and a pack's ability are all addresses with typed signatures.
2. **The agent is the main user of the API.** The API is self-describing, type-checked before it runs, renders itself to PNG for review, and explains its own errors.
3. **Beauty is the default.** Agents state *what* (items, intents, data); the host decides *how* (spacing, type, motion, effects).
4. **Safety is enforced at one point.** All contact with the world passes through one broker, so permissions, audit, undo, dry runs and trust labels all come from the same place.
5. **Nothing is sealed.** Built-in features are written on the same API and can be forked.

This API owes nothing to Raycast. The Raycast host stays as a frozen compatibility layer (§21).

---

## 2. Principles

These decide arguments later.

1. **Describe, don't draw.** A pack returns data and intent. The host owns pixels, the GPU, focus and the window.
2. **One broker.** Every side effect is a call to an ability, checked and logged in one place. There is no second path.
3. **Typed at the edges.** Every ability has an input schema and an output type. Types drive faces, verbs, piping and AI tool definitions.
4. **Instant or visibly working.** Nothing is ever blank: a loader, skeleton or previous version is always shown.
5. **Never broken in front of the user.** A pack that fails to compile keeps its last good version running.
6. **The agent can see what it built.** Every surface renders headless to PNG plus a text tree.
7. **Grants come from the human, in EchoSpot's own window.** Nothing an agent can type grants a permission.
8. **Local first.** Packs live on the machine as folders. No store, no registry.

---

## 3. Vocabulary

| Term | Meaning |
|---|---|
| **Pack** | A folder: the unit of authorship, permission and loading. |
| **Ability** | A typed function with an address, e.g. `github.prs`, `calc.convert`, `scripts.pi`. |
| **Address** | `<pack>.<ability>`. Built-ins live in reserved packs (`calc`, `clipboard`, `ai`…). |
| **Kind** | Whether an ability is `read`, `effect` (reversible) or `final` (irreversible). |
| **Type** | What a value is: `text`, `money`, `file`, `url`, `event`, `window`… |
| **Verb** | An action attached to a type, available wherever a value of that type appears. |
| **Surface** | A place pixels go. |
| **Face** | How one ability's output looks on one surface (`row`, `panel`, `tile`, `line`, `icon`, `json`). |
| **Trigger** | What makes code run. |
| **Store** | Live typed data with history. |
| **Board** | A layout of tiles: a dashboard. |
| **Look** | A named art direction: tokens, materials and an effect vocabulary. |
| **Intent** | A meaning for UI (`urgent`, `live`, `success`) that the Look turns into visuals. |
| **Scene** | A named fixture state used to render and test a surface. |
| **Grant** | A permission the human gave a pack. |

---

## 4. Architecture

```
 coding agent ──► echospot dev …  (CLI over the IPC socket, --json everywhere)
                        │
┌───────────────────────▼──────────────────────────────────────────────┐
│ EchoSpot host (Rust, Rune)                                           │
│                                                                      │
│  Pack loader ── watches folders, compiles Luau, keeps last good VM   │
│  Registry    ── addresses, types, verbs, faces, triggers, icons      │
│  Broker      ── every ability call: grant check → audit → run        │
│                 → undo record. Dry-run mode stubs effects.           │
│  Stores      ── typed reactive data, history, persistence            │
│  Surfaces    ── palette, root rows, windows, overlays (layer-shell), │
│                 widgets, bar feed, boards, toasts                    │
│  Design      ── tokens, Looks, components, fx budget, checker        │
│                                                                      │
│  ┌────────── one rune-luau VM per pack ───────────┐                  │
│  │ sandboxed globals, 64 MB heap, watchdog,       │                  │
│  │ hot reload, @echo, @echo/ui, @echo/types       │                  │
│  └────────────────────────────────────────────────┘                  │
└──────────────────────────────────────────────────────────────────────┘
```

**Runtime choice.** Luau via `rune-luau` is the primary runtime (Appendix A lists what it gives
us). Rust compiled to WASM components is a later tier for heavy compute and crates (§20). Plain
scripts in any language stay supported as text-in, text-out abilities (§6.7).

**One VM per pack**, not per command. Surfaces of one pack share state. An idle VM costs nothing
(`rune-luau` never enters it when no task is due).

**Threading.** Luau runs on the UI thread. Anything slow (network, files, processes, AI) is a host
async function that runs on a worker; the Luau task waits without blocking frames.

---

## 5. Packs

### 5.1 Layout

```
~/.config/echospot/packs/<id>/
  pack.toml          identity, API version, permissions (read without running code)
  init.luau          entry module: returns echo.pack { … }
  *.luau             more modules (require("@self/x"))
  icons/*.svg        one per ability or surface (§14)
  scenes/*.luau      fixture states for render and test (§17.5)
  tests/*.luau       tests run by `echospot dev test`
  fixtures/http/     recorded responses for offline tests
  INTENT.md          what the human asked for, and decisions made (§17.7)
  inbox/             point-and-say reports from the human (§17.8)
  .luaurc            aliases, so luau-lsp and the host agree
```

The source stays here, editable. Sharing is copying the folder.

### 5.2 `pack.toml`

```toml
id = "github"                 # lowercase, unique; reserved ids are refused (§6.6)
name = "GitHub"
api = 1                       # host supports api N and N-1
tint = "graphite"

[permissions]
net = ["api.github.com"]                       # hosts only; https only
secrets = { github = ["api.github.com"] }      # secret name → hosts it may be sent to
clipboard = "write"                            # none | write | read
files = ["~/Projects/**"]                      # path globs; picker grants are separate (§15.5)
run = [{ bin = "/usr/bin/gh", args = ["pr", "list", "--json", "{fields}"] }]
ai = { budget_usd_month = 2.00 }
calls = ["scripts.pi", "calendar.next"]        # abilities of other packs this pack uses
background = true                              # schedules and event triggers
```

Permissions live in TOML so the host can show them **before any pack code runs**.

### 5.3 Lifecycle and loaders

| State | What the user sees |
|---|---|
| `drafting` | `pack.toml` exists, entry missing or not compiled. Rows appear with a spinner and "Being built". |
| `checking` | Compiling and type-checking. Spinner with the stage name. |
| `needs-grant` | Compiled; asks for permissions. Row shows "Review permissions". |
| `ready` | Normal. |
| `broken` | New version failed. **The last good version keeps running**; a small badge offers "Copy error for agent". |

Compilation of Luau takes milliseconds, so most loaders flash by. The WASM tier (§20) shows real
progress from cargo's JSON output.

Hot reload uses `rune-luau`'s `saveHotState` / `loadHotState`, so a pack keeps its state while
the agent edits it.

### 5.4 Index without running

On load, the host runs `init.luau` once, collects declarations (abilities, schemas, faces,
triggers) and caches the index keyed by file hashes. The launcher's search reads the cache, so
packs never slow search.

---

## 6. Abilities

### 6.1 Declaring one

```luau
--!strict
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

- `run` runs as a `rune-luau` task, so `cx:call` can wait.
- The schema is real data. It produces the form, the CLI arguments, the JSON schema for AI tools and the type definitions for the agent.
- `examples` feed root-search parsing (§9.2) and are tested automatically.

### 6.2 Kinds

| Kind | Meaning | Rule |
|---|---|---|
| `read` | No effect on the world. | May only call `read` abilities. Enforced at runtime; violations are errors. |
| `effect` | Changes something and can be undone. | Must provide `undo` or call only effects that have one. |
| `final` | Cannot be undone (sends a message, deletes permanently). | Host shows a confirm step unless the user turned it off for that ability. |

The host checks kinds as calls happen. A `read` ability that calls an effect fails with an error
naming both, which gives agents a strong guarantee to build on.

### 6.3 Calling

```luau
local prs = cx:call("github.prs", { repo = "rune" })      -- waits; typed result
local ok, err = cx:try("scripts.pi", { digits = 50 })     -- no throw
```

Each call goes through the broker: grant check → audit entry → run → undo record. The callee
always runs **with its own grants**, never the caller's. The caller needs `calls = [...]` for
any address that is not a free built-in (§6.6).

### 6.4 Composition

Because everything has an address and a type:

- **In code:** packs call each other (`cx:call`).
- **In the palette:** select any result, press Tab → "Send to…", and see every ability whose input accepts that type. Results pipe into abilities.
- **For agents:** `echospot dev api search "pi digits"` finds `scripts.pi` with its signature.

The host keeps a dependency graph. Changing or deleting an ability shows what depends on it, and
a signature change that breaks a caller marks the caller `broken` with a clear error.

### 6.5 Faces

An ability's output type picks default faces; a pack can override any of them.

```luau
faces = {
    tile = function(out, ui) return ui.Stat { value = out, caption = "USD → INR" } end,
},
```

See §8.2 for defaults.

### 6.6 Built-in abilities (v1 catalogue)

Built-ins are exposed as reserved packs. "Free" ones need no grant.

| Address | Kind | Free |
|---|---|---|
| `calc.eval`, `calc.convert`, `calc.date` | read | yes |
| `clipboard.history`, `clipboard.read` | read | no |
| `clipboard.copy`, `clipboard.paste` | effect | write: yes |
| `notes.list`, `notes.read` | read | no |
| `notes.create`, `notes.append` | effect | no |
| `windows.list`, `windows.focus`, `windows.move` | read / effect | no |
| `files.search` (EchoFiles index) | read | no |
| `calendar.next`, `calendar.range` | read | no |
| `ai.ask`, `ai.extract`, `ai.stream`, `ai.agent` | read | budget |
| `system.notify`, `system.open_url` | effect | yes |
| `snippets.expand`, `quicklinks.open` | read / effect | yes |
| `http.get`, `http.request` | read / effect | declared hosts |
| `run.exec` | effect | declared binaries |

Reserved ids: `echo`, `calc`, `clipboard`, `notes`, `windows`, `files`, `calendar`, `ai`,
`system`, `snippets`, `quicklinks`, `http`, `run`, `scripts`, `raycast`, `apps`, `emoji`.

### 6.7 Wrapping what already exists

Nothing the user already has is left out:

| Existing thing | Becomes | Signature |
|---|---|---|
| Script command (`@raycast.*` headers) | `scripts.<name>` | args → text (or JSON if it prints JSON) |
| `commands.json` entry | `scripts.<name>` | → text |
| Snippet | `snippets.<keyword>` | → text |
| Quicklink | `quicklinks.<name>` | query → url |
| AI preset | `ai.preset.<name>` | text → text |
| Raycast extension command | `raycast.<ext>.<cmd>` | opaque: run only, no types |

Scripts run as the user, as today. Calling one from a pack needs `calls = ["scripts.x"]`.

---

## 7. Types and verbs

### 7.1 Core types

`text`, `markdown`, `code`, `number`, `quantity` (value + unit), `money`, `currency`, `percent`,
`date`, `time`, `datetime`, `duration`, `url`, `file`, `folder`, `image`, `color`, `app`,
`window`, `event`, `person`, `place`, `table`, `series` (time series), `list<T>`, `record{…}`,
`enum`.

A pack may define record types: `t.record("pr", { title = t.text(), url = t.url(), … })`.
Fields that are themselves typed (the `url` above) bring their verbs along.

### 7.2 Verbs

Verbs are actions attached to a type:

```luau
verbs = {
    shorten = echo.verb { on = "url", title = "Shorten URL", icon = "icons/short.svg", run = … },
},
```

Every `url` anywhere (clipboard history, a GitHub PR, a note) now offers "Shorten URL". Built-in
verbs: copy, paste, open, reveal, share-to-ability, pin, quick-look.

Verb ordering: the type's primary verb first, then recently used, then the rest. Packs cannot
force themselves to the top.

---

## 8. Surfaces and faces

### 8.1 Surfaces

| Surface | Notes |
|---|---|
| `panel` | The palette's main view. Push and pop navigation. |
| `row` | Inline answer in root search, as you type (§9.2). |
| `window` | Its own Rune window: mini-apps, editors. |
| `overlay` | Layer-shell surface: corner, edge or full screen, optionally click-through. Rune already supports this (`rune::overlay`). |
| `widget` | Pinned to the desktop layer. |
| `bar` | A stream for Waybar or the Omarchy shell: `echospot bar <address>` prints JSON lines. |
| `tile` | A cell on a board. |
| `toast`, `hud` | Short-lived messages. |
| `icon` | The ability's own icon, which can be live (§14.4). |

### 8.2 Default faces from output types

| Output type | `row` | `panel` | `tile` | `line` |
|---|---|---|---|---|
| `money`, `quantity`, `number` | big value, verb hints | Stat with history | Stat | value |
| `list<record>` | top match | List | compact list (top 5) | count |
| `series` | sparkline | Chart | Chart | last value + trend |
| `markdown`, `text` | first line | Detail | clipped text | first line |
| `event` | title + time | Detail | countdown | "in 12m" |
| `image` | thumbnail | Gallery | cover | none |
| `table` | none | Table | top rows | none |

Getting these defaults right matters more than any custom face. Start with five types done very
well, then grow.

### 8.3 Resources: loading states for free

Rendering cannot wait, so surfaces read data through resources:

```luau
render = function(view, cx)
    local prs = cx:load("github.prs", { repo = "rune" })   -- Resource: pending | ok | error
    return ui.List { items = prs, empty = "No open PRs" }
end
```

`ui.List` accepts a resource directly: pending shows skeleton rows, error shows an error state
with Retry, ok shows the items. The agent writes no loading code at all. Results are cached per
address and arguments; stores invalidate them (§10).

### 8.4 Navigation

`cx:push(surface, args)`, `cx:pop()`, `cx:replace(...)`. Back, Escape and history are the host's.
Deep links: `echospot://github.prs?repo=rune` opens a surface from anywhere.

---

## 9. Triggers

### 9.1 List

| Trigger | Declared as |
|---|---|
| Invoke (select a row) | default for every ability with a panel face |
| Query (as you type) | `query = function(text, cx) … end` |
| Hotkey | user-assigned; packs can suggest |
| Schedule | `every = "15m"`, `at = "09:00 weekdays"` |
| Events | `on = "clipboard.changed" \| "window.focused" \| "workspace.changed" \| "file.changed:<glob>" \| "calendar.soon" \| "battery.low" \| "network.changed"` |
| CLI | `echospot call <address> --json '{…}'` |
| AI tool call | any `read` ability, automatically; effects only if marked `ai = true` |
| Another pack | `cx:call` |

Background triggers need `background = true` and show in Settings with their schedule and last
run.

### 9.2 Root-search answers

`query` runs on each keystroke with a **budget of 8 ms per pack and 16 ms total**. It may return
a quick answer, then upgrade it asynchronously (a spinner on that row). Rules so results don't
jump: rows keep their position once shown; late rows enter below shown rows.

Natural input like "50 bucks to rupees" is matched against `examples` with a local parser first.
AI slot-filling is opt-in per ability, because it costs time and money.

---

## 10. Stores

```luau
stores = {
    builds = echo.store { type = t.list(buildRecord), keep = "30d", persist = true },
},
```

- **Reactive:** a surface that reads a store re-renders when it changes (built on rune-luau signals and memo versions; no polling).
- **History:** `keep` retains versions. `store:at(time)` reads the past, which lets a board scrub back to yesterday.
- **Built-in stores:** `clipboard`, `windows`, `calendar`, `notes`, `battery`, `network`, `now` (clock).
- Other packs read a store with `calls = ["github.stores.builds"]`. Only the owner writes.

---

## 11. Boards (dashboards)

A board is a file in any pack:

```luau
-- boards/morning.luau
return echo.board {
    title = "Morning",
    look = "ink",
    tiles = {
        { "calendar.next", size = "2x1" },
        { "github.prs", args = { repo = "rune" }, size = "2x2" },
        { "weather.now", size = "1x1", refresh = "30m" },
        { "scripts.pi", args = { digits = 8 }, size = "1x1" },
    },
}
```

- Each tile uses the ability's `tile` face.
- Store-backed tiles update live; others refresh on their schedule.
- A board can show as a palette route, its own window or a desktop widget.
- The layout grid is the host's: tiles snap to a 4-column grid, so boards always align.
- Later: tiles that take other tiles' outputs as input ("summarise my day" reads calendar and PRs).

---

## 12. AI inside packs

| Call | Use |
|---|---|
| `cx:call("ai.ask", { prompt })` | text answer |
| `cx:call("ai.extract", { text, schema })` | structured data matching a schema |
| `ai.stream` | text that flows live into a `ui.Stream` block |
| `cx:call("ai.agent", { goal, tools = { "github.prs", "calendar.next" } })` | a tool loop using only listed abilities, under the pack's grants |

- The user's configured provider and keys are used. The pack never sees keys.
- Each pack has a monthly budget. Spend per pack is shown in Settings.
- Prompts and responses go into the audit log.
- Every pack ability is automatically available as a tool to EchoSpot's own AI chat and over MCP (§17.9).

---

## 13. Design system

The goal: every surface, by any agent, looks like it belongs to EchoSpot. Agent UI usually fails
the same way: arbitrary sizes and colours, missing empty and error states, no motion and weak
hierarchy. This system makes those hard to express.

### 13.1 Tokens

Packs use names, never raw values:

- **Space:** `xxs xs sm md lg xl xxl` (EchoSpot's `theme::space`).
- **Radius:** `row card panel control thumb`.
- **Type roles:** `display title headline body caption mono tabular`.
- **Colour:** `text text-secondary text-tertiary accent positive warning danger card separator`, plus tints `violet indigo blue cyan teal green yellow orange red pink graphite`.

`rune-luau` already resolves colour names through `ScriptOptions::palette`, weight names through
`ScriptOptions::text` and elevation names through `ScriptOptions::shadows`; EchoSpot fills those
with its own theme. Raw numbers and hex values still work as an escape hatch, but the checker
flags each one.

### 13.2 Looks

A Look is an art direction designed once, in Rust, by a human: tokens, material (glass, flat,
ink), motion curves and an effect vocabulary. Initial set:

- `glass`: the current macOS-style liquid glass (EchoSpot default).
- `ink`: flat black, off-white and orange (the brand).
- One expressive Look with stronger effects, for boards and overlays.

Packs pick a Look or inherit the user's. Agents never invent one.

### 13.3 Intents and effects

Packs say what something *means*; the Look decides how it looks:

| Intent | Example rendering (glass Look) |
|---|---|
| `urgent` | fire (`rune-fx` FireParams::heat) along the row edge |
| `live` | soft pulse |
| `loading` | shimmer |
| `success` | sparkles, once |
| `new` | glow that fades |
| `muted` | lower contrast, no effect |

```luau
ui.List { items = prs, intent = function(pr) return pr.failing and "urgent" or nil end }
```

Raw effects are available too: `ui.fx { fire = "heat" }`. `rune-fx` exposes a catalogue of
parameters with ranges and presets (`Param`, `ParamKind`, `Preset`) and a cost per effect
(`Fx::cost`), so the host validates parameters and enforces an **effect budget** per surface
(default: three running effect layers). Over budget, the lowest-priority effects freeze to a
still frame instead of running.

### 13.4 Components

Layered, all in Luau, all public:

1. **Primitives** (`rune-luau` built-ins plus host nodes): `row col box text spacer each virtualList`, and added by EchoSpot: `icon image input fx glass markdown code chart`.
2. **Components** (`@echo/ui`, written only with primitives): `List Grid Detail Form Stat Chart Table Timeline Gallery KeyValue Progress Banner Empty Tabs Split Stream Board`.
3. **Modifiers** on every component: `look intent density accessory fx empty error`.

**Eject.** `echospot dev eject List` copies the List component's Luau source into the pack.
The agent then changes anything: custom rows, odd selection, a fire list. This works because
components use only public primitives; nothing is hidden.

### 13.5 Built-in states and motion

- Every component has designed empty, loading and error states.
- Insert, remove, reorder and value changes animate with the Look's curves (`rune-motion`).
- Focus rings, hover, press and keyboard navigation come from the component, never the pack.

### 13.6 Stress scenes

For every surface the host generates scenes automatically: zero items, one, a thousand, very long
titles, missing icons, slow load, error, light, dark and the narrowest tile size. The agent
doesn't have to think of them.

### 13.7 The checker

`echospot dev check --design` renders every scene headless (EchoSpot's `--snap` and `--tree`
modes are the base) and measures the result, because the layout knows every rectangle:

- clipped or overflowing text
- misaligned edges (left edges within 1 px of each other must match)
- contrast below WCAG AA
- more than three type sizes on one surface
- raw values instead of tokens
- tap targets under the minimum size
- effects over budget

It returns a JSON report plus PNGs with each problem boxed.

### 13.8 Exemplars

`echospot dev exemplar clipboard` renders a built-in screen in the same scenes, so the agent can
compare its work with EchoSpot's own.

Honest limit: rules guarantee consistency and a floor, not beauty. The ceiling comes from the
Looks and exemplars humans design.

---

## 14. Icons

Every ability, verb, board and pack gets an icon, drawn by the agent in EchoSpot's style.

### 14.1 Spec (matches `src/icons.rs`)

- 24 × 24 grid; drawing inside the 2–22 safe area.
- Stroke 1.9, round caps, round joins, `currentColor` only.
- Fills only for small dots (≤ 3 units across).
- No text, gradients, images or transforms. At most 8 path elements.
- Readable at 16 px: no gaps under 1.5 units.
- Shown on a tile tinted with a palette tint name (`tint = "green"`), never a raw colour.

### 14.2 `echospot dev icon check icons/convert.svg`

- Validates the spec above.
- Measures visual weight (ink coverage) and compares it to the median of EchoSpot's icons. Too heavy or too light fails.
- Similarity check: rasterises at 24 px and compares with every existing icon. Too similar fails, with the name of the near-duplicate.
- Writes a sheet: the icon at 16, 20, 24, 32 and 76 px, light and dark, on its tile, beside ten existing icons.

### 14.3 Creativity

The spec limits form, not ideas. The agent should draw the concept, not a generic shape. The skill
(§17.6) includes good and bad examples.

### 14.4 Living icons

An `icon` face can return state: `{ badge = 3 }`, `{ progress = 0.4 }`, `{ pulse = true }`.
The host draws these consistently on the tile.

---

## 15. Security model

### 15.1 Threat model

Treat pack code as untrusted, even though it is local:

- An agent can make mistakes.
- An agent can be misled by something it read (prompt injection in a web page or issue).
- Packs may be copied from other people.

Goals: a pack can do only what its grants allow; the human always sees what was granted;
nothing escalates silently.

### 15.2 The sandbox

`rune-luau` gives us:

- sandboxed, frozen globals: Luau has no `io` library, and its `os` only reads time, so there is no file, process or network access unless the host adds it;
- `require` limited to the pack's own folder;
- a 64 MB heap limit;
- a watchdog that stops any entry into the VM after its time limit.

EchoSpot sets a lower watchdog for render entries (50 ms; `ScriptOptions::time_limit` is per
script today, so this needs a per-entry limit in rune-luau, see Appendix C).

The only way out of the VM is the host modules EchoSpot registers, and all of them route through
the broker.

### 15.3 Permissions and grants

- Declared in `pack.toml` (§5.2).
- Granted **only in EchoSpot's window**, by a human click. No CLI, IPC or file write can grant, so an agent cannot grant itself.
- Granted per pack. When an update asks for more, the pack keeps running with its old grants and shows "Review new permissions" with a diff.
- Revocable at any time in Settings.

### 15.4 Network

- HTTPS only, to declared hosts. No raw sockets.
- Response size cap (default 10 MB) and timeout (default 15 s).
- Redirects to undeclared hosts are refused.

### 15.5 Files

- Every pack has its own data folder, always available.
- Other paths need declared globs.
- **Picker grants:** `cx:call("files.pick")` opens EchoSpot's own picker; whatever the user picks is granted to that pack for that path only. The user's choice is the permission.

### 15.6 Running programs

- Exact binary path plus an argument template. No shell, no string commands.
- Template slots are filled as separate arguments, so injection cannot become new arguments.

### 15.7 Secrets

- Stored by the host (keyring, or a 0600 file). Set in Settings.
- Each secret is bound to hosts. A pack uses it by name (`auth = "github"`) and the host adds the header.
- The pack never sees the value, and the host refuses to send it to any other host.

### 15.8 Composition and the confused deputy

- A callee always runs with its own grants.
- A caller cannot pass hosts, paths or binaries for the callee to use beyond the callee's own grants.
- Calls into another pack need `calls = [...]`, shown in the permission screen.

### 15.9 Audit log

Every broker call that is not a free `read` is logged: time, pack, address, a short argument
summary, result size, AI spend. Viewable in Settings and through `echospot dev logs`.

### 15.10 Trust labels

Because the broker sees everything, EchoSpot states facts about a pack instead of trusting its
author: "Works offline", "Talks only to api.github.com", "Never reads your clipboard",
"Spent $0.12 on AI this month".

### 15.11 Known risks

- A bug in Luau or mlua that escapes the VM would compromise EchoSpot. Mitigation: keep them updated. If packs ever come from strangers, move those packs to a separate process.
- A render can stall frames until the watchdog fires. Mitigation: the per-entry limits above, and memo caching.
- Scripts wrapped from `~/.config/echospot/scripts` are not sandboxed. They are the user's own and already run with full rights today.

---

## 16. Effects, dry runs and undo

- **Dry run.** `echospot dev run github.merge --dry` executes the ability with every `effect` and `final` call stubbed and listed. The agent can verify what Enter would do without doing it.
- **Undo.** Each `effect` call records its reverse. After any action, a toast offers Undo for five seconds, and **Timeline** (a built-in surface) lists recent effects with Undo where possible.
- **Final.** `final` effects show a confirm step and are labelled "can't be undone".

---

## 17. Agent experience

### 17.1 The CLI

All subcommands support `--json`.

| Command | Purpose |
|---|---|
| `echospot dev new <id> [--from currency]` | scaffold a pack from a template |
| `echospot dev api search <words>` | find abilities, components, types, verbs |
| `echospot dev api show <name>` | signature, kinds, example, link to docs |
| `echospot dev check [--design]` | compile, type-check, lint, design check |
| `echospot dev render <surface> [--scene x]` | PNG plus text tree |
| `echospot dev scenes` | render every surface in every scene |
| `echospot dev run <address> --json '{…}' [--dry]` | call an ability |
| `echospot dev test` | run `tests/*.luau` with fake stores and recorded HTTP |
| `echospot dev icon check <svg>` | §14.2 |
| `echospot dev eject <component>` | §13.4 |
| `echospot dev logs [--follow]` | prints, warnings, errors, audit |
| `echospot dev inbox` | point-and-say reports (§17.8) |
| `echospot dev grants` | what the pack has and still needs (read-only) |
| `echospot dev docs [topic]` | documentation for this installed version |

### 17.2 Types before running

The host generates `echospot.d.luau`: definitions for `@echo`, `@echo/ui`, `@echo/types` and
every address the pack may call, with exact input and output types. `echospot dev check` runs the
Luau type checker on the pack, so most mistakes are found before anything runs. `luau-lsp` works
in editors through `.luaurc`.

### 17.3 Errors that teach

Every error has a code, a location, what was expected, the nearest valid names and a command to
learn more:

```json
{ "code": "E0412", "at": "init.luau:31:9",
  "message": "ui has no component 'Table2'",
  "nearest": ["Table", "Grid", "KeyValue"],
  "learn": "echospot dev api show Table" }
```

### 17.4 Docs that cannot drift

The docs, the type definitions and the JSON schemas are generated from the same declarations the
host uses. `echospot dev docs` prints them for the installed version.

### 17.5 Scenes and fixtures

```luau
-- scenes/failing.luau
return { load = { ["github.prs"] = { { title = "Fix CI", failing = true, url = "…" } } } }
```

Scenes replace ability results and store contents. Recorded HTTP fixtures make tests run
offline: `echospot dev test --record` captures real responses once.

### 17.6 The skill

`echospot agent install` writes:

- a Claude Code skill (`~/.claude/skills/echospot/SKILL.md`);
- an `AGENTS.md` section for Codex and others.

The skill stays short: when to use it, the loop (**new → check → render → look at PNG → fix → test**), the design and icon rules, the safety rules, and "run `echospot dev docs` for everything else". Short means it never goes stale.

### 17.7 Intent file

`INTENT.md` holds the human's original request in their words, decisions made and why, and known
limits. Agents read it before editing and append to it after.

### 17.8 Point and say

On any EchoSpot surface, a hotkey (proposed: `Ctrl+.`) captures:

- the pack, surface, scene state and the exact element (Rune keys give a stable path);
- a screenshot;
- one sentence from the user ("show the repo name here").

It is saved to `inbox/<time>/`. The next agent session reads it with `echospot dev inbox`, and
can replay the exact state. Errors get a "Report to agent" button that does the same.

### 17.9 The flywheel

Every ability is also a tool for outside agents through `echospot call` and an MCP server
(`echospot mcp serve`). Each pack an agent builds makes the next agent more capable on this
machine. Effects over MCP still go through the broker and grants.

---

## 18. Answers become commands

EchoSpot's AI can answer with a **live surface** instead of text: blocks (Table, Chart, List,
actions) bound to abilities. "Keep" turns that answer into a permanent pack.

It is safe because a generated surface is only block data plus calls to abilities that are
already granted. No new code runs.

---

## 19. Forking built-ins

New EchoSpot features are written as **system packs** on this API and shipped inside the binary.
`echospot fork clipboard` copies a system pack's source to the user's packs, where the copy takes
over its id.

Existing Rust features are not rewritten at once: they are exposed as abilities now (§6.6) and
moved to system packs over time.

---

## 20. Rust tier (later)

For heavy compute or crates: Rust compiled to WASM components, run with wasmtime.

- The same declarations generate a WIT interface, so Rust packs see the same abilities, types and broker.
- **Rust packs are headless:** they provide abilities and stores. UI comes from default faces or a Luau file in the same pack.
- Compiled once to native code by wasmtime and cached, so later loads are fast.
- Build progress drives the `checking` loader.

---

## 21. Raycast compatibility

The existing Raycast host (Node runtime, `src/ext/`) stays as is, frozen. Its commands appear as
opaque `raycast.*` addresses: runnable, not typed. Nothing in this design depends on it.

---

## 22. Worked examples

### 22.1 A recipe: currency command (no permissions)

§6.1 is the whole pack. It calls only the free built-in `calc.convert`, so it runs immediately
with no grant screen. Default faces give it a root-search row, a panel and a board tile.

### 22.2 An extension: GitHub PRs with fire on failing ones

```toml
# pack.toml
id = "github"
name = "GitHub"
api = 1
[permissions]
net = ["api.github.com"]
secrets = { github = ["api.github.com"] }
```

```luau
--!strict
local echo = require("@echo")
local t = require("@echo/types")
local ui = require("@echo/ui")

local pr = t.record("pr", { title = t.text(), url = t.url(), repo = t.text(), failing = t.bool() })

return echo.pack {
    abilities = {
        prs = echo.ability {
            title = "My Pull Requests",
            icon = "icons/prs.svg",
            kind = "read",
            input = { repo = t.text():optional() },
            output = t.list(pr),
            run = function(input, cx)
                local res = cx:call("http.get", {
                    url = "https://api.github.com/search/issues?q=is:pr+is:open+author:@me",
                    auth = "github",
                })
                return echo.map(res.json.items, function(i)
                    return { title = i.title, url = i.html_url, repo = i.repository_url, failing = false }
                end)
            end,
            faces = {
                panel = function(view, cx)
                    return ui.List {
                        items = cx:load("github.prs", {}),
                        title = "title", subtitle = "repo",
                        intent = function(p) return p.failing and "urgent" or nil end,
                        empty = "No open pull requests",
                    }
                end,
            },
        },
    },
}
```

Note what is absent: no loading code, no empty-state drawing, no "open URL" action (the `url`
field brings its verbs), no styling.

### 22.3 Composition: using your Pi script

```toml
[permissions]
calls = ["scripts.pi"]
```

```luau
run = function(input, cx)
    local digits = cx:call("scripts.pi", { args = { tostring(input.n) } })
    return { value = digits, at = os.clock() }
end
```

### 22.4 An overlay: a focus timer in the screen corner

```luau
surfaces = {
    timer = echo.overlay {
        place = "top-right", input = "none",
        render = function(view, cx)
            local left = cx:store("focus.remaining")
            return ui.Stat { value = left, look = "ink", intent = left < 60 and "urgent" or "live" }
        end,
    },
},
```

---

## 23. Performance budgets

| What | Budget |
|---|---|
| Launcher open with packs installed | no slower than without (index is cached, §5.4) |
| Query per keystroke | 8 ms per pack, 16 ms total |
| Render entry | 50 ms hard stop; warning at 8 ms in `dev logs` |
| Any other entry | 250 ms hard stop (rune-luau default) |
| Pack heap | 64 MB default |
| Running effect layers per surface | 3 |
| Pack load after save | under 100 ms for Luau |

`rune-luau`'s `Script::metrics()` and `memory()` feed a per-pack cost view in Settings.

---

## 24. Roadmap

Each milestone ends with something demonstrable.

**M0: spike.** Embed rune-luau in EchoSpot. One pack renders a panel; hot reload works. Measure
open and reload times. *Exit:* numbers within budgets, or a decision to change course.

**M1: core.** Pack format, loader states, abilities, kinds, broker, grants screen, network and
secrets, `@echo/ui` with List, Detail, Form, Stat; one Look; `dev new/check/render/run/logs`;
`echospot.d.luau`; icon check; the skill. *Exit:* an agent builds the GitHub pack (§22.2) from a
one-sentence request without human code edits.

**M2: composition.** Built-in abilities catalogue, wrapping scripts, snippets and quicklinks;
types and verbs; Send to…; stores and triggers; root-search answers; boards. *Exit:* a morning
board mixing built-ins, a script and a pack.

**M3: quality and AI.** Design checker, stress scenes, exemplars, eject, intents and effect
budget; AI primitives; dry runs, undo and Timeline; point and say. *Exit:* checker passes on all
system packs; an inbox report is fixed by an agent end to end.

**M4: reach.** Overlays, widgets, bar feed, answers become commands, MCP serve, forking
built-ins, Rust tier.

---

## 25. Open questions

1. Hotkey for point and say: `Ctrl+.` or something else?
2. Default Look for new packs: the user's current Look, or always `glass`?
3. Should `ai.agent` (§12) be in M3 or wait until budgets have been used for a while?
4. Where boards live by default: palette route or their own window?
5. Do system packs ship as Luau source inside the binary (forkable) or compiled bytecode plus source on demand?

---

## Appendix A: What `rune-luau` already provides

Read from `../rune/crates/rune-luau` (version 0.0.1, pre-release).

- `Script`: one sandboxed Luau VM per script, native code on Linux x86-64 and arm64.
- `@rune/ui`: `row col box spacer text virtualList each hex`, built straight into `rune::El` with no tree diff. Props cover layout, colour by palette name, radius, shadow, hover, press, focus, accessibility labels, text styling.
- `view:Memo(key, version, build)` and `ui.each`: per-item memoisation.
- `task`: `spawn defer delay wait cancel` on the frame clock. An idle script never enters its VM.
- Signals: host signals (`Connect Once Wait`) and script signals (`Fire Version`).
- `Host::async_function` with a `Completer` finished from any thread; `spawn_blocking` for work off the UI thread.
- `Host::register_module`, `Host::global`, and `HostNode` for adding `ui.<name>` nodes built in Rust (with props, children and handlers).
- `script.call`, `script.query`, `script.fire`, named surfaces (`script.surface(cx, name)`).
- Hot reload with `saveHotState` / `loadHotState`; a failed reload keeps the running VM.
- Errors never panic: render errors show an error card with Reload; task errors are logged.
- Limits: watchdog (250 ms default) and heap (64 MB default); `metrics()`, `memory()`, `log()`.
- `Data`: plain values across boundaries; serde decode with the `serde` feature.

## Appendix B: Other Rune pieces this uses

- `rune-fx`: effects `Glow Pulse Electric Fire Distort MotionBlur Shimmer Sparkles Aurora Grain` and fills (liquid metal, mesh gradient, dither…). A catalogue of parameters with ranges and presets, and `Fx::cost`.
- `rune-kit`: list, table, combobox, select, dialog, toast, markdown, syntax, tree and more, as candidates for host nodes.
- `rune-motion`: transitions, springs, choreography for built-in motion.
- `rune-light`: glass material.
- `rune::overlay`: layer-shell overlays on Wayland with placement, input regions and keyboard policy.

## Appendix C: Changes needed in Rune or rune-luau

- Per-entry time limits (render vs task), since `time_limit` is per script today.
- An option to list which built-in modules a VM gets, if we want to hide raw `@rune/ui` later.
- Host nodes for `icon image input fx glass markdown code chart`: written in EchoSpot with `HostNode`, no rune-luau change needed.
