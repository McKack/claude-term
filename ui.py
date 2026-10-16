"""Widgets, menus and dialogs for claude-term."""
import random
import time

from rich.markup import escape
from rich.table import Table
from rich.text import Text
from textual import events
from textual.app import ComposeResult
from textual.containers import Center, Grid, Horizontal, Vertical, VerticalScroll
from textual.message import Message
from textual.screen import ModalScreen
from textual.theme import Theme
from textual.widgets import (Button, Checkbox, Input, Label, Markdown, OptionList, RadioButton, RadioSet, Select,
                             SelectionList, Static, TextArea)
from textual.widgets.option_list import Option


def _theme(name, fg, **kw):
    # "text" pinned to the theme's own foreground, so replies are neon, not auto-contrast white;
    # the highlighted row in lists/menus is then inverted explicitly, or it'd be green on green
    bg = kw["background"]
    return Theme(name=name, foreground=fg, dark=True, **kw, variables={
        "text": fg, "block-cursor-foreground": bg, "block-cursor-background": fg,
        "block-cursor-blurred-foreground": bg, "block-cursor-blurred-background": kw["secondary"]})


THEMES = {
    "classic": Theme(name="classic", primary="#d97757", secondary="#8b93a7", accent="#f0a585",
                     foreground="#dfe3ea", background="#151821", surface="#1c2130", panel="#232a3b",
                     warning="#e5c07b", error="#e06c75", success="#8cc685", dark=True),
    "matrix": _theme("matrix", "#00ff41", primary="#00ff41", secondary="#00a82b", accent="#b6ffb6",
                     background="#000000", surface="#000c02", panel="#001a05",
                     warning="#ccff00", error="#ff3b3b", success="#39ff14"),
    "amber": _theme("amber", "#ffb000", primary="#ffb000", secondary="#b37b00", accent="#ffe0a0",
                    background="#0a0600", surface="#160d00", panel="#211400",
                    warning="#ffd966", error="#ff5500", success="#ffcc00"),
}
LOGO = " ▐▛███▜▌ \n▝▜█████▛▘\n  ▘▘ ▝▝  "  # Claude Code's crab; equal line widths keep it straight when centered
TIPS = [
    "Ctrl+R opens your recent sessions on the left. Enter resumes one.",
    "Shift+Tab cycles the permission mode: ask first → accept edits → auto → plan.",
    "Type / for command suggestions. ↑↓ to pick, Tab to complete.",
    "Alt+T, Alt+V, Alt+C and Alt+H open the menus. F10 works too.",
    "Ask Claude to “put the plan in the side panel”. It can draw there.",
    "Ctrl+T switches theme with a live preview.",
    "Ctrl+J adds a new line without sending.",
    "Ctrl+V pastes text, screenshots and file paths. Images become attachments.",
    "F2 flips between Code and Ask Claude, a side chat that never touches your files.",
    "Click a task in the Tasks panel to start it. ↻ rescans the project's plan files.",
]


def clock(seconds):
    s = int(max(0, seconds))
    return f"{s // 3600}:{s // 60 % 60:02d}:{s % 60:02d}" if s >= 3600 else f"{s // 60}:{s % 60:02d}"


class Chip(Static):
    """A clickable label. Posts Chip.Pressed; the app routes on chip id / data."""

    class Pressed(Message):
        def __init__(self, chip):
            super().__init__()
            self.chip = chip

    def __init__(self, label="", data=None, **kw):
        super().__init__(label, **kw)
        self.data = data

    def on_click(self, event: events.Click):
        event.stop()
        self.post_message(self.Pressed(self))


class MenuList(OptionList):
    """OptionList whose selection follows the mouse, like a desktop menu."""

    def _on_mouse_move(self, event: events.MouseMove):
        super()._on_mouse_move(event)
        i = event.style.meta.get("option")
        if i is not None and not self.get_option_at_index(i).disabled:
            self.highlighted = i


