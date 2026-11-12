# EchoSpot

A native Raycast alternative for Omarchy, built with [Rune](https://github.com/anomalyco/rune).

> **Warning**  
> This project is an active work in progress. Features are still under active development and subject to breaking changes.

## Features

- **Command Palette & Launcher**: Fast application launcher and system commands. Any row can
  have an **alias** and a **global hotkey** (Ctrl+Shift+K on the row).
- **Calculator**: Arithmetic in symbols or words, percentages, functions, number bases, unit conversions and unit arithmetic, live currency and crypto rates, date maths and countdowns (`days until 25 dec`), and time zones (`5pm london in tokyo`). Also `echospot calc <expression>` from a terminal.
- **Clipboard History**: Resident clipboard manager for text and images.
- **Emoji & Symbols**: Searchable emoji and symbol catalogue with recents and pins.
- **File Search**: Quick file finder.
- **Snippets & Quicklinks**: Saved text and custom search shortcuts. Snippets can expand as you
  type their keyword in any app (off by default; Settings → General).
- **Notes**: Markdown notes kept as plain files in `~/.local/share/echospot/notes`.
- **My Schedule**: The next two weeks from any iCal feed or `.ics` file, with Join Meeting.
- **Script Commands**: Scripts in `~/.config/echospot/scripts` with Raycast's `@raycast.*`
  headers become launcher commands.
- **AI**: Quick AI and chat with Claude, OpenAI, OpenRouter, Vercel AI Gateway or any
  OpenAI-compatible endpoint. Your own quick actions and chat presets, file and image
  attachments, web search, and tools from MCP servers (`~/.config/echospot/mcp.json`).
- **Window Management**: 42 layout commands for Hyprland (halves, thirds, fourths, edges,
  sizing, restore) and a window switcher.
- **System Controls**: Omarchy system toggles and themes.
- **Extensions**: Runs Raycast extensions, including menu-bar commands (shown as lists),
  background refresh, and `AI.ask`.
- **Backup**: `echospot backup` / `echospot restore`, or Export/Import Backup in the launcher.

`FEATURES.md` tracks what is built, what was verified, and what is still missing.

## License

Licensed under the [GNU Affero General Public License v3 (AGPL-3.0)](LICENSE).
