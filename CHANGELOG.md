# Changelog

User-facing changes, newest first.

## Unreleased
- Chat: identical status notes in a row (e.g. "Resumed session") collapse into one line with a ×2, ×3… count.
- Windows: Clauracle draws its own title bar in the app's theme, with native measures (32 px, 24 px when maximised), a restore icon and a dimmed title when the window is not active.
- Any `/command` of the Claude CLI now goes to it as typed and its reply shows in the chat; options a command names (`/effort`, `/model`, `/config`, `/output-style`…) become buttons.
- Composer: one Skills button. On top are Clauracle's picks, installed in the background on first run and ticked per project: Ponytail, Impeccable, Superpowers' verify-before-done, Playwright and Context7 (Godot on a click). Every other skill sits in a folder per source.
- No third-party skill ships inside Clauracle any more: Ponytail too is downloaded from its source on first run, and installed skills are updated in the background once a day.
- "Add" buttons sit in the section header, top right, in one style (Settings accounts, GitHub accounts, helper agents, projects, services).
- One look for the same thing everywhere: dropdowns, section headers (Settings and the right panel), dialog titles, controls in one row share a height, and Refresh, Edit, Copy and Show each have one icon.
- Removing a service, deleting a secret, removing a helper agent, cleaning disk space and dismissing a task now ask first.
- Empty lists and panels offer their next step (open a folder, new file, add) instead of only saying they are empty.
- Switches and buttons show a focus ring when you move with the keyboard.
- Every close button looks the same; confirmations that remove or discard something have a red button and a clear verb.
- Right-click an attachment for the same menu as its … button; right-click a queued task to edit it.
- Help texts across Settings, Services and Build are one short sentence each.
- Icon buttons use three sizes everywhere.
- Every message you send is saved on your machine before it is filed, with each image's notes beside the image. A message with an image is always filed as a task.
- Clauracle asks before it clicks or types in its own window for you.
- Clauracle no longer addresses you by name, and reports work as done only after running a check.
- The "Dense AI comments" option is gone: comments are always short, plain sentences.
- Image editor: the selection-type buttons (rectangle, freehand) show their icons; the colour swap arrow now points left.
- Sent chat messages appear instantly again, also while the Continue bar is up.
- Code tab: "Open in file viewer" moved to the top right of the file header; X next to the file name closes it (#458)

- Collapsed side rail: file icons that slide behind the account button are hidden and can't be clicked through it.
- Image editor: selecting a shape shows its tool's settings (text gets bold/italic/underline/font); select by shape, box or freehand, click through stacked shapes; dashed pen/line styles; the context bar shows the tool's icon; Fill toggle with a solid-shape icon; swap/reset colour buttons on the swatches.
- Every dropdown now opens the app's own menu instead of the square system popup (incl. Settings search); a dropdown opening upward shows its arrow pointing up.
- Edit view menu bar: hovering switches between open menus, a "Show menu" toggle (Alt+M), icons on every entry.

- Fewer AI tokens: the release checklist writes LICENSE.md (official text from GitHub) and CREDITS.md itself, and a task's PDF report is built without AI. Only README writing still uses Claude.

- Library license check: Settings › Projects › License and the release checklist list every shipped Rust/npm library with its license, flag ones your license doesn't allow or that have no known license, and add uncredited libraries to CREDITS.md in one click. The license file is now LICENSE.md.
- Notifications: Snooze 24h or Snooze ∞ in the bell card silences popups, desktop and phone notifications (they still land in the list); Unsnooze turns them back on.
- Right-click on a misspelled word in a text field now offers spelling suggestions (Windows).
- Clauracle is now licensed under GPL-3.0 (was MIT); CREDITS.md lists who made it and the libraries, font and icons it ships.
- Smart build and builds after a task now do a fast local debug build (no installers). Turn on "Release builds after tasks" in Settings › Projects › Build for the full build, or set your own Local build command. Releases and deploys still use the full build.
- Edit view: a third option next to Chat and Terminal. Files open as tabs you can edit and save (Ctrl+S), with a resizable panel below holding the Terminal view and a real Shell in the project folder.
- Notification center: a bell bottom right keeps the history of notifications, with the current time next to it.
- Start menu: click your account (bottom left) for one menu with search (files, secret names, tasks, settings), every tab as a tile, and Settings / Restart / Shut down.
- Appearance: new Font setting (Sans-serif, Serif, Monospace), saved with presets. Classic Claude uses Serif, The Matrix uses Monospace.
- Type `/model` to pick from every model your Claude Code offers (including specific versions), then its effort level, with mouse or keyboard. The account menu uses the same live list.
- Composer: attachment references are plain @[name] tags; removing an attachment also removes its tags; typing @ lists project files to attach and reference.
- Staged releases: a finished build for every picked platform is kept as a staged release (its files and commit are saved, so later edits don't spoil it) and the version moves up for the next build. They're listed under Build & Release › Release, each with Skip (deletes its files) and Release. Only the lowest version can be released.
- Pick the project's licence in Settings › Projects › Licence. The release checklist says "Choose" (or "Confirm" when a LICENSE file is already there) and jumps straight to it.
- `/build windows|linux|macos|ios|android|all` builds one target (plain `/build` = a local copy for this computer). The Builds card shows the files being built and the last 3 output lines, jumps to the full log, and has a quick "Build local copy" button.
- Settings: the Projects manager and Accounts page are tidier: a 1-2-3 setup guide until a project or your accounts are set up, and sections that fold into a one-line summary once filled in.
- Clauracle now speaks as itself: buttons and tips say "Clauracle" instead of "AI", and replies address you by name.
- Services: when everything is set up but no sign-in/payments/email-type service is chosen, the card offers to add one (that unlocks "Check the app uses …").
- Right-click anywhere now opens Clauracle's own menu: Cut/Copy/Paste/Select all in text boxes, Open/Copy link, Copy image, Copy selection.
- Composer: the caret no longer drifts away from the text in long prompts.
- Image editor: the cursor shows the selected tool; a click with the pen draws a dot; a click with Box + note places just the numbered point; a new width menu sets line thickness and arrowheads on lines.
- File viewer: zoom with the wheel, Ctrl + / − / 0 or the header slider; pan images with the middle mouse button.
- Code tab: syntax colouring, recently worked-on files when nothing is open, and the file path opens the file in the File viewer.
- Agents tab lists your agents (faded when off) with the on/off switch and settings at the top.
- Chat: sent/seen ticks sit next to the message time; icons replace text symbols (also in the Build menu). No more black focus rings or doubled system tooltips.
- One built-in File viewer: PDFs (reports, PDFs in Code) and full terminal output open in the same window as the image editor, with Save as… and contextual tools on the left.
- Writing a report shows a pill in the chat, which turns into "Report ready!" with a button that opens the PDF.
- Services & Secrets: "Check the app uses …" asks Claude to find chosen features (sign-in, payments…) the app does not use yet and files a task for each, waiting on your input.
- Task report: the detailed-report button uses the sparkle icon, and the timeline starts at the time work actually began.
- The "Building…" line in the chat now looks like the Thinking pill, with a hammer icon.
- The chat log keeps everything it showed, task reports included, across a fresh session and an app restart; only the history limit drops the oldest greyed-out messages.
- Release checks now require an app icon source (SVG, or PNG of at least 1024 px) from Beta up; the Build row lists one file per line by platform and links the last release; a finished task shows its "Done! See report" note again. Image editor: round colour picker, box numbers never show "undefined"; image thumbnails show Refer bottom-right and the note as a small dot that grows on hover.
- Multi-line text boxes grow with their text instead of showing a resize corner; issues filed with screenshots no longer put your local folder path on GitHub.
- The Working pill is now the last centred line of the chat and scrolls with it, instead of staying pinned to the bottom.
- File pills in the message box and in chat show a file-type icon and more side padding; the "Then" build picker updates the moment the setting changes.
- Tasks: an ask filed as an issue shows in the task list at once (was ~30s late), and the list no longer says "Working on" an old task at the start of a new turn.
- Chat: pasted-image refs in the composer are real pills; the Working pill floats centred above the composer; the live reply avatar + timer stick to the top while you scroll.
- Services › Google Play: **Make service account** creates it in your Cloud project, fills `PLAY_SERVICE_ACCOUNT_JSON` and copies its email for the Play Console invite.
- Services › Stripe: **Make webhook** adds the endpoint on your website address and fills `STRIPE_WEBHOOK_SECRET`.
- Website hosts over SSH (cPanel, DirectAdmin, Plesk, VPS): **Make deploy key** creates the key and copies its public key for the panel; on a plain server **Install key** adds it with your password once (not saved).
- The Graph side tab is now **Hot**: a map of your files sized by size and coloured by how much they changed in the last 30 days; click one to see its recent commits and the tasks behind them.
- Tasks list: click a task to edit it, labels as coloured dots, the editor opens instantly from cache; tasks the AI starts are claimed on GitHub so teammates skip them.
- Pasted images drop a clickable [ref] into your message; image notes live on the image (notepad icon) instead of in the text; the image editor now has movable, resizable, erasable boxes and lines, a smoother pen and multi-line notes that fold away.
- After a task: "Done! Worked for Xh Ym" with a See report window (what was done, what failed with fix buttons, a timeline), plus meeting-ready PDF reports for the task or the last 7 days.
- Settings › Extensions: find and install new skills (skills.sh) and MCP connections (MCP registry) right there; new installs are switched on for the open project.
- Task list: the queue is now called "Next" with the "Then" step right below it; row actions (Edit, Start, drag) sit in a soft fade panel, and drag-to-reorder works again.
- Image attachments in the message box show as clean thumbnails at their real shape, squeeze to fit, and scroll with arrows when there are many.
- Settings › Appearance › Updates: choose how often Clauracle checks for a new version (never, daily, weekly, monthly), or check now.
- When a command the AI runs fails because a tool, package or crate feature is missing, Clauracle spots it and offers a one-click Install (npm, pip, cargo features, Node/Git/Python/Rust and more). No AI tokens used.
- Website setup does more by itself: GitHub Pages turns on with one click, Cloudflare Pages reads your Account ID from the token, and Netlify makes the site and fills its Site ID.
- New Graph tab next to Files: a spinning 3D cloud of how the project's files link (imports, includes, modules). Hubs sit in the middle and unlinked files float at the edge. Click a dot to open the file.
- On Windows, the Linux version can be built on your own computer through WSL (Ubuntu), no GitHub CI needed.
- Build & Release shows GitHub Actions minutes used this month, the included amount, any billed spend and a clear "limit reached" note, with links to the usage and spending-limit pages.
- The "All tasks done" buttons are faded and can't be clicked while the AI is still working.
- Press Tab in the chat input to switch permission mode (Shift+Tab goes back); the status bar names the mode when there is room.
- Right-click a side-panel tab for Refresh and a shortcut to its settings.
- GitHub tab: Collaborators section lists who has access (and pending invites) and invites people by GitHub username.
- Issues list: hover a task for a pencil button to edit its title, description, labels and assignees.
- Your message shows in the chat right away, with a gray ✓ once sent and a blue ✓✓ when the AI has picked it up (a red ! and a notice if it did not go through).
- Continue ribbon: lifts a little on hover; it only expands when the next task is cut off (shown with …), so it no longer jumps.
- Questions are answered, not filed: a plain question ("…?" or "btw …") gets a short answer plus proposed tasks as cards with Accept / Dismiss and an optional note; Confirm files the accepted ones as issues.
- Smart builds: new projects build and relaunch themselves after a task when the change is big or a fix (doc-only and tiny edits skip it); build steps are guessed when unset, and failures go back to the AI. Pick "Smart" under Build → After each task on existing projects.
- Continue ribbon: when a task finishes it stays up longer (9 s) and shimmers twice so it is hard to miss.

- Question cards no longer reset or lose your typing while the AI keeps working.
- Settings has fewer tabs: GitHub accounts now sit on the Accounts page, helper agents under Extensions.
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
