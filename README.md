# Clauracle

A small, project-first desktop app around your installed **Claude Code** and **GitHub CLI**, built with Tauri 2:
one codebase, native installers for Windows (`.exe`), macOS (`.app`/`.dmg`) and Linux (`.AppImage`/`.deb`), ~5 MB.
It drives `claude` and `gh` in the background, so your commands, skills, MCP servers, hooks and settings stay the CLIs' own.
Nothing else to install: if `claude` or `gh` is missing, Clauracle offers to install it.

- **Projects first.** Each project keeps its own Claude session, AI account, GitHub account, model, permissions,
  connections, skills, build commands and `.env`. Projects run side by side; switching never logs anyone out.
- **Several AI accounts.** Main is the `claude` CLI's own login; add more with *Sign in with Claude* (`claude setup-token`,
  token kept in the OS keychain). Accounts in the same **pool** take over from each other when one hits its 5h/weekly limit.
- **Several GitHub accounts** through `gh`, no GitHub app registration: each project gets its own `GH_TOKEN`
  (never `gh auth switch`). Clone any repo of any account; PRs and issues in the side panel.
- **Clear & continue:** *Manual* (stops when a task is done, you press Continue), *Auto* (clears and continues until a
  limit), *Auto + answers* (also answers recommended-type questions; real decisions wait). Handover lives in `HANDOVER.md`.
  Questions come as a numbered card with notes and an *Other* answer; in auto mode they push to desktop and phone (ntfy).
- **Token savers:** Ponytail (simplest code that works), dense comments, helper **agents** on cheaper models
  (Settings › Agents), `.env` hidden from Claude, build/run/deploy after each task done by code, not by the AI.
- **Side panel:** GitHub · Services & Secrets (`.env` with masked values, OAuth presets) · Build & Release · Agents · Code
  (with Hot: files coloured by recent change) · Browser (open in a real browser profile).
- **Chat / Terminal / Edit** views: Edit opens files as tabs with a Terminal and a real Shell below. Attach files or paste
  images into the message box; `/model` and `/build` pick a model or a build target.
- **Build & Release:** a per-stage checklist (alpha → published) with one-click fixes; finished builds are kept as staged
  releases until you publish them on GitHub.

## Run / build

    npm install
    npx tauri dev              # run (needs Rust: rustup.rs)
    npx tauri build            # installer for this OS in src-tauri/target/release/bundle/
    cargo test --manifest-path src-tauri/Cargo.toml

All three OSes: push a `v*` tag (or run the *build* workflow); `.github/workflows/build.yml` builds each and drafts a release.

The terminal predecessor, claude-term, is archived in `archive/tui/` (git tag `tui-final`); the Python prototype in `archive/python/`.
GPL-3.0 licence (`LICENSE`); what changed per version: `CHANGELOG.md`; who and what it's built with: `CREDITS.md`.
