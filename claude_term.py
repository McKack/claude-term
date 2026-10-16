"""claude-term: a themeable Textual wrapper around Claude Code, via claude-agent-sdk.

Usage: python claude_term.py [project-dir]
"""
import asyncio
import difflib
import itertools
import json
import os
import random
import shutil
import subprocess
import sys
import time
from typing import Annotated

from rich.markup import escape
from rich.text import Text
from textual import work
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import Collapsible, Markdown, OptionList, Static
from textual.widgets.option_list import Option

from claude_agent_sdk import (
    AssistantMessage, ClaudeAgentOptions, ClaudeSDKClient, PermissionResultAllow,
    PermissionResultDeny, RateLimitEvent, ResultMessage, StreamEvent, SystemMessage, TextBlock, ThinkingBlock,
    ToolResultBlock, ToolUseBlock, UserMessage, create_sdk_mcp_server, get_session_messages,
    list_sessions, query, tool,
)

import attach
import store
from ui import (THEMES, TIPS, AccountManager, Ask, AskText, Chip, Choose, Confirm, Form, InstallCard,
                PopupMenu, Prompt, ResumePanel, TaskPanel, ThemePicker, Welcome)

MODELS = {"": "Default", "claude-opus-5-5": "Opus 5.5", "claude-sonnet-5-5": "Sonnet 5.5",
          "claude-haiku-5-5": "Haiku 5.5", "claude-fable-5-1": "Fable 5.1"}
EFFORTS = {"": "Auto", "low": "Low", "medium": "Medium", "high": "High", "xhigh": "Extra high", "max": "Max"}
MODES = {"default": "Ask first", "acceptEdits": "Accept edits", "auto": "Auto", "plan": "Plan",
         "bypassPermissions": "Bypass permissions"}
MODE_CYCLE = ["default", "acceptEdits", "auto", "plan"]  # shift+tab; bypass only via the menu
SCOUT_MODEL = "claude-haiku-5-5"
MENUBAR = ("claude", "view", "help")  # menus() also holds submenus, which have no title on the bar
LOCAL_COMMANDS = {"help": "this list", "theme": "/theme [classic|matrix|amber]", "new": "fresh session",
                  "clear": "fresh session (like the CLI)", "resume": "recent sessions",
                  "account": "/account [name]", "tasks": "rescan plan files", "ask": "Ask Claude tab",
                  "code": "Code tab", "clear-continue": "toggle the hands-off loop",
                  "advisor": "advisor review now"}
SUGGESTIONS = [
    ("↺  Resume last session", "resume_last"),
    ("Summarize this project", "Give me a short overview of this project: what it is, how it's laid out, how to run it."),
    ("Review uncommitted changes", "Review my uncommitted changes and point out bugs or risky bits."),
    ("What's left to do here?", "Look through this project for TODOs, open threads and obvious next steps."),
]
SHORTCUTS = ("enter send · ctrl+j new line · ctrl+v paste text/images/files · esc interrupt/close\n"
             "shift+tab permission mode · f2 Code ↔ Ask Claude · ctrl+n new session · ctrl+r recent sessions\n"
             "ctrl+o change folder · ctrl+l clear screen · ctrl+t theme · ctrl+b side panel · alt+c/v/h or f10 menus")
SPINNER = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"
SYSTEM_APPEND = ("You are running inside claude-term, a custom terminal UI. Its mcp__ui__* tools let you "
                 "show markdown in a side panel (plans, summaries, tables), pop notifications, and change "
                 "the theme. Use the panel for content worth keeping on screen. For a real "
                 "decision, or when you see a better/leaner way than what was asked, use mcp__ui__ask_user: a dialog "
                 "with radio/checkbox/select/text/switch fields and clickable links, your recommendation pre-selected.")
ASK_APPEND = ("This is the 'Ask Claude' side chat in claude-term: quick questions, explanations, brainstorming. "
              "You can read and search the project and the web, but never change anything. Keep answers short.")
SCOUT_PROMPT = ("Find this project's planning/handover docs (HANDOVER.md, PLAN.md, TODO.md, ROADMAP.md, TASKS.md, "
                "NEXT*.md, plan/todo sections in CLAUDE.md, docs/ or .claude/ plan files). Use Glob, then Read. "
                "Extract up to 25 concrete, actionable tasks, most important first. done=true for checked or "
                "finished items. title <= 80 chars; detail = 1-2 sentences of context; source = relative file "
                "path. Nothing found: empty list.")
SCOUT_SCHEMA = {"type": "object", "required": ["tasks"], "properties": {"tasks": {"type": "array", "items": {
    "type": "object", "required": ["title", "source", "done"], "properties": {
        "title": {"type": "string"}, "detail": {"type": "string"}, "source": {"type": "string"},
        "done": {"type": "boolean"}}}}}}
LOOP_LIMIT = 80  # % of the 5-hour or weekly window; at or past it the loop stops and asks
LOOP_RULES = """continue

# clear-continue is ON (the user's hands-off loop)
The user is away. Work ONE task per session, keep a handover a fresh session can resume from, and hand over.
1. This message (sent automatically in a fresh session) means: find the project's handover and do its next task.
   If the project's CLAUDE.md / CLAUDE.local.md define a continue procedure or a task queue, follow that. Otherwise
   read HANDOVER.md at the project root and take the FIRST open task of its queue ("## Queue", "## Open tasks" or
   similar). Do only that task.
2. No handover yet? Create HANDOVER.md at the project root from what you know: goal, current state, a "## Queue" of
   ordered open tasks (first = next), how to build and test, rules and decisions learned. If the project is a git repo
   and the file is not tracked, add it to .git/info/exclude rather than committing it.
3. Finishing a task: verify it, update the handover (task ticked off, what was verified and what was not, the next
   task first in the queue), add an entry to CHANGES.md (below), commit if the project's rules allow it, then call
   mcp__ui__clear_and_continue as the LAST action of the turn.
4. Do NOT call it, stop and say why instead, when: the queue is empty, the next step is a decision only the user can
   make, something failed that you cannot fix, or the tool answered "NOT cleared".
5. CHANGES.md at the project root is the user's log of what the loop did. Newest entry FIRST (under the "# Changes"
   heading; create the file with that heading if missing, and add it to .git/info/exclude like the handover). One entry
   per task, also for a task that stopped short. No fluff, no prose paragraphs:
   ## <Task title> - <YYYY-MM-DD>
   - <one plain line per change, feature or fix; what was verified and what was NOT; anything the user must do>"""
ASK_USER_SCHEMA = {"type": "object", "required": ["title", "question", "fields"], "properties": {
    "title": {"type": "string", "description": "a few words, shown as the dialog title"},
    "question": {"type": "string", "description": "markdown: the decision, trade-offs, your reasoning. Put example "
                 "images, docs and repos in as links (https:// or file:///), the user can click them"},
    "fields": {"type": "array", "items": {"type": "object", "required": ["id", "type", "label"], "properties": {
        "id": {"type": "string"}, "label": {"type": "string"},
        "type": {"type": "string", "enum": ["radio", "checkbox", "select", "text", "textarea", "switch"]},
        "options": {"type": "array", "items": {"type": "object", "required": ["value"], "properties": {
            "value": {"type": "string"}, "label": {"type": "string"}, "description": {"type": "string"}}}},
        "recommended": {"description": "your suggestion: an option value (a list for checkbox, a bool for switch, "
                        "text for text). It is starred and pre-selected, so the user can just confirm"},
        "default": {"description": "pre-selected value when it should differ from the recommendation"},
        "help": {"type": "string"}, "placeholder": {"type": "string"}}}}}}
ADVISOR_SYSTEM = (
    "You are the project advisor in claude-term: a senior engineer looking over the shoulder of a Claude Code "
    "session. You never change anything. You look for what the coding session misses: scope creep away from the "
    "project's goal, duplicated code where a component could be reused, performance and size problems, heavy or "
    "outdated dependencies with a modern lightweight (or stdlib/native) alternative, missing tests for risky logic, "
    "and skills, MCP servers or commands that would help. Check current facts on the web when a library is involved. "
    "Only raise things worth the user's attention; an empty list is a fine answer.")
ADVISOR_PROMPT = (
    "Review where this project is heading. Read the project's goal and state (CLAUDE.md, HANDOVER.md, README) and the "
    "files the recent conversation touched. Return at most 4 suggestions, most valuable first. title <= 70 chars; "
    "why = 1-3 sentences with the evidence; kind = scope|reuse|performance|library|tests|skill|other; prompt = the "
    "exact instruction to give the coding session if the user approves; recommended = true when you'd do it now; "
    "links = docs/repos backing it up.")
ADVISOR_SCHEMA = {"type": "object", "required": ["suggestions"], "properties": {"suggestions": {"type": "array", "items": {
    "type": "object", "required": ["title", "why", "kind", "prompt", "recommended"], "properties": {
        "title": {"type": "string"}, "why": {"type": "string"}, "kind": {"type": "string"},
        "prompt": {"type": "string"}, "recommended": {"type": "boolean"},
        "links": {"type": "array", "items": {"type": "string"}}}}}}}
HANDOVER_PROMPT = (
    "Update this project's HANDOVER.md from the conversation below, so a fresh session can pick up the work: tick off "
    "what was finished (say what was verified and what wasn't), add new open tasks it surfaced, keep the next task "
    "first. Keep the file's structure, headings and style; change only what the conversation justifies; no fluff. "
    "No HANDOVER.md yet: write one with the goal, current state, an ordered '## Queue', how to build and test. "
    "Read files only to check facts. Return the complete new file as markdown.")
HANDOVER_SCHEMA = {"type": "object", "required": ["markdown"], "properties": {"markdown": {"type": "string"}}}
COMMIT_PROMPT = (
    "Write a git commit message for the diff below. Match the style of the recent log (if any): a subject line of at "
    "most 72 chars in the imperative mood, then a blank line and a short body only if the why isn't obvious. "
    "No fluff, no markdown headings.")
COMMIT_SCHEMA = {"type": "object", "required": ["message"], "properties": {"message": {"type": "string"}}}
ADVISOR_MODES = {"off": "Off", "suggest": "Suggest: ask me first", "act": "Act: send recommended ones to Claude"}
NO_HISTORY = {"no-session-persistence": None}  # helper chats stay out of the resume list
RECONNECT = object()


def clip(s, n=2000):
    s = s if isinstance(s, str) else json.dumps(s, indent=1, ensure_ascii=False)
    return s if len(s) <= n else s[:n] + f"\n… ({len(s) - n} more chars)"