class PopupMenu(ModalScreen):
    """Dropdown at (x, y). items: (label, shortcut, action) | ("#", header) | None (separator).
    Dismisses with the action, "<" / ">" or "menu:<name>" to move to another menu, or None.
    bar: {menu name: Region} of the menu bar, so hovering or clicking another title switches menus.
    subs: {name: items} for "sub:<name>" entries; a submenu opens beside its entry, on top of this menu."""
    BINDINGS = [("escape", "dismiss(None)"), ("left", "dismiss('<')"), ("right", "right")]

    def __init__(self, items, x, y, above=False, name=None, bar=None, subs=None):
        super().__init__()
        self.items, self.x, self.y, self.above, self.menu_name, self.bar = items, x, y, above, name, bar or {}
        self.subs = subs or {}

    def open_sub(self, action):
        menu = self.query_one("#menu")
        row = next(i for i, it in enumerate(self.items) if it and it[-1] == action)

        def back(result):
            # left / esc / a click elsewhere: stay here. Anything else closes this menu too, once the
            # submenu is gone (closing now would pop the submenu's screen instead of this one)
            if result not in (None, "<", ">"):
                self.call_later(lambda: None if self.dismiss(result) else None)  # not awaited: own handler
        self.app.push_screen(PopupMenu(self.subs[action[4:]], menu.region.right, menu.region.y + 1 + row,
                                       name=self.menu_name, bar=self.bar), back)

    def action_right(self):
        menu = self.query_one("#menu")
        opt = menu.highlighted is not None and menu.get_option_at_index(menu.highlighted)
        self.open_sub(opt.id) if opt and (opt.id or "").startswith("sub:") else self.dismiss(">")

    @staticmethod
    def width_of(items):
        return max((len(i[0]) + len(i[1]) + 6 if i[0] != "#" else len(i[1]) + 2 for i in items if i), default=20)

    def compose(self) -> ComposeResult:
        width = self.width_of(self.items)
        opts = []
        for i in self.items:
            if i is None:
                opts.append(None)
            elif i[0] == "#":
                opts.append(Option(Text(i[1], style="dim"), disabled=True))
            else:
                label, key, action = i
                opts.append(Option(Text.assemble(label.ljust(width - len(key) - 3), (key, "dim")), id=action))
        menu = MenuList(*opts, id="menu")
        menu.styles.width = width + 2
        menu.styles.offset = (self.x, max(0, self.y - (len(opts) + 2 if self.above else 0)))
        yield menu

    def dismiss(self, result=None):
        # a fast mouse can queue a second close before this screen is gone; a second dismiss would
        # re-run the callback and pop the screen *below* (crash), so only the first one counts
        if getattr(self, "closing", False):
            return None
        self.closing = True
        return super().dismiss(result)

    def bar_hit(self, x, y):
        return next((n for n, r in self.bar.items() if r.contains(x, y)), None)

    def on_option_list_option_selected(self, e: OptionList.OptionSelected):
        self.open_sub(e.option.id) if e.option.id.startswith("sub:") else self.dismiss(e.option.id)

    def on_mouse_move(self, e: events.MouseMove):
        if (name := self.bar_hit(e.screen_x, e.screen_y)) and name != self.menu_name:
            self.dismiss(f"menu:{name}")

    def on_click(self, e: events.Click):
        if self.query_one("#menu").region.contains(e.screen_x, e.screen_y):
            return
        name = self.bar_hit(e.screen_x, e.screen_y)
        self.dismiss(f"menu:{name}" if name and name != self.menu_name else None)


class Prompt(TextArea):
    """Multi-line input: Enter sends, Ctrl+J / Shift+Enter add a line, pastes can become attachments."""

    class Submitted(Message):
        def __init__(self, text):
            super().__init__()
            self.text = text

    async def _on_key(self, event: events.Key):
        if self.app.slash_key(event.key):  # the / suggestion list wants ↑ ↓ tab enter
            event.prevent_default()
            event.stop()
        elif event.key == "enter":
            event.prevent_default()
            event.stop()
            self.post_message(self.Submitted(self.text))
        elif event.key in ("ctrl+j", "shift+enter"):
            event.prevent_default()
            event.stop()
            self.insert("\n")
        else:
            await super()._on_key(event)

    async def _on_paste(self, event: events.Paste):
        if self.app.handle_paste(event.text):  # an image or file paths: attach instead of typing
            event.prevent_default()
            event.stop()
        else:
            await super()._on_paste(event)


