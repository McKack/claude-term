# claude-term

A themeable terminal UI that wraps your installed Claude Code (via claude-agent-sdk).
Same commands, skills, MCP servers, hooks and settings; new look.

    pip install -r requirements.txt
    python claude_term.py [project-dir]

- Themes: Classic (default), Matrix, Amber. `/theme` or ctrl+t, live preview.
- Menu bar (Claude / View / Help; permissions + web and Claude Code commands are submenus), welcome screen, recent sessions drawer (ctrl+r).
- Composer: enter send, ctrl+j new line, `/` suggestions, shift+tab permission mode,
  model chip → account / model / effort / web search.
- Streaming, diffs, task sidebar, question forms, permission dialogs.
- Extra accounts: `claude setup-token` per account; tokens live in Windows Credential Manager.
- Claude can drive the UI through the in-app MCP server `ui` (side panel, toasts, theme, dialogs, clear-continue).

Preferences: `~/.claude-term/config.json`. Smoke test: `python test_claude_term.py` (no Claude calls).