def diff_of(tool_name, a, theme, live=True):
    """Colored unified diff for Edit/Write inputs, else None. Returns (Text, added, removed)."""
    path = a.get("file_path", "")
    if tool_name == "Edit":
        old, new = a.get("old_string", ""), a.get("new_string", "")
    elif tool_name == "Write":  # live: diff against the file on disk; replayed: it has changed since
        old = open(path, encoding="utf-8", errors="replace").read() if live and os.path.isfile(path) else ""
        new = a.get("content", "")
    else:
        return None
    lines = list(difflib.unified_diff(old.splitlines(), new.splitlines(), lineterm="", n=2))[2:]
    out, plus, minus = Text(f"{path}\n", style="bold"), 0, 0
    for line in lines[:500]:  # ponytail: huge diffs cut at 500 lines
        style = (theme.success if line[:1] == "+" else theme.error if line[:1] == "-"
                 else theme.secondary if line.startswith("@@") else "")
        plus, minus = plus + (line[:1] == "+"), minus + (line[:1] == "-")
        out.append(line + "\n", style)
    return out, plus, minus


def find_claude():
    """The installed CLI: on PATH, or where the native installer puts it (PATH may not be refreshed yet)."""
    local = os.path.join(os.path.expanduser("~"), ".local", "bin", "claude.exe")
    return shutil.which("claude") or (local if os.path.isfile(local) else None)


async def one_message(blocks):
    yield {"type": "user", "message": {"role": "user", "content": blocks}, "parent_tool_use_id": None,
           "session_id": "default"}


class Lane:
    """One conversation with its own claude process: the Code session, or the Ask Claude side chat."""

    def __init__(self, view):
        self.view, self.inbox, self.client = view, asyncio.Queue(), None
        self.busy, self.stream, self.cost, self.started = False, None, 0.0, False
        self.account = None  # the account this lane's claude process actually runs on