def stamp(when, now=None):
    """'00:22' today, '16 Oct 23:47' before that."""
    t, today = time.localtime(when), time.localtime(now or time.time())
    return time.strftime("%H:%M" if t[:3] == today[:3] else "%d %b %H:%M", t)


class Bubble(Horizontal):
    """A chat bubble: you on the right, Claude on the left, each with a round avatar and a timestamp below.
    The body is any widget (Static for you, a streaming Markdown for Claude)."""

    def __init__(self, body, who, avatar, when):
        super().__init__(classes=f"bubble -{who}")
        self.body, self.who, self.avatar, self.when = body, who, avatar, when

    def compose(self) -> ComposeResult:
        mine = self.who == "user"
        self.body.add_class("bubble-body")
        parts = [Static(self.avatar, classes="avatar"), Static("▶" if mine else "◀", classes="tail"),
                 Vertical(self.body, Static(stamp(self.when), classes="stamp"), classes="bubble-col")]
        yield from reversed(parts) if mine else parts


class Welcome(Vertical):
    def __init__(self, name, facts, suggestions):
        super().__init__(id="welcome")
        self.name_, self.facts, self.suggestions = name, facts, suggestions

    def compose(self) -> ComposeResult:
        yield Static(LOGO, id="logo")
        yield Static(f"Hi, {self.name_}" if self.name_ else "Hi", id="hello")
        yield Static(self.facts, id="facts")
        with Center(), Grid(id="chips"):
            for label, action in self.suggestions:
                yield Chip(label, data=action, classes="suggest")
        with Center(), Vertical(id="tip"):
            yield Chip("↻", id="tip-next")
            yield Static("✦ Did you know?", id="tip-title")
            yield Static(random.choice(TIPS), id="tip-text")

    def next_tip(self):
        self.query_one("#tip-text", Static).update(random.choice(TIPS))


class InstallCard(Vertical):
    """First run without the CLI: install it, sign in, continue."""

    def compose(self) -> ComposeResult:
        yield Static(LOGO, id="logo")
        yield Static("Claude Code isn't installed yet", id="hello")
        yield Static("claude-term runs on top of the official Claude Code CLI. Install it here with "
                     "Anthropic's installer (claude.ai/install.ps1), then sign in once.", id="facts")
        with Center(), Grid(id="chips"):
            yield Chip("⤓  Install Claude Code", data="install_claude", classes="suggest", id="install-btn")
            yield Chip("Sign in, then continue", data="sign_in", classes="suggest", id="signin-btn")
        with Center(), Vertical(id="tip"):
            yield Static("Click Install to start.", id="tip-text")

    def log(self, text):
        self.query_one("#tip-text", Static).update(Text(text))


class TaskPanel(Vertical):
    """Tasks: in progress (with a running timer), up next, finished. Rows are clickable."""
    SECTIONS = (("in_progress", "IN PROGRESS"), ("pending", "UP NEXT"), ("completed", "FINISHED"))

    def compose(self) -> ComposeResult:
        with Horizontal(id="tasks-head"):
            yield Static("TASKS", id="tasks-title")
            yield Static(id="tasks-status")
            yield Chip("↻", id="tasks-refresh")
            yield Chip("»", id="side-close")
        yield VerticalScroll(id="tasks-list")

    def show(self, tasks, now):
        box = self.query_one("#tasks-list")
        box.remove_children()
        rows = []
        for status, title in self.SECTIONS:
            group = [t for t in tasks if t["status"] == status]
            if group:
                rows.append(Static(title, classes="task-head"))
                rows += [Chip(self.line(t, now), data=t["key"], classes=f"task -{status}") for t in group]
        box.mount_all(rows or [Static("No tasks yet. ↻ scans this project's plan files.", classes="task-empty")])

    def tick(self, tasks, now):
        by_key = {t["key"]: t for t in tasks}
        for row in self.query(".task.-in_progress"):
            if t := by_key.get(row.data):
                row.update(self.line(t, now))

    @staticmethod
    def line(t, now):
        status = t["status"]
        mark = {"in_progress": "▶", "completed": "✔"}.get(status, "◇" if t.get("source") else "○")
        label = t.get("active") if status == "in_progress" and t.get("active") else t["title"]
        right = ""
        if t.get("started"):
            right = clock((t.get("ended") or now) - t["started"])
        grid = Table.grid(expand=True)
        grid.add_column(ratio=1)
        grid.add_column(justify="right", no_wrap=True)
        grid.add_row(Text(f"{mark} {label}"), Text(f" {right}" if right else ""))
        return grid


