# Clauracle

Tauri 2 desktop app that wraps the user's installed Claude Code CLI (stream-json protocol, spoken directly) and GitHub through `gh`.
It is a *wrapper*: commands, skills, MCP servers, hooks and settings all come from the real CLIs.
Current state and open work: see `HANDOVER.md`. Frozen, don't edit: `archive/tui/` (old TUI), `archive/python/` (pywebview prototype).

## Files
- `src-tauri/src/lib.rs`: Tauri commands (what the page calls), child-process tracking, app setup.
- `src-tauri/src/claude.rs`: one session per project: spawns `claude --input-format stream-json …`, control protocol
  (initialize with skills/agents, can_use_tool, in-process MCP `clauracle` with `ask`/`ready`), clear-continue, handover, build steps.
- `src-tauri/src/store.rs`: `~/.clauracle/config.json`, accounts (tokens in the OS keychain), usage/limits/pools, gh helpers,
  catalog of MCP/skills, installers, .env parsing, browsers.
- `ui/index.html`: the whole UI, vanilla JS, no build step. Rust → page: event `clauracle` `{event, data}`.
- `plugin/`: Clauracle's built-in Claude Code plugin (Ponytail skills, MIT), bundled as a resource.

## Commands
- Run: `npx tauri dev`. Test: `cargo test --manifest-path src-tauri/Cargo.toml`. Build: `npx tauri build` (bundle for this OS).
- If cargo isn't found, prefix PATH with `~/.cargo/bin`.
- UI is embedded at compile time: rebuild after editing `ui/index.html`. Syntax-check its script with `node --check`.
- Visual check: run the debug exe and capture with PrintWindow; never drive the user's mouse/keyboard (SendKeys, clicks).

## Conventions
- Keep it minimal: std first, no new deps without a reason. Mark deliberate shortcuts with `ponytail:`.
- Settings are deny-lists (`off`), so new connections/skills start on. Switched-off items become `--disallowedTools`.
- The project is the source of truth: its `gh`/`off` are copied from its AI account at creation and never follow it after.
- Never touch `gh auth switch`: per-process `GH_TOKEN` (+ git credential helper via `GIT_CONFIG_*`) instead.
- Account/settings changes reconnect sessions after the current reply (`Input::Reconnect`).

## Gotchas
- Shells started by Claude Code carry `NO_COLOR=1` and `CLAUDE*` vars; strip them before launching for a check.
- Windows: `.cmd` shims run via `cmd /c`; children use CREATE_NO_WINDOW and are killed with `taskkill /T` on exit.
- Config written by PowerShell can carry a BOM; the loader strips it, and backs up an unparsable file to `config.broken.json`.
- Bash heredocs feeding code mangle backslashes: edit files with the Edit tool instead.