class ClaudeTerm(App):
    TITLE = "CLAUDE://TERM"
    ENABLE_COMMAND_PALETTE = False  # hides Textual's own theme switcher
    BINDINGS = [
        Binding("escape", "escape", show=False),
        Binding("shift+tab", "cycle_mode", priority=True, show=False),
        Binding("f2", "do('toggle_tab')", show=False),
        Binding("ctrl+n", "do('new')", show=False), Binding("ctrl+r", "do('resume')", show=False),
        Binding("ctrl+o", "do('folder')", show=False), Binding("ctrl+l", "do('clear')", show=False),
        Binding("ctrl+t", "do('theme')", show=False), Binding("ctrl+b", "do('side')", show=False),
        Binding("alt+c,f10", "menu('claude')", show=False), Binding("alt+v", "menu('view')", show=False),
        Binding("alt+h", "menu('help')", show=False),
    ]
    CSS = """
    Screen { background: $background; color: $foreground; }
    #window { background: $background; }

    #menubar { height: 1; background: $panel; }
    #menubar Chip { width: auto; padding: 0 1; }
    #menubar Chip:hover, #menubar Chip.-open { background: $block-cursor-background; color: $block-cursor-foreground; }
    #menu-info { width: 1fr; text-align: right; color: $secondary; padding: 0 1; }

    #main { height: 1fr; }
    #resume { width: 44; display: none; border: round $panel; margin: 0 0 1 1; padding: 0 1; background: $surface; }
    #resume.-on { display: block; }
    #resume-head { height: 1; }
    #resume-title { width: 1fr; color: $primary; text-style: bold; padding: 0 0 0 1; }
    #resume-close, #side-close { width: 3; color: $secondary; }
    #resume-close:hover, #side-close:hover { color: $primary; }
    .rail { width: 5; height: 1fr; border: round $panel; background: $surface; color: $secondary;
            text-align: center; }
    .rail:hover { border: round $primary; color: $primary; }
    #rail-resume { margin: 0 0 1 1; }
    #rail-side { margin: 0 1 1 0; }
    #resume-filter { border: round $panel; background: $surface; }
    #resume-list { height: 1fr; background: $surface; border: none; }
    #center { width: 1fr; }
    #chat, #ask { width: 100%; height: 1fr; margin: 0 2; padding: 0 1; border: round $panel;
                 scrollbar-color: $panel; scrollbar-background: $background; }
    #ask { display: none; }
    #side { width: 44; display: none; margin: 0 1 1 0; }
    #side.-on { display: block; }
    TaskPanel { height: 1fr; border: round $panel; padding: 0 1; background: $surface; }
    #tasks-head { height: 1; }
    #tasks-title { width: auto; color: $primary; text-style: bold; }
    #tasks-status { width: 1fr; color: $secondary; padding-left: 1; }
    #tasks-refresh { width: 3; color: $secondary; }
    #tasks-refresh:hover { color: $primary; }
    #tasks-list { height: 1fr; background: $surface; scrollbar-color: $panel; scrollbar-background: $surface; }
    .task-head { color: $secondary; text-style: bold; margin-top: 1; }
    .task { height: auto; }
    .task:hover { background: $panel; }
    .task.-in_progress { color: $accent; text-style: bold; }
    .task.-completed { color: $secondary; text-style: strike; }
    .task-empty { color: $secondary; margin-top: 1; }
    #panel { height: 1fr; border: round $panel; border-title-color: $accent; background: $surface; }

    #slash { height: auto; max-height: 10; display: none; margin: 0 2; border: round $panel; background: $surface; }
    #slash.-on { display: block; }
    #tabs { height: 1; margin: 0 3; }
    .tab { width: auto; padding: 0 2; color: $secondary; }
    .tab:hover { color: $primary; }
    .tab.-active { background: $surface; color: $primary; text-style: bold; }
    #composer { height: auto; margin: 0 2 1 2; border: round $panel; background: $surface; padding: 0 1; }
    #composer:focus-within { border: round $primary; }
    #attachments { height: auto; display: none; }
    #attachments.-on { display: block; }
    .att { width: auto; background: $panel; padding: 0 1; margin: 0 1 0 0; }
    .att:hover { color: $error; }
    #prompt { height: auto; max-height: 50vh; min-height: 2; background: $surface; border: none; padding: 0; }
    #prompt .text-area--placeholder { color: $secondary; }
    #composer-bar { height: 1; }
    #attach { width: auto; color: $secondary; margin-right: 2; }
    #state { width: auto; color: $secondary; }
    #mode { width: auto; margin-left: 2; color: $secondary; }
    #spacer { width: 1fr; }
    #picker { width: auto; }
    #attach:hover, #mode:hover, #picker:hover { color: $primary; text-style: bold; }

    #welcome { height: auto; align-horizontal: center; padding: 2 2 1 2; }
    #welcome > Static { width: 100%; text-align: center; }
    #welcome Center { height: auto; }
    #logo { color: $primary; }
    #hello { text-style: bold; margin-top: 1; }
    #facts { color: $secondary; }
    #chips { grid-size: 2; grid-gutter: 0 1; height: auto; width: 92; max-width: 100%; margin: 1 0; }
    .suggest { border: round $panel; height: 3; width: 100%; text-align: center; background: $background; }
    .explain { width: auto; color: $error; padding: 0 1; margin: 0 0 0 2; }
    .explain:hover { background: $panel; }
    .suggest:hover { border: round $primary; color: $primary; }
    #tip { height: auto; width: 92; max-width: 100%; background: $panel; padding: 0 2 1 2; margin-top: 1; }
    #tip-next { dock: right; width: 3; color: $secondary; padding-top: 1; }
    #tip-next:hover { color: $primary; }
    #tip-title { text-style: bold; text-align: center; color: $accent; padding-top: 1; }
    #tip-text { text-align: center; }
    .ask-intro { margin: 2 4; padding: 1 2; background: $panel; color: $secondary; text-align: center; }

    .user { margin: 1 0 0 0; padding: 0 1; border-left: outer $primary; background: $surface; }
    .claude { margin: 1 0 0 0; padding: 0; background: $background; }
    .note { color: $secondary; text-style: italic; background: $background; }
    .err { color: $error; background: $background; }
    Collapsible { border: none; padding: 0; margin: 0; background: $surface; color: $secondary; }
    CollapsibleTitle { color: $secondary; }

    PopupMenu { background: transparent; }
    #menu { height: auto; max-height: 32; border: round $panel; background: $surface; padding: 0; }
    .dialog { align: center middle; }
    #dlg { width: 80%; max-width: 100; height: auto; max-height: 85%; border: round $primary;
           border-title-color: $accent; background: $surface; padding: 1 2; }
    #dlg-body { max-height: 24; overflow-y: auto; color: $secondary; margin: 1 0; }
    #dlg Input { margin-top: 1; }
    #pick { height: auto; max-height: 12; margin: 1 0; background: $panel; }
    #dlg-btns { height: auto; align-horizontal: right; margin-top: 1; }
    #form-q { background: $surface; margin: 0; padding: 0; }
    .form RadioSet { width: 100%; background: $panel; border: none; }
    .form Checkbox { background: $surface; border: none; }
    .form Select { width: 100%; }
    .form-text { height: auto; min-height: 4; max-height: 14; background: $panel; border: none; }
    #dlg-btns Button { margin-left: 2; }
    #dlg-btns Button.-success { color: $background; text-style: bold; }
    .hint { color: $secondary; }
    .section { color: $primary; text-style: bold; margin-top: 1; }
    .row { height: auto; }
    .row Select { width: 26; }
    #dlg #auto-at { width: 6; margin: 1 1 0 1; background: $panel; }
    .row .hint { padding-top: 1; }
    #acct-rows { height: auto; }
    .acct-row { height: 1; margin-top: 1; }
    .acct-rank { width: 4; color: $secondary; }
    #dlg .acct-nick { width: 22; margin: 0; background: $panel; }
    .acct-usage { width: 1fr; color: $secondary; padding-left: 2; }
    .acct-btn { width: 3; text-align: center; }
    .acct-btn:hover { color: $primary; text-style: bold; }
    #acct-add { margin-top: 1; }
    """

    def __init__(self, cwd, resume=None):
        super().__init__()
        self.resume_at, self.restart_cmd, self.started_at = resume, None, time.time()
        self.cfg = store.load()
        self.cwd, self.session_id, self.claude = cwd, None, None
        self.code, self.ask = Lane("#chat"), Lane("#ask")
        self.tab, self.tools, self.tasks, self.attachments = "code", {}, {}, []
        self.side_closed = False  # the person collapsed the side panel: it no longer opens by itself
        self.live_model, self.last_reply, self.term_only, self.slash, self.frame = "", "", set(), [], 0
        self.armed = False  # clear-continue: Claude finished a task; clear + continue when this turn ends
        self.turns = 0  # advisor: Code turns since start
        self.worked = False  # a Code turn finished since the last handover / new session
        self.session = self.fresh_session(store.pick_account(self.cfg) or self.cfg["accounts"][0]["id"])

    def fresh_session(self, account):
        """Per-session settings: the defaults from the account manager, overridden from the chat menu."""
        return {"account": account, **self.cfg["defaults"]}

    @property
    def lane(self):
        return self.ask if self.tab == "ask" else self.code

    def compose(self) -> ComposeResult:
        with Vertical(id="window"):
            with Horizontal(id="menubar"):
                for name in MENUBAR:
                    yield Chip(name.title(), id=f"m-{name}")
                yield Static(id="menu-info")
            with Horizontal(id="main"):
                yield Chip(id="rail-resume", classes="rail")
                yield ResumePanel(id="resume")
                with Vertical(id="center"):
                    yield VerticalScroll(id="chat")
                    yield VerticalScroll(id="ask")
                    yield OptionList(id="slash")
                    with Horizontal(id="tabs"):
                        yield Chip("Code", id="tab-code", classes="tab -active")
                        yield Chip("Ask Claude", id="tab-ask", classes="tab")
                    with Vertical(id="composer"):
                        yield Horizontal(id="attachments")
                        yield Prompt(id="prompt", soft_wrap=True, show_line_numbers=False, compact=True,
                                     tab_behavior="focus", placeholder="Ask Claude anything…  (/ for commands)")
                        with Horizontal(id="composer-bar"):
                            yield Chip("+ Attach", id="attach")
                            yield Static(id="state")
                            yield Chip(id="mode")
                            yield Static(id="spacer")
                            yield Chip(id="picker")
                with Vertical(id="side"):
                    yield TaskPanel()
                    yield Markdown(id="panel")
                yield Chip(id="rail-side", classes="rail")

    def on_mount(self):
        for t in THEMES.values():
            self.register_theme(t)
        self.apply_theme(self.cfg["theme"], save=False)
        self.query_one("#panel").display = False
        self.query_one("#prompt").focus()
        self.set_commands([])
        self.set_interval(0.1, self.spin)
        self.set_interval(1, self.tick_tasks)
        self.render_tasks()
        self.claude = find_claude()
        ui = store.load_project(self.cwd, "ui") or {}  # where the last run left off (closed or crashed)
        self.resume_at = self.resume_at or ui.get("session")
        if self.claude and self.resume_at:  # restarted: straight back into the session
            self.run_worker(self.resume_session(self.resume_at), exit_on_error=False)
            self.agent_loop()
            self.load_plan_tasks(auto=True)
        elif self.claude:
            self.show_welcome()
            self.agent_loop()
            self.load_plan_tasks(auto=True)
        else:  # first run: offer to install the CLI
            self.query_one("#chat").mount(InstallCard(id="welcome"))
        self.restore_ui(ui)
        self.sync_rails(sessions=True)
        self.set_interval(2, self.save_ui)
        self.refresh_bars()

    # ---- ui state survives closing and crashes: draft, attachments, tab, panels, session ----

    def ui_state(self):
        return {"session": self.session_id, "tab": self.tab, "draft": self.query_one("#prompt", Prompt).text,
                "attachments": self.attachments, "side": self.query_one("#side").has_class("-on"),
                "resume": self.query_one("#resume").has_class("-on")}

    def save_ui(self):
        # ponytail: saved every 2s when changed, so a crash loses at most the last 2s of typing
        try:
            state = self.ui_state()
        except Exception:  # widgets already gone while shutting down
            return
        if state != getattr(self, "saved_ui", None):
            self.saved_ui = state
            store.save_project(self.cwd, state, "ui")

    def on_unmount(self):
        self.save_ui()

    def restore_ui(self, ui):
        if not ui:
            return
        prompt = self.query_one("#prompt", Prompt)
        prompt.text = ui.get("draft", "")
        prompt.move_cursor(prompt.document.end)
        self.add_attachments([p for p in ui.get("attachments", []) if os.path.isfile(p)])
        self.side_closed = ui.get("side") is False
        self.query_one("#side").set_class(ui.get("side", False), "-on")
        ui.get("resume") and self.do("resume")
        self.switch_tab(ui.get("tab", "code"))
        self.query_one("#prompt").focus()

    # ---- the two conversations ------------------------------------------------

    def options(self):
        c, s = self.cfg, self.session
        self.code.account = s["account"]
        return ClaudeAgentOptions(
            cwd=self.cwd, cli_path=self.claude,  # your installed claude: same commands, skills, MCP
            env=store.account_env(s["account"]),
            system_prompt={"type": "preset", "preset": "claude_code", "append": SYSTEM_APPEND},
            model=s["model"] or None, effort=s["effort"] or None, permission_mode=c["mode"],
            disallowed_tools=[] if s["web"] else ["WebSearch", "WebFetch"], include_partial_messages=True,
            mcp_servers={"ui": self.ui_server()}, can_use_tool=self.can_use_tool, resume=self.session_id)

    def ask_options(self):
        c, s = self.cfg, self.session
        self.ask.account = s["account"]
        tools = ["Read", "Glob", "Grep"] + (["WebSearch", "WebFetch"] if s["web"] else [])
        return ClaudeAgentOptions(
            cwd=self.cwd, cli_path=self.claude, env=store.account_env(s["account"]),
            system_prompt={"type": "preset", "preset": "claude_code", "append": ASK_APPEND},
            model=c["ask_model"] or None, tools=tools, allowed_tools=tools, permission_mode="dontAsk",
            strict_mcp_config=True, include_partial_messages=True, extra_args=NO_HISTORY)

    @work(exclusive=True, group="agent", exit_on_error=False)
    async def agent_loop(self):
        await self.run_lane(self.code, self.options, self.show)

    @work(exclusive=True, group="ask", exit_on_error=False)
    async def ask_loop(self):
        await self.run_lane(self.ask, self.ask_options, self.show_ask)

    async def run_lane(self, lane, make_options, on_msg):
        # ponytail: one task owns each client (the SDK dislikes cross-task connect/disconnect)
        while True:
            try:
                async with ClaudeSDKClient(make_options()) as client:
                    lane.client = client
                    if lane is self.code:  # picked while connecting
                        self.session["model"] and await client.set_model(self.session["model"])
                        await client.set_permission_mode(self.cfg["mode"])
                    while (item := await lane.inbox.get()) is not RECONNECT:
                        lane.busy = True
                        await client.query(item if isinstance(item, str) else one_message(item))
                        async for msg in client.receive_response():
                            await on_msg(msg)
                        await self.end_stream(lane)
                        lane.busy = False
                        self.refresh_bars()
                        if lane is self.code and self.armed:
                            self.clear_continue()
                        elif lane is self.code:
                            self.worked = True  # something to hand over
                            self.sync_rails(sessions=True)
                            self.count_turn()
            except Exception as e:  # keep the UI alive and reconnect
                lane.busy, lane.client = False, None
                self.say("err", f"claude error: {e!r} (retrying in 5s)", lane)
                self.refresh_bars()
                await asyncio.sleep(5)

    def reconnect(self, note=None):
        note and self.say("note", note + (" (after the current reply)" if self.code.busy else ""))
        self.code.inbox.put_nowait(RECONNECT)
        if self.ask.started:
            self.ask.inbox.put_nowait(RECONNECT)

    def safely(self, coro, what):
        """Run a client call without letting an error take the whole app down."""
        async def run():
            try:
                await coro
            except Exception as e:
                self.say("err", f"{what} failed: {e}")
        self.run_worker(run(), exit_on_error=False)

    async def modal(self, screen):
        fut = asyncio.get_running_loop().create_future()
        self.push_screen(screen, fut.set_result)
        return await fut

    async def can_use_tool(self, tool_name, args, ctx):
        if tool_name.startswith("mcp__ui__"):  # our own UI tools are harmless
            return PermissionResultAllow()
        if tool_name == "AskUserQuestion":
            answers = {}
            for q in args.get("questions", []):
                if (answer := await self.modal(Choose(q))) is None:
                    return PermissionResultDeny(message="The user dismissed the question.")
                answers[q["question"]] = answer
            return PermissionResultAllow(updated_input={**args, "answers": answers})
        diff = diff_of(tool_name, args, self.current_theme)
        body = diff[0] if diff else Text(clip(args, 1200))
        if await self.modal(Ask(tool_name, body)):
            return PermissionResultAllow()
        return PermissionResultDeny(message="User denied this.")

    def ui_server(self):
        def ok(text="done"):
            return {"content": [{"type": "text", "text": text}]}

        @tool("show_panel", "Show markdown in the claude-term side panel. Empty markdown closes it.",
              {"title": str, "markdown": str})
        async def show_panel(a):
            panel = self.query_one("#panel", Markdown)
            panel.border_title = a["title"]
            panel.update(a["markdown"])
            panel.display = bool(a["markdown"].strip())
            self.refresh_side()
            return ok()

        @tool("notify", "Pop a toast notification in claude-term.", {"text": str})
        async def notify(a):
            self.notify(a["text"], title="claude")
            self.bell()
            return ok()

        @tool("set_theme", "Switch claude-term's theme.", {"name": Annotated[str, " | ".join(THEMES)]})
        async def set_theme(a):
            if a["name"] not in THEMES:
                return ok(f"unknown theme; choose from {list(THEMES)}")
            self.apply_theme(a["name"])
            return ok()

        @tool("clear_and_continue",
              "Only while clear-continue is on: call as the LAST action of a turn, once the task is done and the "
              "handover is updated. When the turn ends claude-term starts a fresh session and sends \"continue\". "
              f"Refused when the loop is off, or at {LOOP_LIMIT}% of the 5-hour or weekly limit: then stop and ask "
              "the user.", {})
        async def clear_and_continue(a):
            if not self.looping:
                return ok("clear-continue is OFF: nothing will be cleared. Just end the turn.")
            if (pct := self.loop_limit_hit()) is not None:
                self.set_loop(False, kickoff=False)
                return ok(f"NOT cleared: usage at {pct}% (limit {LOOP_LIMIT}%). clear-continue is now off. "
                          "Stop and ask the user whether to continue.")
            self.armed = True
            return ok("Armed. When this turn ends: fresh session, then \"continue\". End the turn now.")

        @tool("ask_user", "Ask the user a decision in a dialog: markdown question with clickable links, then "
              "radio/checkbox/select/text/switch fields. Your recommendation is starred and pre-selected so they can "
              "just confirm. Returns their answers as JSON (plus a free-text note), or that they closed it.",
              ASK_USER_SCHEMA)
        async def ask_user(a):
            answers = await self.modal(Form(a))
            return ok(json.dumps(answers) if answers is not None else "The user closed the dialog without answering.")

        self.ui_tools = {t.name: t for t in (show_panel, notify, set_theme, clear_and_continue, ask_user)}
        return create_sdk_mcp_server("ui", tools=list(self.ui_tools.values()))

    # ---- rendering agent output ----------------------------------------------

    async def show(self, msg, live=True):
        if isinstance(msg, StreamEvent):
            await self.on_stream(msg, self.code)
        elif isinstance(msg, RateLimitEvent):
            self.note_usage(msg, self.code.account)
        elif isinstance(msg, SystemMessage) and msg.subtype == "init":
            self.session_id = msg.data.get("session_id", self.session_id)
            self.live_model = msg.data.get("model", "")
            self.term_only = set(msg.data.get("terminal_slash_commands") or [])
            self.set_commands(msg.data.get("slash_commands") or [])
        elif isinstance(msg, AssistantMessage):
            for b in msg.content:
                if isinstance(b, TextBlock):
                    self.last_reply = b.text
                    if not await self.end_stream(self.code):  # streamed already, else draw it whole
                        self.add(Markdown(b.text, classes="claude"))
                elif isinstance(b, ThinkingBlock) and b.thinking:
                    self.add(Collapsible(Static(Text(b.thinking)), title="∴ thinking", collapsed=True))
                elif isinstance(b, ToolUseBlock):
                    self.show_tool(b, msg.parent_tool_use_id, live)
        elif isinstance(msg, UserMessage) and isinstance(msg.content, list):
            for b in msg.content:
                if isinstance(b, ToolResultBlock) and (hit := self.tools.pop(b.tool_use_id, None)):
                    self.show_result(b, hit, msg.tool_use_result)
        elif isinstance(msg, ResultMessage):
            self.session_id, self.code.cost = msg.session_id, self.code.cost + (msg.total_cost_usd or 0)
            if msg.is_error:
                self.armed = False  # failed turn: leave it to the user
                self.say("err", "; ".join(msg.errors or []) or msg.subtype)

    async def show_ask(self, msg):
        if isinstance(msg, StreamEvent):
            await self.on_stream(msg, self.ask)
        elif isinstance(msg, RateLimitEvent):
            self.note_usage(msg, self.ask.account)
        elif isinstance(msg, AssistantMessage):
            for b in msg.content:
                if isinstance(b, TextBlock) and not await self.end_stream(self.ask):
                    self.add(Markdown(b.text, classes="claude"), self.ask)
                elif isinstance(b, ToolUseBlock):
                    arg = str(next(iter((b.input or {}).values()), ""))[:70]
                    self.say("note", f"⚙ {b.name} {arg}", self.ask)
        elif isinstance(msg, ResultMessage):
            self.ask.cost += msg.total_cost_usd or 0
            if msg.is_error:
                self.say("err", "; ".join(msg.errors or []) or msg.subtype, self.ask)

    async def on_stream(self, msg, lane):
        if msg.parent_tool_use_id:  # subagent chatter: skip
            return
        ev = msg.event
        if ev.get("type") == "content_block_start" and ev.get("content_block", {}).get("type") == "text":
            await self.end_stream(lane)
            md = Markdown(classes="claude")
            self.add(md, lane)
            lane.stream = Markdown.get_stream(md)
        elif ev.get("type") == "content_block_delta" and ev.get("delta", {}).get("type") == "text_delta" and lane.stream:
            await lane.stream.write(ev["delta"]["text"])
            self.query_one(lane.view).scroll_end(animate=False)

    async def end_stream(self, lane):
        if not lane.stream:
            return False
        stream, lane.stream = lane.stream, None
        await stream.stop()
        return True

    def show_tool(self, b, parent, live=True):
        a = b.input or {}
        if not parent:
            self.track_todos(b.name, a)
        diff = diff_of(b.name, a, self.current_theme, live)
        first = str(next(iter(a.values()), "")).splitlines() if a else []
        title = f"⚙ {b.name}  {first[0][:70] if first else ''}"
        if diff:
            title = f"✎ {b.name}  {os.path.basename(a.get('file_path', ''))}  +{diff[1]} −{diff[2]}"
        elif b.name in ("TodoWrite", "TaskCreate", "TaskUpdate"):
            title = f"☑ {b.name}  {a.get('subject') or 'tasks updated'}"
        body = Static(diff[0] if diff else Text(clip(a)))
        body.source = diff[0] if diff else Text(clip(a))
        c = Collapsible(body, collapsed=not (diff and live), title=escape(("  ↳ " if parent else "") + title))
        self.tools[b.id] = c, body, b.name, a, parent
        self.add(c)

    def show_result(self, b, hit, structured):
        c, view, name, a, parent = hit
        if name == "TaskCreate" and not parent and isinstance(structured, dict):
            self.track_todos(name, a, structured)
        out = b.content if isinstance(b.content, str) else "\n".join(
            x.get("text", "") for x in b.content or [] if isinstance(x, dict))
        if name not in ("Edit", "Write") or b.is_error:  # diffs speak for themselves
            view.update(Text.assemble(view.source, f"\n── result ──\n{clip(out or '(empty)')}"))
        if b.is_error:
            c.title += "  ✗"
            ask = (f"In my Code session, the {name} tool call failed. Explain briefly why and how to fix it.\n\n"
                   f"Input:\n{clip(a, 1500)}\n\nError:\n{clip(out or '(empty)', 1500)}")
            self.query_one(self.code.view).mount(Chip("? Explain this error", data=ask, classes="explain"), after=c)

    # ---- tasks: Claude's own list + tasks found in the project's plan files ------

    @staticmethod
    def set_status(t, status):
        now = time.time()
        if status == "in_progress" and not t.get("started"):
            t["started"] = now
        if status == "completed" and t.get("started") and not t.get("ended"):
            t["ended"] = now
        if status == "pending":
            t["started"] = t["ended"] = None
        t["status"] = status

    def track_todos(self, name, a, result=None):
        if name == "TodoWrite":  # the whole list each time; keep timers by matching the text
            old = self.tasks
            self.tasks = {k: t for k, t in old.items() if not k.startswith("c:")}
            for todo in a.get("todos", []):
                key = "c:" + todo.get("content", "")
                t = {**old.get(key, {}), "key": key, "title": todo.get("content", ""),
                     "active": todo.get("activeForm", "")}
                self.set_status(t, todo.get("status", "pending"))
                self.tasks[key] = t
        elif name == "TaskCreate" and result and (task := result.get("task")):
            key = f"c:{task['id']}"
            self.tasks[key] = {"key": key, "title": a.get("subject", ""), "active": a.get("activeForm", ""),
                               "status": "pending"}
        elif name == "TaskUpdate" and (t := self.tasks.get(f"c:{a.get('taskId')}")):
            if a.get("status") == "deleted":
                del self.tasks[t["key"]]
            else:
                t.update({k: a[v] for k, v in (("title", "subject"), ("active", "activeForm")) if v in a})
                "status" in a and self.set_status(t, a["status"])
        else:
            return
        self.render_tasks()

    def set_plan_tasks(self, found):
        self.tasks = {k: t for k, t in self.tasks.items() if not k.startswith("p:")}
        for i, f in enumerate(found):
            key = f"p:{i}"
            status = f.get("status") or ("completed" if f.get("done") else "pending")
            self.tasks[key] = {**f, "key": key, "status": status}
        self.render_tasks()

    def save_plan_tasks(self):
        plan = [{k: v for k, v in t.items() if k != "key"} for k, t in self.tasks.items() if k.startswith("p:")]
        store.save_project(self.cwd, {"tasks": plan, "at": time.time()})

    def load_plan_tasks(self, auto=False):
        if data := store.load_project(self.cwd):
            self.set_plan_tasks(data.get("tasks", []))
        elif auto and store.has_plan_files(self.cwd):
            self.scout()

    @work(exclusive=True, group="scout", exit_on_error=False)
    async def scout(self):
        """A cheap read-only Haiku run that pulls open tasks out of HANDOVER.md, PLAN.md, CLAUDE.md and friends."""
        status, spin = self.query_one("#tasks-status", Static), self.query_one("#tasks-refresh", Chip)
        frames = iter(itertools.cycle("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"))

        def tick():
            f = next(frames)
            status.update(f"{f} scanning plan files…")
            spin.update(f)
            if not self.tasks:  # nothing listed yet: say so where the list will appear
                box, text = self.query_one("#tasks-list"), f"{f} looking for tasks in HANDOVER, PLAN, TODO, CLAUDE.md…"
                if note := box.query(".scanning"):
                    note.first().update(text)
                else:
                    box.remove_children()
                    box.mount(Static(text, classes="task-empty scanning"))

        timer = self.set_interval(0.1, tick)
        tick()
        self.refresh_side() if self.side_closed else self.query_one("#side").add_class("-on")
        try:
            await self.scan_plans(status)
        finally:
            timer.stop()
            spin.update("↻")
            if not self.tasks:
                self.render_tasks()

    async def scan_plans(self, status):
        opts = ClaudeAgentOptions(
            cwd=self.cwd, cli_path=self.claude, env=store.account_env(self.session["account"]), model=SCOUT_MODEL,
            tools=["Read", "Glob", "Grep"], allowed_tools=["Read", "Glob", "Grep"], permission_mode="dontAsk",
            setting_sources=[], strict_mcp_config=True, max_turns=15, extra_args=NO_HISTORY,
            output_format={"type": "json_schema", "schema": SCOUT_SCHEMA},
            system_prompt="You extract task lists from project documents. Be precise and terse.")
        result = None
        try:
            async for m in query(prompt=SCOUT_PROMPT, options=opts):
                if isinstance(m, ResultMessage):
                    result = m
                elif isinstance(m, RateLimitEvent):
                    self.note_usage(m, self.session["account"])
        except Exception as e:
            status.update("scan failed")
            self.say("err", f"task scan failed: {e}")
            return
        found = ((result and result.structured_output) or {}).get("tasks") or []
        self.set_plan_tasks(found)
        self.save_plan_tasks()
        status.update(f"{len(found)} from plan files" if found else "no plan files found")

    def render_tasks(self):
        self.query_one(TaskPanel).show(list(self.tasks.values()), time.time())
        self.refresh_side()

    def tick_tasks(self):
        if any(t["status"] == "in_progress" for t in self.tasks.values()):
            self.query_one(TaskPanel).tick(list(self.tasks.values()), time.time())

    async def task_clicked(self, key):
        t = self.tasks.get(key)
        if not t or t["status"] == "completed":
            return
        plan = key.startswith("p:")
        if t["status"] == "in_progress":
            if plan and await self.modal(Confirm("TASK", "Mark this task as finished?", t["title"], yes="Finished")):
                self.set_status(t, "completed")
                self.save_plan_tasks()
                self.render_tasks()
            elif not plan:
                self.notify("Claude is working on this one.")
            return
        if self.code.busy or any(x["status"] == "in_progress" for x in self.tasks.values()):
            self.notify("A task is already running. Let it finish, or press Esc to stop Claude.")
            return
        body = "\n\n".join(x for x in (t["title"], t.get("detail", ""), f"from {t['source']}" if plan else "") if x)
        if not await self.modal(Confirm("START TASK", "Want to start this task?", body)):
            return
        if plan:
            self.set_status(t, "in_progress")
            self.save_plan_tasks()
            self.render_tasks()
            prompt = f"Let's work on this task from {t['source']}:\n{t['title']}\n\n{t.get('detail', '')}".strip()
        else:
            prompt = f"Start on this task now: {t['title']}"
        self.switch_tab("code")
        self.send(prompt)

    # ---- sessions ------------------------------------------------------------

    def recent_sessions(self, limit=50):
        try:
            return list_sessions(self.cwd, limit=limit, include_worktrees=False)
        except Exception:
            return []

    async def resume_session(self, sid):
        self.switch_tab("code")  # both side panels stay as they are
        await self.reset_view()
        self.session_id = sid
        try:
            msgs = await asyncio.to_thread(get_session_messages, sid, self.cwd)
        except Exception as e:
            msgs = []
            self.say("err", f"could not read that session: {e}")
        for m in msgs[-300:]:  # ponytail: replay the last 300 messages only
            await self.replay(m)
        self.say("note", "── resumed ──")
        self.reconnect()

    async def replay(self, m):
        content = m.message.get("content")
        if isinstance(content, str):
            content = [{"type": "text", "text": content}]
        if not isinstance(content, list):
            return
        texts = [b["text"] for b in content if b.get("type") == "text" and not b["text"].startswith("<")]
        if m.type == "user":
            if texts:
                self.add(Static(Text("\n".join(texts)), classes="user"))
            results = [ToolResultBlock(b["tool_use_id"], b.get("content"), b.get("is_error"))
                       for b in content if b.get("type") == "tool_result"]
            results and await self.show(UserMessage(content=results), live=False)
        else:
            blocks = [TextBlock(b["text"]) if b["type"] == "text" else ToolUseBlock(b["id"], b["name"], b.get("input") or {})
                      for b in content if b.get("type") in ("text", "tool_use")]
            blocks and await self.show(AssistantMessage(content=blocks, model=""), live=False)

    # ---- clear-continue: the hands-off loop (one task per session, handover in between) ----

    @property
    def looping(self):  # on/off is saved per project folder
        return self.cwd in self.cfg["loop_dirs"]

    def loop_limit_hit(self):
        pct = store.usage_pct(self.cfg, self.session["account"])
        return pct if pct is not None and pct >= LOOP_LIMIT else None

    def set_loop(self, on, kickoff=True):
        dirs = [d for d in self.cfg["loop_dirs"] if d != self.cwd]
        self.cfg["loop_dirs"] = dirs + [self.cwd] if on else dirs
        self.armed = False
        self.save_and_refresh()
        if not on:
            self.say("note", "clear-continue: OFF.")
        elif kickoff:
            pct = store.usage_pct(self.cfg, self.session["account"])
            self.say("note", f"clear-continue: ON. Usage {'unknown' if pct is None else f'{pct}%'}, "
                             f"stops at {LOOP_LIMIT}%. Toggle it again to stop.")
            self.switch_tab("code")
            self.code.inbox.put_nowait(LOOP_RULES)

    def clear_continue(self):
        """The turn that finished a task has ended: fresh session, then pick up the handover."""
        self.armed = False
        if not self.looping:
            return
        if (pct := self.loop_limit_hit()) is not None:
            self.set_loop(False, kickoff=False)
            self.notify(f"clear-continue stopped: usage at {pct}%.", severity="warning", timeout=10)
            return
        self.session_id, self.code.cost = None, 0.0
        self.reset_view()
        self.reconnect()
        self.say("note", "↻ clear-continue: fresh session, picking up the handover")
        self.code.inbox.put_nowait(LOOP_RULES)
        self.scout()  # the handover changed: refresh the plan tasks

    # ---- advisor: a read-only reviewer that looks over the session every few turns ----

    def count_turn(self):
        self.turns += 1
        adv = self.cfg["advisor"]
        if adv["mode"] != "off" and self.turns % adv["every"] == 0:
            self.advise()

    def recent_transcript(self, limit=60, budget=12000):
        if not self.session_id:
            return "(no conversation yet)"
        try:
            msgs = get_session_messages(self.session_id, self.cwd)[-limit:]
        except Exception:
            return "(conversation unavailable)"
        lines = []
        for m in msgs:
            content = m.message.get("content")
            for b in [{"type": "text", "text": content}] if isinstance(content, str) else content or []:
                if b.get("type") == "text" and not b["text"].startswith("<"):
                    lines.append(f"{m.type}: {clip(b['text'], 400)}")
                elif b.get("type") == "tool_use":
                    lines.append(f"tool: {b['name']} {clip(b.get('input') or {}, 150)}")
        return clip("\n".join(lines)[-budget:], budget)

    @work(exclusive=True, group="advisor", exit_on_error=False)
    async def advise(self, manual=False):
        manual and self.notify("Advisor is looking over the project…", title="advisor")
        transcript = await asyncio.to_thread(self.recent_transcript)
        tools = ["Read", "Glob", "Grep", "WebSearch", "WebFetch"]
        opts = ClaudeAgentOptions(
            cwd=self.cwd, cli_path=self.claude, env=store.account_env(self.session["account"]),
            model=self.cfg["ask_model"] or None, tools=tools, allowed_tools=tools, permission_mode="dontAsk",
            strict_mcp_config=True, max_turns=25, extra_args=NO_HISTORY, system_prompt=ADVISOR_SYSTEM,
            output_format={"type": "json_schema", "schema": ADVISOR_SCHEMA})
        seen = (store.load_project(self.cwd, "advisor") or {}).get("titles", [])
        prompt = (f"{ADVISOR_PROMPT}\n\nCommands and skills available to the session: {' '.join(self.slash)}\n"
                  f"Already suggested, don't repeat: {seen or 'nothing yet'}\n\nRecent conversation:\n{transcript}")
        result = None
        try:
            async for m in query(prompt=prompt, options=opts):
                if isinstance(m, ResultMessage):
                    result = m
                elif isinstance(m, RateLimitEvent):
                    self.note_usage(m, self.session["account"])
        except Exception as e:
            self.say("err", f"advisor failed: {e}")
            return
        found = ((result and result.structured_output) or {}).get("suggestions") or []
        # ponytail: last 50 titles per project; enough to stop repeats without growing forever
        store.save_project(self.cwd, {"titles": (seen + [s["title"] for s in found])[-50:]}, "advisor")
        if not found:
            manual and self.notify("The advisor has nothing to add right now.", title="advisor")
            return
        if self.cfg["advisor"]["mode"] == "act" and not manual:
            picks = [s for s in found if s.get("recommended")]
            for s in found:
                self.say("note", f"advisor{' → acting' if s in picks else ''}: {s['title']}. {s['why']}")
            picks and self.hand_to_claude(picks, "")
            return
        self.bell()
        answers = await self.modal(Form(self.advice_form(found)))
        if answers and (picks := [found[int(i)] for i in answers["pick"]]):
            self.hand_to_claude(picks, answers["note"])
        elif answers and answers["note"]:
            self.send(f"About the advisor's suggestions: {answers['note']}")

    @staticmethod
    def advice_form(found):
        def block(s):
            links = "".join(f"\n  - <{u}>" for u in s.get("links") or [])
            return f"**{s['title']}**  _{s['kind']}_\n\n{s['why']}{links}"
        return {"title": "ADVISOR", "question": "\n\n".join(block(s) for s in found), "fields": [{
            "id": "pick", "type": "checkbox", "label": "Send to Claude",
            "options": [{"value": str(i), "label": s["title"]} for i, s in enumerate(found)],
            "recommended": [str(i) for i, s in enumerate(found) if s.get("recommended")]}]}

    def hand_to_claude(self, picks, note):
        steps = "\n".join(f"- {s['prompt']}" for s in picks)
        self.switch_tab("code")
        self.send(f"The project advisor suggested this and I approve:\n{steps}" + (f"\n\n{note}" if note else ""))

    def advisor_settings(self):
        adv = self.cfg["advisor"]
        spec = {"title": "ADVISOR", "question": (
            "A read-only reviewer that looks over the session every few Code turns: scope creep, reuse, performance, "
            "lighter or newer libraries, missing tests, helpful skills. It runs on the Ask Claude model."), "fields": [
            {"id": "mode", "type": "radio", "label": "Mode", "default": adv["mode"], "recommended": "suggest",
             "options": [{"value": k, "label": v} for k, v in ADVISOR_MODES.items()]},
            {"id": "every", "type": "select", "label": "Look over the session every", "default": str(adv["every"]),
             "options": [{"value": str(n), "label": f"{n} turns"} for n in (3, 5, 8, 12, 20)]}]}

        def saved(answers):
            if answers:
                self.cfg["advisor"] = {"mode": answers["mode"] or adv["mode"], "every": int(answers["every"] or adv["every"])}
                self.save_and_refresh()
        self.push_screen(Form(spec), saved)

    # ---- install (first run) ------------------------------------------------------

    @work(exclusive=True, group="install", exit_on_error=False)
    async def install_claude(self):
        card, lines = self.query_one(InstallCard), []
        card.log("Installing… (this takes a minute)")
        try:
            proc = await asyncio.create_subprocess_exec(
                "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command",
                "irm https://claude.ai/install.ps1 | iex",
                stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.STDOUT)
            async for raw in proc.stdout:
                if line := raw.decode(errors="replace").rstrip():
                    lines.append(line)
                    card.log("\n".join(lines[-8:]))
            await proc.wait()
        except OSError as e:
            lines.append(f"could not run the installer: {e}")
        self.claude = find_claude()
        card.log("\n".join(lines[-6:] + ["", "✔ Installed. Click “Sign in, then continue”." if self.claude
                                          else "✗ Install failed. See the output above."]))

    def sign_in(self):
        """Open the CLI in its own console for the one-time login, then connect."""
        self.claude = self.claude or find_claude()
        if not self.claude:
            self.query_one(InstallCard).log("Install Claude Code first.")
            return
        subprocess.Popen(["cmd", "/c", "start", "Claude Code sign-in", "cmd", "/k", self.claude])
        self.query_one(InstallCard).remove()
        self.show_welcome()
        self.say("note", "Finish signing in in the new window (then type /exit there). "
                         "If the first message here fails, it retries once you're signed in.")
        self.agent_loop()
        self.load_plan_tasks(auto=True)

    # ---- attachments ---------------------------------------------------------

    def add_attachments(self, paths):
        self.attachments += [p for p in paths if p not in self.attachments]
        self.render_attachments()

    def render_attachments(self):
        box = self.query_one("#attachments")
        box.remove_children()
        box.mount_all([Chip(f"▣ {os.path.basename(p)}  ×", data=p, classes="att") for p in self.attachments])
        box.set_class(bool(self.attachments), "-on")

    def handle_paste(self, text):
        """From the prompt: True when the paste became attachments instead of text."""
        if paths := attach.paths_in(text):
            self.add_attachments(paths)
            return True
        if not text.strip():  # Windows Terminal sends an empty paste when the clipboard holds an image
            self.paste_clipboard()
            return True
        return False

    @work(group="clipboard", exit_on_error=False)
    async def paste_clipboard(self):
        if files := await attach.clipboard_files():
            self.add_attachments(files)
        else:
            self.notify("Nothing to paste: the clipboard has no text, image or files.")

    @work(group="picker", exit_on_error=False)
    async def pick_files(self):
        self.add_attachments(await attach.pick_files())

    # ---- ui ----------------------------------------------------------------

    def apply_theme(self, name, save=True):
        name = name if name in THEMES else "classic"
        self.theme = name
        self.render_tasks()  # task colors are baked in
        if save:
            self.cfg["theme"] = name
            store.save(self.cfg)

    def show_welcome(self):
        last = self.recent_sessions(1)
        facts = [self.cwd, store.git_facts(self.cwd)]
        if last:
            facts.append(f"last session {store.ago(last[0].last_modified)}: “{(last[0].summary or '')[:50]}”")
            facts.append(store.git_since(self.cwd, last[0].last_modified))
        chips = [s for s in SUGGESTIONS if last or s[1] != "resume_last"]
        self.query_one("#chat").mount(Welcome(store.first_name(), " · ".join(f for f in facts if f), chips))

    def reset_view(self):
        """Empty the Code chat. Returns an awaitable that's done once the old widgets are really gone."""
        self.tools.clear()
        self.track_todos("TodoWrite", {})
        return self.query_one("#chat").remove_children()

    def refresh_side(self):
        # opens itself when there's something to show, unless the person collapsed it
        if (self.tasks or self.query_one("#panel").display) and not self.side_closed:
            self.query_one("#side").add_class("-on")
        self.sync_rails()

    def sync_rails(self, sessions=False):
        """A collapsed side panel shows as a narrow clickable rail with its counts."""
        side, resume = self.query_one("#side").has_class("-on"), self.query_one("#resume").has_class("-on")
        rail = self.query_one("#rail-side", Chip)
        rail.display = not side
        rail.update(self.rail_text("«", "TASKS", sum(t["status"] != "completed" for t in self.tasks.values())))
        rail = self.query_one("#rail-resume", Chip)
        rail.display = not resume
        if sessions or not hasattr(self, "sessions_rail"):  # reads the session folder: only when asked
            recent = self.recent_sessions()
            self.sessions_rail = (len(recent), store.ago(recent[0].last_modified).split()[0] if recent else "")
        rail.update(self.rail_text("»", "SESSIONS", *self.sessions_rail))

    @staticmethod
    def rail_text(arrow, title, *facts):
        # every fact runs down the column too: the rail is 3 cells wide, "12" or "5m" would wrap or clip
        facts = [str(f).replace("just", "now") for f in facts if f != ""]
        return "\n".join([arrow, "", *title] + [c for f in facts for c in ["", *f]])

    def add(self, widget, lane=None):
        view = self.query_one((lane or self.code).view)
        for w in view.query("#welcome, .ask-intro"):
            w.remove()
        view.mount(widget)
        view.scroll_end(animate=False)

    def say(self, cls, text, lane=None):
        self.add(Static(Text(text), classes=cls), lane)

    def set_commands(self, names):
        names = {"/" + n.lstrip("/") for n in names
                 if n.lstrip("/") not in self.term_only and not n.lstrip("/").startswith("__")}
        self.slash = sorted(names | {"/" + n for n in LOCAL_COMMANDS})

    def spin(self):
        if self.lane.busy:
            self.frame += 1
            self.query_one("#state", Static).update(
                Text.assemble((SPINNER[self.frame % len(SPINNER)], self.current_theme.primary), " working · esc to stop"))

    def refresh_bars(self):
        c, lane = self.cfg, self.lane
        if not lane.busy:
            self.query_one("#state", Static).update("○ ready")
        mode = self.query_one("#mode", Chip)
        mode.update(f"◇ {MODES[c['mode']]}")
        mode.display = self.tab == "code"
        s = self.session
        model = s["model"] if self.tab == "code" else c["ask_model"]
        web = " · ⊕ web" if s["web"] else ""
        pct = store.usage_pct(c, s["account"])
        used = f" {pct}%" if pct is not None else ""
        self.query_one("#picker", Chip).update(
            f"{store.nick(c, s['account'])}{used} · {MODELS.get(model, model)} · ◉ {EFFORTS[s['effort']]}{web}  ⌄")
        live = f"{self.live_model} · " if self.live_model and not s["model"] and self.tab == "code" else ""
        where = f"session {(self.session_id or 'new')[:8]}" if self.tab == "code" else "ask claude · not saved"
        loop = "↻ clear-continue · " if self.looping else ""
        self.query_one("#menu-info", Static).update(f"{loop}{live}{where} · ${lane.cost:.4f}")

    def switch_tab(self, tab):
        if tab == self.tab:
            return
        self.tab = tab
        for t in ("code", "ask"):
            self.query_one(f"#tab-{t}").set_class(t == tab, "-active")
        self.query_one("#chat").display = tab == "code"
        ask_view = self.query_one("#ask")
        ask_view.display = tab == "ask"
        if tab == "ask" and not ask_view.children:
            ask_view.mount(Static("Ask Claude: a side chat for quick questions, explanations and ideas.\n"
                                  "It can read this project and search the web, but never changes anything,\n"
                                  "and it doesn't touch your Code session.", classes="ask-intro"))
        prompt = self.query_one("#prompt", Prompt)
        prompt.placeholder = ("Ask Claude anything…  (/ for commands)" if tab == "code"
                              else "Quick question? Read-only, and separate from your Code session.")
        prompt.focus()
        self.refresh_bars()

    def send(self, text):
        lane, files = self.lane, self.attachments
        shown = text + "".join(f"\n▣ {os.path.basename(p)}" for p in files)
        self.add(Static(Text(shown), classes="user"), lane)
        if lane.busy:
            self.say("note", "queued until the current reply finishes.", lane)
        lane.inbox.put_nowait(attach.content_blocks(text, files) if files else text)
        self.attachments = []
        self.render_attachments()
        if lane is self.ask and not lane.started:
            lane.started = True
            self.ask_loop()

    def local_command(self, text):
        """Handle commands this UI owns. True when handled."""
        cmd, _, arg = text[1:].partition(" ")
        arg, lane = arg.strip(), self.lane
        if cmd == "help":
            local = "  ".join(f"/{k}" for k in LOCAL_COMMANDS)
            claude = " ".join(c for c in self.slash if c[1:] not in LOCAL_COMMANDS)
            self.say("note", f"this UI: {local}\nclaude: {claude}\n\n{SHORTCUTS}", lane)
        elif cmd == "theme":
            self.apply_theme(arg) if arg in THEMES else self.do("theme")
        elif cmd in ("new", "clear", "resume"):
            self.do(cmd if cmd != "clear" else "new")
        elif cmd in ("ask", "code"):
            self.switch_tab(cmd)
        elif cmd == "clear-continue":
            self.do("loop")
        elif cmd == "advisor":
            self.advise(manual=True)
        elif cmd == "tasks":
            self.scout()
        elif cmd == "account":
            match = next((a["id"] for a in self.cfg["accounts"] if arg.lower() in (a["id"].lower(), a["nick"].lower())), None)
            self.switch_account(match) if arg and match else self.do("accounts")
        elif cmd in self.term_only:
            self.say("note", f"/{cmd} only works in the official terminal app.", lane)
        else:
            return False
        return True

    # slash suggestion list -------------------------------------------------

    def on_text_area_changed(self, e):
        text, lst = e.text_area.text, self.query_one("#slash", OptionList)
        matches = ([c for c in self.slash if c.startswith(text.lower())][:8]
                   if text.startswith("/") and " " not in text and "\n" not in text else [])
        lst.clear_options()
        lst.add_options([Option(c, id=c) for c in matches])
        lst.set_class(bool(matches) and matches != [text], "-on")
        lst.highlighted = 0 if matches else None

    def slash_key(self, key):
        lst = self.query_one("#slash", OptionList)
        if not lst.has_class("-on"):
            return False
        if key in ("up", "down"):
            lst.action_cursor_up() if key == "up" else lst.action_cursor_down()
        elif key in ("tab", "enter") and lst.highlighted is not None:
            cmd = lst.get_option_at_index(lst.highlighted).id
            lst.remove_class("-on")
            prompt = self.query_one("#prompt", Prompt)
            if key == "enter":
                prompt.text = ""
                self.on_prompt_submitted(Prompt.Submitted(cmd))
            else:
                prompt.text = cmd + " "
                prompt.move_cursor(prompt.document.end)
        elif key == "escape":
            lst.remove_class("-on")
        else:
            return False
        return True

    # events ----------------------------------------------------------------

    def on_prompt_submitted(self, e: Prompt.Submitted):
        text = e.text.strip()
        self.query_one("#prompt", Prompt).text = ""
        if (text or self.attachments) and not (text.startswith("/") and self.local_command(text)):
            self.send(text)

    def on_option_list_option_selected(self, e: OptionList.OptionSelected):
        if e.option_list.id == "slash":
            self.slash_key("enter")

    async def on_resume_panel_picked(self, e: ResumePanel.Picked):
        await self.resume_session(e.session_id)

    def on_chip_pressed(self, e: Chip.Pressed):
        chip = e.chip
        if chip.id and chip.id.startswith("m-"):
            self.action_menu(chip.id[2:])
        elif chip.id and chip.id.startswith("tab-"):
            self.switch_tab(chip.id[4:])
        elif chip.id == "mode":
            self.action_cycle_mode()
        elif chip.id == "picker":
            self.open_picker()
        elif chip.id == "attach":
            self.pick_files()
        elif chip.id == "tip-next":
            self.query_one("#welcome", Welcome).next_tip()
        elif chip.id in ("rail-side", "side-close"):
            self.do("side")
        elif chip.id in ("rail-resume", "resume-close"):
            self.do("resume")
        elif chip.id == "tasks-refresh":
            self.scout()
        elif chip.has_class("att"):
            self.attachments.remove(chip.data)
            self.render_attachments()
        elif chip.has_class("task"):
            self.run_worker(self.task_clicked(chip.data), exit_on_error=False)
        elif chip.data in ("install_claude", "sign_in"):
            self.install_claude() if chip.data == "install_claude" else self.sign_in()
        elif chip.has_class("explain"):  # failed tool call → Ask Claude lane
            self.switch_tab("ask")
            self.send(chip.data)
        elif chip.has_class("suggest"):
            self.act(chip.data) if chip.data == "resume_last" else self.send(chip.data)

    # menus -------------------------------------------------------------------

    def menus(self):
        c, has = self.cfg, lambda cmd: cmd in self.slash
        check = lambda on: "✓ " if on else "  "  # noqa: E731
        claude_cmds = [(label, "", cmd) for label, cmd in (
            ("  Context usage", "/context"), ("  Compact context", "/compact"), ("  Usage & limits", "/usage"),
            ("  Init CLAUDE.md", "/init"), ("  Review changes", "/review"),
            ("  Security review", "/security-review"), ("  MCP servers", "/mcp")) if has(cmd)]
        return {
            "claude": [("  New session", "ctrl+n", "new"), ("  Resume session…", "ctrl+r", "resume"),
                       ("  Change folder…", "ctrl+o", "folder"), None,
                       ("  Attach files…", "", "attach"), ("  Clear screen", "ctrl+l", "clear"),
                       ("  Copy last reply", "", "copy"), None,
                       ("  Permissions & web", "▸", "sub:permissions"),
                       (check(self.looping) + "Clear & continue loop", "", "loop"),
                       (f"  Advisor: {c['advisor']['mode'].title()}…", "", "advisor"),
                       ("  Ask the advisor now", "", "advise"), ("  Rescan plan files", "", "scout")]
                      + ([("  Claude Code commands", "▸", "sub:commands")] if claude_cmds else [])
                      + [("  Commit…", "", "commit"), ("  Accounts…", "", "accounts"), None,
                         ("  Restart" + (" · new build ready" if self.new_build() else ""), "", "restart"),
                         ("  Exit", "ctrl+q", "quit")],
            "view": [("Theme…", "ctrl+t", "theme")]
                    + [(check(c["theme"] == n) + n.title(), "", f"theme:{n}") for n in THEMES]
                    + [None, ("  Side panel", "ctrl+b", "side"),
                       ("  Recent sessions", "ctrl+r", "resume"), ("  Ask Claude ↔ Code", "f2", "toggle_tab"), None,
                       ("  Expand tool calls", "", "expand"), ("  Collapse tool calls", "", "collapse")],
            "help": [("Keyboard shortcuts", "", "shortcuts"), ("All commands", "/help", "/help"),
                     ("Next tip", "", "tip"), None, ("About claude-term", "", "about")],
            # submenus: opened from a "sub:<name>" entry, not on the bar
            "permissions": [("#", "Permission mode  (shift+tab)")]
                           + [(check(c["mode"] == m) + label, "", f"mode:{m}") for m, label in MODES.items()]
                           + [None, (check(self.session["web"]) + "Web search", "", "web")],
            "commands": claude_cmds,
        }

    def action_menu(self, name):
        order = list(MENUBAR)
        chip = self.query_one(f"#m-{name}", Chip)
        chip.add_class("-open")
        bar = {n: self.query_one(f"#m-{n}").region for n in order}

        def done(result):
            # runs *before* the menu is popped, so anything that opens a screen waits a tick (call_later);
            # opening one here would get popped instead of the menu
            chip.remove_class("-open")
            if result in ("<", ">"):
                self.call_later(self.action_menu, order[(order.index(name) + (1 if result == ">" else -1)) % len(order)])
            elif result and result.startswith("menu:"):
                self.call_later(self.action_menu, result[5:])
            elif result:
                self.call_later(self.act, result)

        menus = self.menus()
        subs = {k: v for k, v in menus.items() if k not in MENUBAR}
        self.push_screen(PopupMenu(menus[name], chip.region.x, chip.region.y + 1, name=name, bar=bar, subs=subs), done)

    def open_picker(self):
        c, s = self.cfg, self.session
        current_model = s["model"] if self.tab == "code" else c["ask_model"]
        dot = lambda on: "● " if on else "○ "  # noqa: E731

        def account_row(a):
            pct = store.usage_pct(c, a["id"])
            return (("✓ " if s["account"] == a["id"] else "  ") + a["nick"], f"{pct}%" if pct is not None else "",
                    f"account:{a['id']}")
        items = ([("#", "Account  (this session)")] + [account_row(a) for a in c["accounts"]]
                 + [("  Manage accounts…", "", "accounts")]
                 + [None, ("#", "Model" + ("  (Ask Claude)" if self.tab == "ask" else ""))]
                 + [(("✓ " if current_model == m else "  ") + label, "", f"model:{m}") for m, label in MODELS.items()]
                 + [None, ("#", "Effort")]
                 + [(dot(s["effort"] == e) + label, "", f"effort:{e}") for e, label in EFFORTS.items()]
                 + [None, (("✓ " if s["web"] else "  ") + "Web search", "", "web"),
                    (("✓ " if self.looping else "  ") + "Clear & continue loop", "", "loop")])
        chip = self.query_one("#picker", Chip)
        x = max(0, chip.region.right - 34)
        self.push_screen(PopupMenu(items, x, chip.region.y, above=True), lambda r: r and self.call_later(self.act, r))

    def act(self, action):
        if action.startswith("/"):
            self.local_command(action) or self.send(action)
        elif ":" in action:
            kind, value = action.split(":", 1)
            {"theme": lambda: self.apply_theme(value), "mode": lambda: self.set_mode(value),
             "model": lambda: self.set_model(value), "effort": lambda: self.set_effort(value),
             "account": lambda: self.switch_account(value)}[kind]()
        else:
            self.do(action)

    def action_do(self, action):
        self.do(action)

    def do(self, action):
        if action == "new":
            self.handover_then(self.new_session) if self.handover_due() else self.new_session()
        elif action == "resume":
            panel = self.query_one("#resume", ResumePanel)
            panel.toggle_class("-on")
            self.sync_rails(sessions=True)
            if panel.has_class("-on"):
                panel.show(self.recent_sessions(), store.ago)
                panel.query_one("#resume-list").focus()
            else:
                self.query_one("#prompt").focus()
        elif action == "resume_last" and (last := self.recent_sessions(1)):
            self.run_worker(self.resume_session(last[0].session_id), exit_on_error=False)
        elif action == "toggle_tab":
            self.switch_tab("ask" if self.tab == "code" else "code")
        elif action == "folder":
            self.push_screen(AskText("CHANGE FOLDER", self.cwd), self.change_folder)
        elif action == "attach":
            self.pick_files()
        elif action == "scout":
            self.scout()
        elif action == "advisor":
            self.advisor_settings()
        elif action == "commit":
            self.commit()
        elif action == "advise":
            self.advise(manual=True)
        elif action == "loop":
            self.set_loop(not self.looping)
        elif action == "clear":
            self.query_one(self.lane.view).remove_children()
        elif action == "copy" and self.last_reply:
            self.copy_to_clipboard(self.last_reply)
            self.notify("Last reply copied.")
        elif action == "quit":
            self.handover_then(self.exit) if self.handover_due() else self.exit()
        elif action == "restart":
            self.restart()
        elif action == "theme":
            self.push_screen(ThemePicker(self.theme), lambda n: n and self.apply_theme(n))
        elif action == "side":
            self.side_closed = self.query_one("#side").toggle_class("-on").has_class("-on") is False
            self.sync_rails()
        elif action in ("expand", "collapse"):
            for c in self.query(Collapsible):
                c.collapsed = action == "collapse"
        elif action == "web":
            self.session["web"] = not self.session["web"]
            self.refresh_bars()
            self.reconnect(f"web search {'on' if self.session['web'] else 'off'} for this session")
        elif action == "accounts":
            usage = {a["id"]: store.usage_text(self.cfg, a["id"]) for a in self.cfg["accounts"]
                     if a["id"] in self.cfg["usage"]}
            self.push_screen(AccountManager(self.cfg, usage, MODELS, EFFORTS, self.session["account"]),
                             self.accounts_saved)
        elif action == "shortcuts":
            self.say("note", SHORTCUTS, self.lane)
        elif action == "tip":
            self.notify(random.choice(TIPS), title="Did you know?")
        elif action == "about":
            self.notify("A themeable wrapper around your installed Claude Code. Same commands, skills and MCP.",
                        title="claude-term")

    def launcher(self):
        """How to start claude-term again: the exe build.ps1 makes (the running one may be a renamed old copy)."""
        if getattr(sys, "frozen", False):
            return [os.path.join(os.path.dirname(sys.executable), "claude-term.exe")]
        return [sys.executable, os.path.abspath(__file__)]

    def new_build(self):
        exe = self.launcher()[-1]
        return getattr(sys, "frozen", False) and os.path.isfile(exe) and os.path.getmtime(exe) > self.started_at

    def restart(self):
        """Exit, and have main() start the (new) build in this folder, back in this session."""
        self.restart_cmd = self.launcher() + [self.cwd] + (["--resume", self.session_id] if self.session_id else [])
        self.exit()

    def new_session(self):
        self.session_id, self.code.cost, self.worked = None, 0.0, False
        self.session = self.fresh_session(self.session["account"])
        self.refresh_bars()
        self.switch_tab("code")
        self.run_worker(self.fresh_view, group="view", exclusive=True, exit_on_error=False)
        self.reconnect()
        self.sync_rails(sessions=True)

    async def fresh_view(self):
        # the old welcome must be gone before the new one mounts (same id): New twice in a row crashed
        await self.reset_view()
        self.show_welcome()

    def action_quit(self):  # ctrl+q: same as Claude → Exit
        self.do("quit")

    # ---- handover writer: before leaving a session, offer to bring HANDOVER.md up to date ----

    def handover_due(self):
        # the clear-continue loop keeps its own handover
        return self.worked and self.session_id and self.claude and not self.looping

    @work(exclusive=True, group="handover", exit_on_error=False)
    async def handover_then(self, after):
        try:
            if await self.modal(Confirm("HANDOVER", "Update HANDOVER.md with this session first?",
                                        "A quick Haiku pass drafts it; you see the changes before anything is saved.",
                                        yes="Update")):
                await self.write_handover()
        except Exception as e:
            self.say("err", f"handover failed: {e}")
        self.worked = False
        after()

    async def write_handover(self):
        path = os.path.join(self.cwd, "HANDOVER.md")
        old = open(path, encoding="utf-8").read() if os.path.isfile(path) else ""
        self.notify("Drafting the handover…", title="handover")
        transcript = await asyncio.to_thread(self.recent_transcript)
        tools = ["Read", "Glob", "Grep"]  # read-only: claude-term writes the file itself, after you confirm
        opts = ClaudeAgentOptions(
            cwd=self.cwd, cli_path=self.claude, env=store.account_env(self.session["account"]), model=SCOUT_MODEL,
            tools=tools, allowed_tools=tools, permission_mode="dontAsk", setting_sources=[], strict_mcp_config=True,
            max_turns=15, extra_args=NO_HISTORY, output_format={"type": "json_schema", "schema": HANDOVER_SCHEMA},
            system_prompt="You keep a project's handover file accurate and terse.")
        prompt = (f"{HANDOVER_PROMPT}\n\nCurrent HANDOVER.md:\n{old or '(none yet)'}\n\n"
                  f"Recent conversation:\n{transcript}")
        result = None
        async for m in query(prompt=prompt, options=opts):
            if isinstance(m, ResultMessage):
                result = m
            elif isinstance(m, RateLimitEvent):
                self.note_usage(m, self.session["account"])
        new = ((result and result.structured_output) or {}).get("markdown", "").strip()
        if not new or new == old.strip():
            self.notify("The handover is already up to date.", title="handover")
            return
        diff = diff_of("Write", {"file_path": path, "content": new + "\n"}, self.current_theme)[0]
        if await self.modal(Confirm("HANDOVER", "Save these changes to HANDOVER.md?", diff, yes="Save")):
            with open(path, "w", encoding="utf-8") as f:
                f.write(new + "\n")
            self.notify("HANDOVER.md updated.", title="handover")

    # ---- commit writer: Haiku drafts the message, you edit and confirm ----

    async def git(self, *args):
        proc = await asyncio.create_subprocess_exec("git", *args, cwd=self.cwd, stdout=asyncio.subprocess.PIPE,
                                                    stderr=asyncio.subprocess.STDOUT)
        out, _ = await proc.communicate()
        return proc.returncode, out.decode(errors="replace").rstrip()  # porcelain lines start with a space

    @work(exclusive=True, group="commit", exit_on_error=False)
    async def commit(self):
        try:
            code, status = await self.git("status", "--porcelain")
        except OSError:
            code, status = 1, "git isn't installed"
        if code:
            self.notify(f"Can't commit here: {status.splitlines()[0] if status else 'not a git repository'}",
                        severity="warning")
            return
        if not status:
            self.notify("Nothing to commit.", title="commit")
            return
        staged = [line for line in status.splitlines() if line[0] not in " ?"]
        files = "\n".join(staged or status.splitlines())
        # what gets committed: what's staged, or else everything (untracked files included)
        code, diff = await self.git("diff", "--cached") if staged else await self.git("diff", "HEAD")
        if code:  # a repo without commits has no HEAD yet
            diff = (await self.git("diff"))[1]
        untracked = "" if staged else "\n".join(line[3:] for line in status.splitlines() if line.startswith("??"))
        log = (await self.git("log", "--oneline", "-10"))[1]
        self.notify("Drafting a commit message…", title="commit")
        opts = ClaudeAgentOptions(
            cwd=self.cwd, cli_path=self.claude, env=store.account_env(self.session["account"]), model=SCOUT_MODEL,
            tools=[], setting_sources=[], strict_mcp_config=True, max_turns=2, extra_args=NO_HISTORY,
            output_format={"type": "json_schema", "schema": COMMIT_SCHEMA},
            system_prompt="You write precise, terse git commit messages.")
        prompt = (f"{COMMIT_PROMPT}\n\nRecent log:\n{log or '(no commits yet)'}\n\n"
                  f"New files:\n{untracked or '(none)'}\n\nDiff:\n{clip(diff, 20000)}")
        result = None
        try:
            async for m in query(prompt=prompt, options=opts):
                if isinstance(m, ResultMessage):
                    result = m
        except Exception as e:
            self.say("err", f"drafting the commit message failed: {e}")
        message = ((result and result.structured_output) or {}).get("message", "").strip()
        answers = await self.modal(Form({
            "title": "COMMIT", "question": f"{'Staged changes' if staged else 'All changes'}:\n\n```\n{files}\n```",
            "fields": [{"id": "message", "type": "textarea", "label": "Commit message", "default": message}]}))
        if not answers or not answers["message"].strip():
            return
        if not staged:
            await self.git("add", "-A")
        code, out = await self.git("commit", "-m", answers["message"].strip())
        self.say("err" if code else "note", out.splitlines()[0] if out else "committed")

    def change_folder(self, path):
        if not path or not os.path.isdir(path):
            path and self.notify("Not a folder.", severity="warning")
            return
        self.save_ui()
        self.cwd = os.path.abspath(path)
        self.set_plan_tasks([])
        self.new_session()  # no handover offer: it would land in the new folder
        self.load_plan_tasks(auto=True)

    def accounts_saved(self, result):
        if not result:
            return
        for account_id in result["removed"]:
            store.forget_token(account_id)
            self.cfg["usage"].pop(account_id, None)
        self.cfg.update(accounts=result["accounts"], auto_switch=result["auto_switch"], defaults=result["defaults"])
        store.save(self.cfg)
        if self.session["account"] in result["removed"]:
            self.switch_account(store.pick_account(self.cfg) or store.MAIN)
        self.refresh_bars()

    def switch_account(self, account_id, note=None):
        self.session["account"] = account_id
        self.refresh_bars()
        self.reconnect(note or f"switched to {store.nick(self.cfg, account_id)}")

    def note_usage(self, event, account_id):
        """A rate-limit event: remember the account's usage, and hand over to the next account past the limit."""
        if not account_id:
            return
        store.record_usage(self.cfg, account_id, event.rate_limit_info.raw)
        store.save(self.cfg)
        self.refresh_bars()
        auto, pct = self.cfg["auto_switch"], store.usage_pct(self.cfg, account_id) or 0
        if not auto["on"] or pct < auto["at"] or self.session["account"] != account_id:
            return  # under the limit, or a switch is already on its way
        nxt = store.pick_account(self.cfg, after=account_id)
        name = store.nick(self.cfg, account_id)
        if nxt:
            self.switch_account(nxt, f"{name} is at {pct}% of its limit: switching to "
                                     f"{store.nick(self.cfg, nxt)}, same conversation")
        elif not getattr(self, "warned_full", False):
            self.warned_full = True
            self.notify(f"{name} is at {pct}% and every other account is over {auto['at']}% too.",
                        severity="warning", timeout=10)

    def set_model(self, model):
        if self.tab == "ask":
            self.cfg["ask_model"] = model
            self.save_and_refresh()
            self.ask.started and self.ask.inbox.put_nowait(RECONNECT)
            return
        self.session["model"] = model
        self.refresh_bars()
        if self.code.client:
            self.safely(self.code.client.set_model(model or None), "switching model")

    def set_effort(self, effort):
        if effort != self.session["effort"]:
            self.session["effort"] = effort
            self.refresh_bars()
            self.reconnect(f"effort → {EFFORTS[effort]}")

    def set_mode(self, mode):
        self.cfg["mode"] = mode
        self.save_and_refresh()
        if self.code.client:
            self.safely(self.code.client.set_permission_mode(mode), "switching permission mode")

    def save_and_refresh(self):
        store.save(self.cfg)
        self.refresh_bars()

    def action_cycle_mode(self):
        if len(self.screen_stack) > 1:  # in a dialog shift+tab keeps its usual meaning
            self.screen.focus_previous()
            return
        mode = self.cfg["mode"]
        self.set_mode(MODE_CYCLE[(MODE_CYCLE.index(mode) + 1) % len(MODE_CYCLE)] if mode in MODE_CYCLE else "default")

    def action_escape(self):
        lane = self.lane
        if lane.busy and lane.client:
            self.armed = False  # interrupted: don't clear
            self.safely(lane.client.interrupt(), "interrupting")
            self.say("note", "interrupted.", lane)
        elif self.query_one("#resume").has_class("-on"):
            self.do("resume")


def main(argv):
    """claude-term [project-dir] [--resume session-id]"""
    args, resume = list(argv), None
    if "--resume" in args:
        i = args.index("--resume")
        resume = args[i + 1] if i + 1 < len(args) else None
        del args[i:i + 2]
    app = ClaudeTerm(os.path.abspath(args[0] if args else os.getcwd()), resume)
    app.run()
    if app.restart_cmd:
        # ponytail: Windows has no real exec, so this process waits for the new one (a few MB per restart)
        sys.exit(subprocess.call(app.restart_cmd))


if __name__ == "__main__":
    main(sys.argv[1:])