class ResumePanel(Vertical):
    """Left drawer: recent sessions for this folder, filterable."""

    class Picked(Message):
        def __init__(self, session_id):
            super().__init__()
            self.session_id = session_id

    def compose(self) -> ComposeResult:
        with Horizontal(id="resume-head"):
            yield Static("RECENT SESSIONS", id="resume-title")
            yield Chip("«", id="resume-close")
        yield Input(placeholder="filter…", id="resume-filter")
        yield OptionList(id="resume-list")

    def show(self, sessions, ago):
        self.sessions, self.ago = sessions, ago
        self.fill("")

    def fill(self, needle):
        lst = self.query_one("#resume-list", OptionList)
        lst.clear_options()
        for s in self.sessions:
            title = s.custom_title or s.summary or s.first_prompt or "(untitled)"
            if needle.lower() not in title.lower():
                continue
            meta = " · ".join(x for x in (self.ago(s.last_modified), s.git_branch) if x)
            lst.add_option(Option(Text.assemble((title[:60], "bold"), "\n", (meta, "dim")), id=s.session_id))
        if not lst.option_count:
            lst.add_option(Option(Text("no sessions here yet", style="dim"), disabled=True))

    def on_input_changed(self, e: Input.Changed):
        self.fill(e.value)

    def on_input_submitted(self, e: Input.Submitted):
        self.query_one("#resume-list").focus()

    def on_option_list_option_selected(self, e: OptionList.OptionSelected):
        e.stop()
        self.post_message(self.Picked(e.option.id))


class Ask(ModalScreen[bool]):
    BINDINGS = [("y", "dismiss(True)", "Allow"), ("n,escape", "dismiss(False)", "Deny")]

    def __init__(self, tool_name, body):
        super().__init__(classes="dialog")
        self.tool_name, self.body = tool_name, body

    def compose(self) -> ComposeResult:
        with Vertical(id="dlg") as d:
            d.border_title = "⚠ PERMISSION REQUEST"
            yield Label(f"Claude wants to run [b]{escape(self.tool_name)}[/]")
            yield Static(self.body, id="dlg-body")
            with Horizontal(id="dlg-btns"):
                yield Button("[Y] Allow", variant="success", id="yes")
                yield Button("[N] Deny", variant="error", id="no")

    def on_button_pressed(self, e: Button.Pressed):
        self.dismiss(e.button.id == "yes")


class Confirm(ModalScreen[bool]):
    BINDINGS = [("y,enter", "dismiss(True)"), ("n,escape", "dismiss(False)")]

    def __init__(self, title, question, body="", yes="Start"):
        super().__init__(classes="dialog")
        self.title_, self.question, self.body, self.yes = title, question, body, yes

    def compose(self) -> ComposeResult:
        with Vertical(id="dlg") as d:
            d.border_title = self.title_
            yield Label(self.question)
            if self.body:
                yield Static(self.body if isinstance(self.body, Text) else Text(self.body), id="dlg-body")
            with Horizontal(id="dlg-btns"):
                yield Button("[N] Not now", id="no")
                yield Button(f"[Y] {self.yes}", variant="success", id="yes")

    def on_button_pressed(self, e: Button.Pressed):
        self.dismiss(e.button.id == "yes")


