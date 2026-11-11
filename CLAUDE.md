# Clauracle

Tauri 2 desktop app that wraps the user's installed Claude Code CLI (stream-json protocol, spoken directly) and GitHub through `gh`.
It is a *wrapper*: commands, skills, MCP servers, hooks and settings all come from the real CLIs.
Current state: `HANDOVER.md`. Open work: GitHub issues labelled `task`. Frozen, don't edit: `archive/tui/` (old TUI), `archive/python/` (pywebview prototype).

## Files
- `src-tauri/src/lib.rs`: child-process tracking, window, app setup; Tauri commands (what the page calls) in one file per area beside it (`projects.rs`, `setup.rs`, `tasks.rs`, `build.rs`, `release.rs`, `license.rs`, `report.rs`, `files.rs`).
- `src-tauri/src/claude/` (`mod.rs` types, `turn.rs`, `control.rs`, `steps.rs`, `plan.rs`): one session per project: spawns `claude --input-format stream-json …`, control protocol
  (initialize with skills/agents, can_use_tool, in-process MCP `clauracle` with `ask`/`ready`), clear-continue, handover, build steps.
- `src-tauri/src/store/` (`mod.rs` config, `mac`, `proc`, `cli`, `env`, `build`, `tasks`): `~/.clauracle/config.json`, accounts (tokens in the OS keychain), usage/limits/pools, gh helpers,
  catalog of MCP/skills, installers, .env parsing, browsers.
- `ui/`: the UI, vanilla JS, no build step. `index.html` = markup, `app.css` = styles, `js/*.js` = classic scripts loaded
  in order (one shared global scope: code that runs at load may only use what earlier files define). Rust → page: event `clauracle` `{event, data}`.
- `plugin/`: Clauracle's own Claude Code plugin, bundled as a resource. Third-party skills are never shipped: the page installs them
  from their source on first run (`APPSKILLS` in `ui/js/menus.js`) and updates them daily (`ext_update`).

## Commands
- Run: `npx tauri dev`. Test: `cargo test --manifest-path src-tauri/Cargo.toml`. Build: `npx tauri build` (bundle for this OS).
- If cargo isn't found, prefix PATH with `~/.cargo/bin`.
- UI is embedded at compile time: rebuild after editing anything in `ui/`. New page script → add its `<script>` tag to `index.html`.
- Check in the real app: `cargo build --manifest-path src-tauri/Cargo.toml`, then `node scripts/live.js "<expr>"` (starts the debug
  build, evaluates in its page, closes it; no arguments = smoke test). Done means this ran and you quote its output.
- Dogfooding: run the installed build, not `target/debug` (Windows locks a running exe, so cargo can't overwrite it).
  After building, the `mcp__clauracle__restart` tool with `exe` swaps to the new build; running turns resume.
- Visual check: run the debug exe and capture with PrintWindow; never drive the user's mouse/keyboard (SendKeys, clicks).
  Never ask the user for screenshots: capture yourself when crucial.

## Conventions
- Efficient by default: cache what the UI shows (avatars, gh lookups, QR, LAN ip…); no re-fetch/re-check within seconds
  or on every card open; keep token use low (compressed prompts/lessons, one task per session).
- Empty states never dead-end: "No X yet" carries a one-click action (enable/add in place) or a link to the exact setting.
- Keep it minimal: std first, no new deps without a reason. Mark deliberate shortcuts with `ponytail:`.
- Connections/skills are allow-lists per project/account (`on`): nothing is on until picked (a picker asks on every new
  project/account). `on: None` = legacy deny-list (`off`). Global `cfg.off` switches an item off everywhere.
  Disabled MCP servers become `--disallowedTools`; Clauracle's own `skill:clauracle:*` are always on.
  Ponytail is the project switch `ponytail`: while on, `~/.claude/skills/ponytail/SKILL.md` is added to the prompt.
- The project is the source of truth: its `gh`/`off` are copied from its AI account at creation and never follow it after.
- Never touch `gh auth switch`: per-process `GH_TOKEN` (+ git credential helper via `GIT_CONFIG_*`) instead.
- Account/settings changes reconnect sessions after the current reply (`Input::Reconnect`).

## Gotchas
- Shells started by Claude Code carry `NO_COLOR=1` and `CLAUDE*` vars; strip them before launching for a check.
- Windows: `.cmd` shims run via `cmd /c`; children use CREATE_NO_WINDOW and are killed with `taskkill /T` on exit.
- Config written by PowerShell can carry a BOM; the loader strips it, and backs up an unparsable file to `config.broken.json`.
- New app icon (`npx tauri icon assets/icon.svg`): run `cargo clean -p clauracle` first, or the exe keeps the old one.
- gcloud, like gh: per-process `CLOUDSDK_CORE_ACCOUNT/PROJECT` from the project, never `gcloud config set`.
- A plain `#[tauri::command] fn` runs on the window's main thread: if it spawns git/gh/openssl, touches network/keychain
  or creates a webview, make it `#[tauri::command(async)]`, or the whole window freezes (no crash, no log) while it waits.
- Bash heredocs feeding code mangle backslashes: edit files with the Edit tool instead.
