# EchoSpot feature backlog

The features Tinycast (macOS) and Raycast have that EchoSpot does not yet. Tick a box when the
feature is built **and** verified; add a line under it saying what was verified and what was not.
Any agent can pick up the first unticked item.

Status key: `[ ]` not started · `[~]` in progress (say who/what is left) · `[x]` built and verified

## How to work on this

- Build: `cargo check`, then `cargo build --release`. Super+Space runs `target/release/echospot`,
  so restart the resident process after a release build: `echospot quit`, then start
  `target/release/echospot` detached.
- Tests: `cargo test` for pure logic. For UI, Rune renders headlessly: write events to a JSON
  file and run `target/debug/echospot --script ev.json --snap out.png --duration 2`, then look at
  the PNG. Events: `{"t":0.2,"type":"type","text":"zz"}`,
  `{"t":0.6,"type":"key","name":"k","cmd":true,"shift":true}` (`cmd` is Ctrl here). It reads the
  real config, so remove any test data you add. `cargo check` does not rebuild the binary; use
  `cargo build`.
- Never `pkill -f echospot` (it matches your own shell). Use `pkill -x echospot` or `echospot quit`.
- Hyprland here is 0.56 with a **Lua config**. `hyprctl dispatch` takes Lua
  (`hl.dsp.window.float({ window = "address:0x…", action = "enable" })`), not the old words.
  `windows::dispatch(lua, legacy)` tries Lua first and falls back. Float actions are
  `enable/disable/toggle`; fullscreen and pin are `toggle/set/unset`.
- Test window commands on a throwaway window (`foot -a echospot-test`) moved to a spare
  workspace, never on the user's own windows.
- New list screens follow `src/features/snippets.rs` (store + `build` + form + `perform` + `save`):
  add a `Route`, an `Act`/op enum in `src/model.rs`, a `forms::Target`, a row in
  `root::builtins()`, an `ipc::route` name, and the match arms in `App::refresh` / `App::perform`.
- Config lives in `~/.config/echospot`, data in `~/.local/share/echospot`.
- There are uncommitted changes in the tree (multi-provider AI, Settings redesign, window fix).
  Do not revert files you did not write.

## Done before this list

- [x] Window commands work on Omarchy's Lua Hyprland (`src/features/windows.rs`).
      Verified: all 25 commands on a test window. Not verified: Move to Next Display (one monitor).
- [x] Multi-provider AI (Claude, OpenAI, OpenRouter, Vercel, custom) and Settings redesign.
      Verified: compiles, tests pass. Not verified: run in the app against live providers.

## 1. Window management (Tinycast has 39 commands, EchoSpot now 42)

- [x] Fourths: First / Second / Third / Last Fourth, Center Half, Center Two Thirds
- [x] Move to edge without resizing: Move Window to Left / Right / Top / Bottom Edge
- [x] Make Window Larger / Smaller
- [x] Restore Window (back to the frame before the last command; kept in memory, per window)
- [x] Tile Window (stop floating), Move Window to Previous Display
- [x] Reasonable Size (60% × 70%, centred)

- [x] Snap Layouts (`src/features/snap.rs`): "Snap Layouts" (`cmd:snap`, `echospot open snap`) and
      one command per layout (`snap:halves`, `snap:two-thirds`, `snap:thirds`, `snap:quarters`,
      `snap:side-stack`, `snap:center`) open a picker: the staged layout drawn as the screen, a
      rail of layouts under it, the workspace's windows queued beside it. A click, a digit, or
      the arrows and Return place the window being placed and move on to the next; Tab or a
      queue row picks another window; "Arrange All" (Ctrl+Return) arranges every window.
      Verified: zone and arrow logic (unit tests); the picker, single-layout view and action menu
      in headless snapshots; `arrange` and `snap` frames on test windows. Not verified: a click or
      Return in the real palette, and the palette staying up and on top between two placements.

Verified: every new command run against a test window, frames checked. Not verified: Previous
Display (one monitor). Not built: custom sizes and saved window layouts ("Rooms").