class Choose(ModalScreen[str | None]):
    """One AskUserQuestion question: pick option(s) or type your own answer."""
    BINDINGS = [("escape", "dismiss(None)", "Cancel")]

    def __init__(self, q):
        super().__init__(classes="dialog")
        self.q, self.multi = q, bool(q.get("multiSelect"))

    def compose(self) -> ComposeResult:
        opts = self.q.get("options", [])
        rows = [Text.assemble((o["label"], "bold"), f"  {o.get('description', '')}") for o in opts]
        with Vertical(id="dlg") as d:
            d.border_title = f"? {self.q.get('header') or 'QUESTION'}"
            yield Label(escape(self.q["question"]))
            if self.multi:
                yield SelectionList(*[(r, o["label"]) for r, o in zip(rows, opts)], id="pick")
            else:
                yield OptionList(*rows, id="pick")
            yield Input(placeholder="or type your own answer…", id="other")
            if self.multi:
                with Horizontal(id="dlg-btns"):
                    yield Button("OK", variant="success", id="ok")

    def on_option_list_option_selected(self, e: OptionList.OptionSelected):
        self.dismiss(self.q["options"][e.option_index]["label"])

    def on_input_submitted(self, e: Input.Submitted):
        self.finish() if self.multi else self.dismiss(e.value.strip() or None)

    def on_button_pressed(self, e: Button.Pressed):
        self.finish()

    def finish(self):
        other = self.query_one("#other", Input).value.strip()
        picks = list(self.query_one("#pick", SelectionList).selected) + ([other] if other else [])
        self.dismiss(", ".join(picks) or None)


class ThemePicker(ModalScreen[str | None]):
    """Live preview while moving; Enter keeps it, Esc reverts."""
    BINDINGS = [("escape", "cancel")]

    def __init__(self, current):
        super().__init__(classes="dialog")
        self.original = current

    def compose(self) -> ComposeResult:
        with Vertical(id="dlg") as d:
            d.border_title = "THEME"
            names = list(THEMES)
            lst = OptionList(*[Option(n.title(), id=n) for n in names], id="pick")
            lst.highlighted = names.index(self.original) if self.original in names else 0
            yield lst
            yield Static("↑↓ preview · enter keep · esc cancel", classes="hint")

    def on_option_list_option_highlighted(self, e: OptionList.OptionHighlighted):
        self.app.apply_theme(e.option.id, save=False)

    def on_option_list_option_selected(self, e: OptionList.OptionSelected):
        self.dismiss(e.option.id)

    def action_cancel(self):
        self.app.apply_theme(self.original, save=False)
        self.dismiss(None)


class TokenInput(Input):
    """A console wraps the long token, so a copied one arrives in pieces: join them instead of keeping line one."""

    def _on_paste(self, event: events.Paste):
        event.prevent_default()  # else Input's own handler pastes line one as well
        event.stop()
        self.insert_text_at_cursor("".join(event.text.split()))


