# Changelog

User-facing changes, newest first.

## Unreleased

- Website service (Services & Secrets): pick your host, set the address, choose landing / privacy / terms pages and let Claude make and deploy them. The Bundle ID suggestion follows the address; from Release candidate up, iOS/Android projects need the privacy and terms pages online.
- AI accounts can use a custom endpoint (proxy/gateway): base URL + API key, stored in the OS keychain.
- Tasks are GitHub issues: GitHub projects keep one queue of open `clauracle`-labelled issues (type pills, drag to
  reorder, Suggestions to accept or dismiss, Cleanup). The Plan block lives in the GitHub tab; Agents got their own tab.
- Questions that block the work pause the auto run; answering resumes it. Answered cards stay read-only in the chat.
- Error cards: a red eye and "I'm sorry, <name>."; blocked actions say "I'm afraid I can't do that."
- Auto-push after each turn. Release after build: optionally publish `v<version>` once a build succeeds.
- Release panel lives in the Build tab under the release wizard; Publish sets the version, builds and releases in one click.
- Step wizards remember progress per project, fade done steps and can be reset.
- Right tabs follow the dev flow; the chat no longer jumps while you're scrolled up.
- CI can publish release assets again.

## 0.1.1 (2026-11-05)

- Bundle ID is now `no.arctim.clauracle` (Windows moves its WebView data over once).
- Release wizard: a checklist per stage (alpha, beta, release candidate, published), plain checks plus AI fix prompts.
- Release panel: tag defaults to the project version, warns on a mismatch, offers Set version and build.
- Second launch focuses the running window; Build/Run/Deploy in one row, Run turns into Stop while the app runs.
- Plan tab: tick, drag, Cleanup and Suggestions; the working task shimmers while the project is busy.
- Mac signing stepper: CSR copied to Downloads, `.cer` picked up automatically, editable Bundle ID.
- Own confirm dialogs, labelled model/effort dropdowns, token estimates on AI buttons, context % that never blanks.
- Permissions: compound shell commands split into parts, one Allow rule per part.
- The window never freezes on git, gh, openssl or keychain calls.
- Many fixes: Run/Stop end the whole process tree, multi-line plan tasks, stale reorders, CRLF-safe version swap.

## 0.1.0 (2026-11-05)

First release of the desktop app (Tauri 2, Windows / macOS / Linux).

- Projects side by side, each with its own Claude session, AI account, GitHub account, model, permissions,
  connections, skills, build commands and `.env`.
- Several Claude accounts with pools that take over at the 5h/weekly limit; several GitHub accounts via per-project `GH_TOKEN`.
- Clear & continue (Manual, Auto, Auto + answers) with a handover file, question cards and desktop/phone notifications.
- Connections, skills and plugins load only when picked; Settings › Claude Code edits the CLI's own settings.
- Permission cards with Allow for this session, 24 hours or always.
- Side panel: GitHub, Build (auto-detected build/run/deploy), `.env`, Code, in-app Browser with a phone view and QR.
- Mac signing service: CSR and certificate to `.p12`, sent to GitHub secrets; CI signs and notarizes when they exist.
- Chat and Terminal views, tool rows with diffs, file edit badges, themes with accents and tint.