## 2. Aliases and per-command hotkeys (`src/features/bindings.rs`)

- [x] Alias on any launcher row (apps, commands, extensions): typing it exactly ranks that row first
- [x] "Set Alias or Hotkey…" action (Ctrl+Shift+K) on launcher rows, stored in
      `~/.config/echospot/bindings.json`
- [x] `echospot run <command-id>` over IPC runs a row's primary action; `echospot commands` lists ids
- [x] Global hotkey per command: registered at startup and on every summon with
      `hyprctl eval 'hl.unbind(k); hl.bind(k, …)'` calling `echospot run <id>`
- [x] Alias and hotkey shown on the row
- [ ] Hotkeys and aliases listed (and removable) in Settings
- [ ] Hotkeys come back right after `hyprctl reload` (today: on the next summon)

Verified: chord parsing (unit test); binding appears in `hyprctl binds`; `run` opens a view
command and reports an unknown id; alias ranking and the form checked in headless snapshots.
Not verified: pressing a real hotkey on the keyboard.

## 3. Script commands (`src/features/scripts.rs`)

- [x] Scripts in `~/.config/echospot/scripts` with Raycast headers (`# @raycast.title`, `mode`,
      `icon`, `packageName`, `description`, `argument1..3`, `needsConfirmation`,
      `currentDirectoryPath`) appear in the launcher; rescanned on every summon
- [x] Modes: `silent` (HUD with the last line), `compact` (toast with the last line),
      `fullOutput` (a Markdown page, with Copy Output and Run Again), plus `terminal`
- [x] Arguments asked in a form before running (text, password, dropdown)
- [x] "Create Script Command" (form, writes a template, opens it) and "Open Scripts Folder"
- [x] One-line shell commands from `~/.config/echospot/commands.json` (`id`, `name`, `command`,
      `description`, `icon`, `in_terminal`) are launcher rows too
- [ ] `inline` mode: output on the row itself, refreshed on `refreshTime` (today it runs as compact)
- [ ] Import a folder of Raycast script commands

Verified: header parsing (unit test); a fullOutput script with an argument run end to end in a
headless snapshot. Not verified: silent, compact and terminal modes on screen.

`App::open_page(Page { title, markdown, actions })` is a general full-width Markdown screen
(`Route::Page`); reuse it.

## 4. Notes (`src/features/notes.rs`)

- [x] Markdown files in `~/.local/share/echospot/notes`, one per note; first `# line` is the title
- [x] "Search Notes" screen: list beside a rendered preview, search by title and body
- [x] Create, edit (form with a text area), delete, copy, paste, copy as Markdown, open in editor
- [x] "Create Note" in the launcher; typing a title that matches nothing offers to create it;
      `echospot open notes`