class AddAccount(ModalScreen[tuple[str, str] | None]):
    """Runs `claude setup-token` in a pseudo-console and picks the token up by itself; pasting still works."""
    DEFAULT_CLASSES = "dialog"
    BINDINGS = [("escape", "dismiss(None)")]

    def __init__(self, cli="claude"):
        super().__init__()
        self.cli, self.proc = cli, None

    def compose(self) -> ComposeResult:
        with Vertical(id="dlg") as d:
            d.border_title = "ADD ACCOUNT"
            yield Static("1. In your browser, sign in to the [b]other[/] account first (the browser's current "
                         "account is the one that gets authorized).\n2. Press Get token. claude-term runs "
                         "[b]claude setup-token[/] and picks the token up when the browser is done.\n"
                         "The token is stored in Windows Credential Manager, never in a file.")
            yield Button("Get token", id="signin")
            yield Static(id="st-status", classes="hint")
            yield Input(placeholder="paste the code here if the page shows one", id="st-code")
            yield Input(placeholder="nickname, e.g. Work", id="acct-name")
            yield TokenInput(placeholder="token (filled in for you, or paste sk-ant-oat…)", password=True,
                             id="acct-token")
            with Horizontal(id="dlg-btns"):
                yield Button("Cancel", id="cancel")
                yield Button("Save", variant="success", id="save")

    def on_mount(self):
        self.query_one("#st-code").display = False

    def on_unmount(self):
        if self.proc and self.proc.isalive():
            self.proc.terminate(force=True)

    def status(self, text):
        self.query_one("#st-status", Static).update(text)

    def on_button_pressed(self, e: Button.Pressed):
        if e.button.id == "signin":
            self.get_token()
        elif e.button.id == "cancel":
            self.dismiss(None)
        else:
            self.save()

    def get_token(self):
        from store import read_setup_token, setup_token_pty
        if self.proc and self.proc.isalive():
            return
        try:
            self.proc = setup_token_pty(self.cli)
        except Exception as e:  # no pywinpty, or claude missing: the old way, a console window to copy from
            from store import open_setup_token_window
            open_setup_token_window()
            self.status(f"Couldn't run it here ({e}); copy the token from the window that opened.")
            return
        self.status("Starting claude setup-token…")

        def show_url(url):
            self.app.call_from_thread(self.status, f"Waiting for the browser…  Didn't open? [@click=screen.open_url"
                                      f"('{escape(url)}')]Open the sign-in page[/] (try a private window for "
                                      f"another account)")
            self.app.call_from_thread(lambda: setattr(self.query_one("#st-code"), "display", True))

        def run():
            token = read_setup_token(self.proc, show_url)
            self.app.call_from_thread(self.got_token, token)
        self.run_worker(run, thread=True, exit_on_error=False)

    def action_open_url(self, url):
        self.app.open_url(url)

    def got_token(self, token):
        if not token:
            self.status("setup-token ended without a token. Try again, or paste one.")
            return
        self.query_one("#acct-token", Input).value = token
        self.query_one("#st-code").display = False
        self.status("✓ Token received. Give the account a nickname and Save.")
        self.query_one("#acct-name", Input).focus()

    def on_input_submitted(self, e: Input.Submitted):
        if e.input.id == "st-code":  # the page's code goes to setup-token, as if typed there
            if self.proc and self.proc.isalive() and e.value.strip():
                self.proc.write(e.value.strip() + "\r")
                e.input.value = ""
                self.status("Code sent, waiting for the token…")
        else:
            self.save()

    def save(self):
        name = self.query_one("#acct-name", Input).value.strip()
        token = "".join(self.query_one("#acct-token", Input).value.split())
        if name and token:
            self.dismiss((name, token))
        else:
            self.notify("Need both a name and a token.", severity="warning")


