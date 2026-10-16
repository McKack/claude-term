# claude-term

A themeable Textual TUI that wraps the user's installed Claude Code CLI through `claude-agent-sdk`.
It is a *wrapper*: commands, skills, MCP servers, hooks and settings all come from the real CLI.
Current state and open work: see `HANDOVER.md`.

## Files
- `claude_term.py`: the app. Lanes (Code session + Ask Claude side chat), rendering, tasks, menus, actions.
- `ui.py`: widgets, themes, menus, dialogs. No SDK imports.
- `store.py`: `~/.claude-term/config.json`, accounts (tokens in Windows Credential Manager via keyring),
  per-project task cache, welcome-screen facts.
- `attach.py`: file picker / clipboard (PowerShell) and turning files into message content blocks.
- `test_claude_term.py`: headless smoke test, no Claude calls. `build.ps1`: PyInstaller exe (`-Install` = PATH).

## Commands
- Run: `python claude_term.py [project-dir] [--resume session-id]`
- Test: `python test_claude_term.py` (must print `ok`); `python store.py`, `python attach.py` self-checks.
- Build: `powershell -File build.ps1` → `dist\claude-term.exe` (~25 MB, uses the installed `claude`).
  `dist\` is on the user PATH, so a rebuild ships it everywhere. Safe to run from inside claude-term:
  the running exe is renamed to `claude-term.old-*.exe` (cleaned up on the next build). Rebuild after every change.
- Screenshots: tests save `screenshot-*.svg`; render with headless Chrome to check visuals.

## Conventions
- Keep it minimal: stdlib/Textual first, no new deps without a reason. Mark deliberate shortcuts with `# ponytail:`.
- Every background worker uses `exit_on_error=False`, and client calls go through `self.safely(...)`.
  An uncaught worker error kills a Textual app.
- Helper agents (task scout, Ask Claude) pass `extra_args=NO_HISTORY` so they stay out of the resume list.
- The action dispatcher is `act()`. Don't name anything `run_action`, which collides with Textual's App method.

## Gotchas learned the hard way
- Shells started by Claude Code carry `NO_COLOR=1` and `CLAUDE*` vars. Strip them before launching the TUI
  for a visual check, or everything renders black and white.
- Windows Terminal sends an *empty* paste when the clipboard holds an image. That's how Ctrl+V detects images.
- PowerShell 5.1 treats native stderr as errors: no `$ErrorActionPreference = "Stop"` around PyInstaller.
- Bash heredocs feeding Python mangle backslashes (`\b`, `\n`): edit files with the Edit tool instead.
- Never name a temp script after a module (`click.py` shadowed the `click` package).