- [ ] A floating always-on-top note window (Tinycast's "floating editor")

Verified: title parsing (unit test); a note created through the form and shown in the list, in
headless snapshots.

## 5. Calendar, "My Schedule" (`src/features/calendar.rs`)

- [x] Calendars as iCal URLs (`https://`, `webcal://`) or `.ics` files, kept in `settings.json`
      under `calendars`
- [x] Parse VEVENT: DTSTART/DTEND/DURATION (UTC, TZID, floating, all-day), SUMMARY, LOCATION,
      DESCRIPTION, URL, STATUS:CANCELLED; folded lines and escapes
- [x] Recurrence: RRULE DAILY/WEEKLY/MONTHLY/YEARLY with INTERVAL, BYDAY (with ordinals),
      BYMONTHDAY, COUNT, UNTIL; EXDATE; RECURRENCE-ID overrides
- [x] "My Schedule" screen: today and the next 14 days grouped by day; feeds cached on disk for
      15 minutes and read off the UI thread; `echospot open schedule`
- [x] "Join Meeting" for Meet/Zoom/Teams/Webex/Whereby/Jitsi links; "Up Next" (now and the next
      3 hours) at the top of the launcher
- [x] Add and remove a calendar from the palette
- [ ] A calendars list in Settings
- [ ] Creating or editing events (read-only today); CalDAV accounts

Verified: parser and recurrence (unit tests); the screen and Up Next in headless snapshots with
a local `.ics` file. Not verified: a real Google/Outlook feed over the network.

## 6. Snippet expansion while typing (`src/features/expander.rs`)

- [x] Keyboard watcher reading `/dev/input/event*` (needs the `input` group), off by default;
      new keyboards picked up within 8 s
- [x] Typing a snippet's keyword in any app erases it and inserts the snippet: typed with `wtype`
      when short and single-line, pasted (clipboard restored after) otherwise
- [x] Buffer rules: reset on Return/Escape/Tab/arrows, on any Ctrl/Alt/Super chord, after 15 s
      idle; backspace followed; max 256 chars; longest keyword wins; a keyword of letters only
      fires at the start of a word
- [x] "Expand snippets as you type" switch in Settings → General, with a note when the keyboard
      cannot be read; the snippet form explains the keyword
- [x] Ignored while EchoSpot itself has focus
- [ ] Layouts other than US (keys are read as US QWERTY)
- [ ] Reset the buffer on a mouse click

Verified: the matcher (unit test); the keyboards are readable by this user. **Not verified:
a real expansion in a real app** — it needs physical typing with the switch on. Try it first.

## 7. AI (`src/features/ai.rs`, `ai_wire.rs`, `ai_library.rs`, `mcp.rs`)

- [x] Custom AI actions: title + instruction, run on the selected text; create/edit/delete from
      the AI screen; listed beside the seven built-in ones and in the launcher (`ai.json`)
- [x] Chat presets: saved instructions + optional model; "New Chat with Preset" from the AI
      screen or the launcher ("Chat with <name>"); a resumed chat keeps its preset's instructions
- [x] Attachments: images (PNG/JPEG/GIF/WebP, up to 5 MB), text files, PDFs via `pdftotext`.
      "Attach from Clipboard" (Ctrl+Shift+A) in the chat, "Ask AI About This File" in Search Files
- [x] Web search: Anthropic `web_search_20260209` (basic variant for pre-4.6 models), OpenRouter
      `web` plugin; toggle with Ctrl+Shift+W, remembered in settings
- [x] MCP tools: stdio servers from `~/.config/echospot/mcp.json` (Claude Desktop's shape); a
      tool-use loop for both the Anthropic and OpenAI wire formats (up to 12 rounds, thinking
      blocks replayed unchanged, `pause_turn` continued); calls shown in the answer; toggle with
      Ctrl+Shift+M; `echospot mcp` lists tools, `echospot mcp call <server__tool> '<json>'`
- [x] The AI screen says when a provider has a key but no model chosen
- [ ] Vercel and Custom still start with no default model (the user must pick one)
- [ ] Gemini, Apple-Intelligence-style local models, and CLI providers (claude, codex)
- [ ] Remote (HTTP) MCP servers; asking before a tool with side effects runs
- [ ] A file picker for attachments; image downscaling
- [ ] Settings pane rows for web search, tools, presets

Verified: stream parsers for both formats, base64, attachments (unit tests); the MCP client
against a fake server; **the whole tool loop end to end** (fake OpenAI-compatible server + fake
MCP server) in a headless snapshot; presets and custom actions on screen. **Not verified against
a real provider**: Anthropic's live stream with tools/web search, OpenRouter's web plugin, image
attachments. No API credit was spent testing.

## 8. Extension runtime (`runtime/src/api.js`, `runtime/src/host.js`, `src/ext/`)

After editing `runtime/src`, run `npm run build` in `runtime/` and then `cargo build` (the bundle
is compiled into the binary).

- [x] `AI.ask` (and so `useAI`) answered by the provider set up in Settings. The request is made
      by EchoSpot itself (`ai::one_shot`), so the key never enters the extension's process.
      `environment.canAccess(AI)` is true when a provider is ready
- [x] `menu-bar` commands: listed in the launcher; their `MenuBarExtra` tree (items, sections,
      submenus) opens as a searchable list, and an item's `onAction` runs on Return
- [x] Background refresh: a command with `interval` in its manifest gets "Turn Background
      Refresh On" in its actions; enabled commands (`~/.config/echospot/background.json`) run on
      that interval (checked every 30 s, never oftener than once a minute, one run at a time),
      silently, with `launchType: "background"`
- [x] `updateCommandMetadata({ subtitle })` shows on the command's launcher row
- [x] `WindowManagement` through `hyprctl`: `getActiveWindow`, `getWindowsOnActiveDesktop`,
      `getDesktops`, `setWindowBounds`
- [ ] A real place in the bar for menu-bar commands (a Waybar module or tray icon)
- [ ] `AI.ask` streaming (`.on("data")` fires once, with the whole answer); `options.model` is
      ignored in favour of the model chosen in Settings
- [ ] `Action.ToggleQuickLook`, `getSelectedFinderItems`, `BrowserExtension`
- [ ] A switch for background refresh in Settings → Extensions

Verified: menu-bar list in a headless snapshot; `updateCommandMetadata`, `launchType`,
`canAccess`, `WindowManagement.getDesktops/getWindowsOnActiveDesktop` and the `aiAsk` call by
running the host under Node with a test extension; a scheduled background run on the release
build (the row's subtitle updated within a minute). Not verified: `setWindowBounds`, and
`AI.ask` answered by a real provider.

## 9. Backup and restore (`src/features/backup.rs`)

- [x] `echospot backup [file] [--with-keys]` and `echospot restore <file>`
- [x] "Export Backup" and "Import Backup" in the launcher
- [x] Covers settings, snippets, quicklinks, aliases/hotkeys, AI actions and presets, scripts,
      `commands.json`, notes and favorites. API keys, `mcp.json` and extension preferences are
      left out unless `--with-keys` / the checkbox. A restore first saves what is there to
      `~/.local/share/echospot/backups`, and a keyless backup does not wipe the keys in use
- [ ] Import from a Raycast `.rayconfig` export (snippets, quicklinks, aliases). Needs a real
      export file to test against
- [ ] Export/Import buttons in Settings; installed extensions themselves are not in the backup

Verified: export contents (no keys); restore into a sandbox, key preserved, safety backup
written, a non-backup file refused.

## 9b. File search scope (`src/features/files.rs`)

- [x] Search Files covers the home folder and every mounted drive (read from
      `/proc/self/mountinfo`; the system's own mounts are skipped), not the home folder alone
- [x] Settings → File Search: a switch per mounted drive, a list of excluded folders (typed,
      with `~`), and "Find hidden files"; kept in `settings.json` under `files`
- [x] "Exclude Folder from Search" (Ctrl+Shift+X) on a result, as Raycast's Exclude from Index
- [ ] A folder chooser for Excluded Folders (today the path is typed or pasted)
- [ ] EchoFiles' index as the engine when `ef` is installed (today always `fd`, or `find`)

Verified: mount parsing, typed paths and the scope (unit tests); fd's anchored `--exclude` on a
scratch tree; a folder on `/media/data106` found, and the Settings pane, in headless snapshots.
Not verified: adding and removing a folder, a drive's switch and Ctrl+Shift+X in the real app.

## 10. Housekeeping

- [x] Unit tests for `AiSettings::normalize` (old settings files keep their key) and provider
      switching
- [x] README lists the new features and mentions AI
- [x] Headless runs (`--snap`, `--script`) no longer talk to the resident palette
- [ ] Settings panes for the new features (hotkeys, calendars, MCP, background refresh, backup).
      Only "Expand snippets as you type" has a switch so far
- [ ] `icons::SPARKLES` has a stray quote in its SVG body and draws wrong (`src/icons.rs`)
- [ ] Code blocks in Markdown render in the proportional font in headless snapshots; check on
      screen whether the mono font resolves
- [ ] Task-list checkboxes in Markdown sit tight against their text (rune-kit)