class AccountManager(ModalScreen[dict | None]):
    """Accounts in priority order (nickname, usage, ↑ ↓ ×), auto-switch threshold, defaults for new sessions.
    Dismisses with {"accounts", "auto_switch", "defaults", "removed"} or None."""
    DEFAULT_CLASSES = "dialog"
    BINDINGS = [("escape", "cancel")]

    def __init__(self, cfg, usage, models, efforts, current):
        super().__init__()
        self.accounts = [dict(a) for a in cfg["accounts"]]
        self.auto, self.defaults = dict(cfg["auto_switch"]), dict(cfg["defaults"])
        self.usage, self.models, self.efforts, self.current = usage, models, efforts, current
        self.cfg, self.added, self.removed = cfg, [], []

    def compose(self) -> ComposeResult:
        with Vertical(id="dlg") as d:
            d.border_title = "ACCOUNTS"
            yield Static("Priority order: new sessions use the first account that's under the limit.", classes="hint")
            yield Vertical(id="acct-rows")
            yield Button("+ Add account", id="acct-add")
            yield Static("AUTO-SWITCH", classes="section")
            with Horizontal(classes="row"):
                yield Checkbox("Move to the next account when usage reaches", self.auto["on"], id="auto-on")
                yield Input(str(self.auto["at"]), type="integer", max_length=3, compact=True, id="auto-at")
                yield Static("% (5-hour or weekly)", classes="hint")
            yield Static("DEFAULTS FOR NEW SESSIONS  ·  the chat menu overrides them per session", classes="section")
            with Horizontal(classes="row"):
                yield Select([(label, m) for m, label in self.models.items()], value=self.defaults["model"],
                             allow_blank=False, id="def-model")
                yield Select([(f"Effort: {label}", e) for e, label in self.efforts.items()],
                             value=self.defaults["effort"], allow_blank=False, id="def-effort")
                yield Checkbox("Web search", self.defaults["web"], id="def-web")
            with Horizontal(id="dlg-btns"):
                yield Button("Cancel", id="cancel")
                yield Button("Save", variant="success", id="save")

    async def on_mount(self):
        await self.draw_rows()

    async def draw_rows(self):
        box = self.query_one("#acct-rows")
        await box.remove_children()
        rows = []
        for i, a in enumerate(self.accounts):
            main = a["id"] == "Main"
            note = self.usage.get(a["id"], "usage not seen yet") + ("  · CLI login" if main else "") \
                + ("  · in use" if a["id"] == self.current else "")
            rows.append(Horizontal(
                Static(f"{i + 1}.", classes="acct-rank"),
                Input(a["nick"], name=f"nick:{i}", compact=True, classes="acct-nick"),
                Static(note, classes="acct-usage"),
                Chip("↑", data=("up", i), classes="acct-btn"),
                Chip("↓", data=("down", i), classes="acct-btn"),
                Chip(" " if main else "×", data=None if main else ("del", i), classes="acct-btn"),
                classes="acct-row"))
        await box.mount_all(rows)

    def on_input_changed(self, e: Input.Changed):
        if (e.input.name or "").startswith("nick:"):
            self.accounts[int(e.input.name[5:])]["nick"] = e.value.strip() or self.accounts[int(e.input.name[5:])]["id"]

    async def on_chip_pressed(self, e: Chip.Pressed):
        e.stop()
        if not e.chip.data:
            return
        op, i = e.chip.data
        if op == "del":
            self.removed.append(self.accounts.pop(i)["id"])
        else:
            j = i - 1 if op == "up" else i + 1
            if 0 <= j < len(self.accounts):
                self.accounts[i], self.accounts[j] = self.accounts[j], self.accounts[i]
        await self.draw_rows()

    def on_button_pressed(self, e: Button.Pressed):
        if e.button.id == "acct-add":
            self.app.push_screen(AddAccount(getattr(self.app, "claude", None) or "claude"), self.added_account)
        elif e.button.id == "cancel":
            self.action_cancel()
        elif e.button.id == "save":
            self.save()

    def added_account(self, result):
        if result:
            from store import store_token
            entry = store_token({"accounts": self.accounts}, *result)
            self.accounts.append(entry)
            self.added.append(entry["id"])
            self.call_later(self.draw_rows)

    def action_cancel(self):
        from store import forget_token
        for account_id in self.added:  # tokens saved while the dialog was open but never kept
            forget_token(account_id)
        self.dismiss(None)

    def save(self):
        try:
            at = int(self.query_one("#auto-at", Input).value)
        except ValueError:
            at = 0
        if not 1 <= at <= 100:
            self.notify("The switch threshold must be between 1 and 100 %.", severity="warning")
            return
        self.dismiss({
            "accounts": self.accounts, "removed": self.removed,
            "auto_switch": {"on": self.query_one("#auto-on", Checkbox).value, "at": at},
            "defaults": {"model": self.query_one("#def-model", Select).value,
                         "effort": self.query_one("#def-effort", Select).value,
                         "web": self.query_one("#def-web", Checkbox).value}})


class AskText(ModalScreen[str | None]):
    """One-line text prompt, e.g. a folder path."""
    BINDINGS = [("escape", "dismiss(None)")]

    def __init__(self, title, value=""):
        super().__init__(classes="dialog")
        self.title_, self.value = title, value

    def compose(self) -> ComposeResult:
        with Vertical(id="dlg") as d:
            d.border_title = self.title_
            yield Input(value=self.value, id="text")

    def on_input_submitted(self, e: Input.Submitted):
        self.dismiss(e.value.strip() or None)


class Form(ModalScreen[dict | None]):
    """A question Claude asks with a form: markdown (links open on click), then radio / checkbox / select / text /
    switch fields. Recommended choices are starred and pre-selected, so Ctrl+S alone accepts the suggestion.
    Dismisses with {field id: value, "note": text}, or None when cancelled."""
    BINDINGS = [("escape", "dismiss(None)", "Cancel"), ("ctrl+s", "confirm", "Confirm")]

    def __init__(self, spec):
        super().__init__(classes="dialog")
        self.spec, self.fields = spec, spec.get("fields") or []

    @staticmethod
    def options(f):
        return [o if isinstance(o, dict) else {"value": str(o)} for o in f.get("options") or []]

    @staticmethod
    def starts(f, value):  # selected at first: the default, else the recommendation
        start = f.get("default", f.get("recommended"))
        return value in start if isinstance(start, list) else value == start

    @staticmethod
    def label(f, o):
        rec = f.get("recommended")
        star = o["value"] in rec if isinstance(rec, list) else o["value"] == rec
        return Text.assemble(("★ ", "bold") if star else "  ", (str(o.get("label") or o["value"]), "bold"),
                             f"  {o['description']}" if o.get("description") else "",
                             ("  recommended", "italic") if star else "")

    def compose(self) -> ComposeResult:
        with VerticalScroll(id="dlg", classes="form") as d:
            d.border_title = f"? {self.spec.get('title') or 'QUESTION'}"
            yield Markdown(self.spec.get("question", ""), id="form-q")
            for i, f in enumerate(self.fields):
                kind, opts = f.get("type", "radio"), self.options(f)
                if kind != "switch":
                    yield Label(Text(f.get("label", "")), classes="section")
                if kind == "radio":
                    with RadioSet(id=f"f{i}"):
                        for o in opts:
                            yield RadioButton(self.label(f, o), self.starts(f, o["value"]))
                elif kind == "checkbox":
                    for j, o in enumerate(opts):
                        yield Checkbox(self.label(f, o), self.starts(f, o["value"]), id=f"f{i}-{j}")
                elif kind == "select":
                    start = next((o["value"] for o in opts if self.starts(f, o["value"])), Select.NULL)
                    yield Select([(self.label(f, o), o["value"]) for o in opts], value=start, id=f"f{i}")
                elif kind == "switch":
                    yield Checkbox(Text(f.get("label", "")), bool(f.get("default", f.get("recommended"))), id=f"f{i}")
                elif kind == "textarea":
                    area = TextArea(str(f.get("default") or f.get("recommended") or ""), id=f"f{i}", classes="form-text",
                                    soft_wrap=True, show_line_numbers=False)
                    if f.get("rows"):  # room up front for a longer note
                        area.styles.min_height = f["rows"]
                    yield area
                else:  # text
                    yield Input(str(f.get("default") or f.get("recommended") or ""), placeholder=f.get("placeholder", ""),
                                id=f"f{i}")
                if f.get("help"):
                    yield Static(Text(f["help"]), classes="hint")
            if self.spec.get("note", True):
                yield Input(placeholder="anything else to tell Claude? (optional)", id="form-note")
            with Horizontal(id="dlg-btns"):
                yield Button("Cancel", id="cancel")
                yield Button(f"{self.spec.get('ok') or 'Confirm'}  ctrl+s", variant="success", id="ok")

    def values(self):
        out = {}
        for i, f in enumerate(self.fields):
            kind, opts, key = f.get("type", "radio"), self.options(f), f.get("id") or f.get("label") or f"field{i}"
            if kind == "radio":
                idx = self.query_one(f"#f{i}", RadioSet).pressed_index
                out[key] = opts[idx]["value"] if idx >= 0 else None
            elif kind == "checkbox":
                out[key] = [o["value"] for j, o in enumerate(opts) if self.query_one(f"#f{i}-{j}", Checkbox).value]
            elif kind == "select":
                v = self.query_one(f"#f{i}", Select).value
                out[key] = None if v == Select.NULL else v
            else:
                w = self.query_one(f"#f{i}")
                out[key] = w.text if isinstance(w, TextArea) else w.value
        out["note"] = next((w.value.strip() for w in self.query("#form-note").results(Input)), "")
        return out

    def action_confirm(self):
        self.dismiss(self.values())

    def on_input_submitted(self, e: Input.Submitted):
        self.action_confirm()

    def on_button_pressed(self, e: Button.Pressed):
        self.action_confirm() if e.button.id == "ok" else self.dismiss(None)
