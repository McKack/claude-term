"""Clauracle: a small, project-first desktop front for Claude Code.

Every project gets its own `claude` process (through claude-agent-sdk) with that project's Claude account and
GitHub account in its environment, so switching projects switches accounts and nothing gets logged out.
"""
import asyncio
import json
import os
import re
import sys
import threading
import uuid
import webbrowser
from pathlib import Path

import webview
from claude_agent_sdk import (AssistantMessage, ClaudeAgentOptions, ClaudeSDKClient, PermissionResultAllow,
                              PermissionResultDeny, RateLimitEvent, ResultMessage, TextBlock, ToolResultBlock,
                              ToolUseBlock, UserMessage, create_sdk_mcp_server, tool)

import store

HERE = Path(getattr(sys, "_MEIPASS", Path(__file__).parent))
PLUGIN = HERE / "plugin"
PONYTAIL = re.sub(r"^---.*?---\s*", "", (PLUGIN / "skills/ponytail/SKILL.md").read_text(encoding="utf-8"), flags=re.S)
APPEND = ("You are running inside Clauracle, a desktop app around Claude Code. When the user turns on the "
          "clear-continue loop, or Clauracle asks for a handover, mcp__clauracle__clear_and_continue ends this session "
          "and starts a fresh one that resumes from HANDOVER.md.")
LOOP_LIMIT = 80  # % of the 5-hour or weekly window; at or past it the loop stops
CONTEXT_LIMIT = 50  # % of the context window; past it (setting on) the session hands over and starts fresh
HANDOVER = """Pause here; don't start anything new{why}.
1. Update HANDOVER.md at the project root (create it if missing; in a git repo where it isn't tracked, add it to
   .git/info/exclude) so a fresh session can pick up exactly where you are: the goal, what you were doing and how far it
   got, what is verified and what isn't, the next steps in order, gotchas learned this session.
   Finished tasks go to CHANGES.md (newest first, one line each), not the handover; keep HANDOVER.md under ~50 lines.
2. Then call mcp__clauracle__clear_and_continue as the LAST action of this turn."""
CONTINUE = ("continue\n\nClauracle cleared the context. Read HANDOVER.md at the project root and resume exactly where "
            "the previous session paused.")
LOOP_RULES = """continue

# clear-continue is ON (the user's hands-off loop)
The user is away. Work ONE task per session, keep a handover a fresh session can resume from, and hand over.
1. Find the project's handover and do its next task. If CLAUDE.md defines a continue procedure or a task queue, follow
   that. Otherwise read HANDOVER.md at the project root and take the FIRST open task of its queue. Do only that task.
2. No handover yet? Create HANDOVER.md from what you know: goal, current state, a "## Queue" of ordered open tasks,
   how to build and test. In a git repo where it isn't tracked, add it to .git/info/exclude.
3. Finishing a task: verify it, update the handover (task ticked off, what was verified, next task first), add a line
   to CHANGES.md (newest first), commit if the project's rules allow it, then call
   mcp__clauracle__clear_and_continue as the LAST action of the turn.
4. Do NOT call it, stop and say why instead, when: the queue is empty, the next step is the user's decision,
   something failed that you cannot fix, or the tool answered "NOT cleared"."""
RECONNECT = object()


def summary(name, args):
    """One line for a tool call: the command, the file or the pattern."""
    for key in ("command", "file_path", "path", "pattern", "url", "query", "description", "prompt"):
        if isinstance(args.get(key), str):
            return args[key].splitlines()[0][:160] if args[key] else ""
    return json.dumps(args)[:160]


class Session:
    """One project's Claude Code conversation. Lives on the app's asyncio loop."""

    def __init__(self, app, pid):
        self.app, self.pid = app, pid
        self.inbox, self.client, self.busy = asyncio.Queue(), None, False
        self.armed = self.handing_over = False  # clear_and_continue called / Clauracle asked for a handover
        self.first_turn = True  # a fresh session already past the context limit must not hand over forever
        self.task = asyncio.ensure_future(self.run())

    @property
    def p(self):
        return store.project(self.app.cfg, self.pid)

    def emit(self, event, **data):
        self.app.emit(event, pid=self.pid, **data)

    def options(self):
        p, cfg = self.p, self.app.cfg
        off = cfg["off"] + store.account(cfg, p["account"])["off"]  # Settings > Connections & Skills / AI accounts
        return ClaudeAgentOptions(
            cwd=p["path"], cli_path=store.cli("claude"),  # the installed claude: same commands, skills, MCP, hooks
            env={**store.account_env(p["account"]), **store.gh_env(p["gh"])},
            system_prompt={"type": "preset", "preset": "claude_code",
                           "append": APPEND + ("\n\n" + PONYTAIL if p["ponytail"] else "")},
            plugins=[{"type": "local", "path": str(PLUGIN)}],
            model=p["model"] or None, permission_mode=p["mode"], resume=p["session"],
            disallowed_tools=store.deny_rules(off),
            mcp_servers={"clauracle": self.tools()}, can_use_tool=self.can_use_tool)

    async def run(self):
        while True:
            try:
                async with ClaudeSDKClient(self.options()) as client:
                    self.client = client
                    self.emit("status", busy=False, connected=True)
                    while (item := await self.inbox.get()) is not RECONNECT:
                        self.busy = True
                        self.emit("status", busy=True)
                        await client.query(item)
                        async for m in client.receive_response():
                            self.show(m)
                        self.busy = False
                        self.emit("status", busy=False)
                        await self.after_turn(client)
            except asyncio.CancelledError:
                raise
            except Exception as e:  # keep the app alive and reconnect
                self.client, self.busy = None, False
                self.emit("line", kind="err", text=f"claude error: {e} (retrying in 5s)")
                self.emit("status", busy=False, connected=False)
                await asyncio.sleep(5)

    def show(self, m):
        if isinstance(m, AssistantMessage):
            for b in m.content:
                if isinstance(b, TextBlock) and b.text.strip():
                    self.emit("line", kind="claude", text=b.text)
                elif isinstance(b, ToolUseBlock) and not b.name.startswith("mcp__clauracle__"):
                    self.emit("line", kind="tool", text=f"{b.name}  {summary(b.name, b.input)}")
        elif isinstance(m, UserMessage) and isinstance(m.content, list):
            for b in m.content:
                if isinstance(b, ToolResultBlock) and b.is_error:
                    self.emit("line", kind="err", text=str(b.content)[:300])
        elif isinstance(m, ResultMessage):
            self.p["session"] = m.session_id
            self.app.save()
            if m.is_error:
                self.emit("line", kind="err", text=m.result or "the turn ended with an error")
        elif isinstance(m, RateLimitEvent):
            store.record_usage(self.app.cfg, self.p["account"], m.rate_limit_info.raw)
            self.app.save()
            self.app.emit("accounts")

    async def after_turn(self, client):
        if self.armed:
            return self.fresh_session(LOOP_RULES if self.p["loop"] else CONTINUE)
        if self.handing_over:
            self.handing_over = False
            self.emit("line", kind="note", text="No handover came back, so the context wasn't cleared.")
        try:
            pct = (await client.get_context_usage())["percentage"]
        except Exception:
            return
        self.emit("status", context=round(pct))
        if self.p["auto_clear"] and not self.first_turn and pct >= CONTEXT_LIMIT:
            self.hand_over(f": the context window is at {round(pct)}%")
        self.first_turn = False

    def hand_over(self, why=""):
        """Have Claude write the handover; its clear_and_continue call then starts the fresh session."""
        self.handing_over = True
        self.emit("line", kind="note", text="Writing a handover, then a fresh session picks it up.")
        self.send_first(HANDOVER.format(why=why))

    def send_first(self, *items):
        """Put items ahead of anything you queued meanwhile (that waits for them)."""
        held = []
        while not self.inbox.empty():
            held.append(self.inbox.get_nowait())
        for item in (*items, *held):
            self.inbox.put_nowait(item)

    def fresh_session(self, first_message=None):
        self.armed = self.handing_over = False
        self.first_turn, self.p["session"] = True, None
        self.app.save()
        self.emit("cleared")
        self.send_first(RECONNECT, *([first_message] if first_message else []))

    def tools(self):
        def ok(text):
            return {"content": [{"type": "text", "text": text}]}

        @tool("clear_and_continue", "Only while the clear-continue loop is on, or when Clauracle asked for a handover: "
              "call as the LAST action of a turn, once the handover is updated. When the turn ends Clauracle starts a "
              f"fresh session that resumes from it. Refused when the loop is off, or at {LOOP_LIMIT}% usage.", {})
        async def clear_and_continue(a):
            if self.handing_over:
                self.armed = True
                return ok("Armed. When this turn ends a fresh session resumes from the handover. End the turn now.")
            if not self.p["loop"]:
                return ok("clear-continue is OFF: nothing will be cleared. Just end the turn.")
            if (pct := store.usage_pct(self.app.cfg, self.p["account"]) or 0) >= LOOP_LIMIT:
                self.app.update(self.pid, {"loop": False})
                return ok(f"NOT cleared: usage at {pct}% (limit {LOOP_LIMIT}%). The loop is now off. Stop and tell the user.")
            self.armed = True
            return ok("Armed. When this turn ends: fresh session, then \"continue\". End the turn now.")
        self.clear_and_continue = clear_and_continue  # the test calls .handler directly
        return create_sdk_mcp_server("clauracle", tools=[clear_and_continue])

    async def can_use_tool(self, name, args, ctx):
        if name.startswith("mcp__clauracle__"):
            return PermissionResultAllow()
        answer = await self.app.ask(self.pid, name, args)
        if not answer.get("allow"):
            return PermissionResultDeny(message=answer.get("note") or "The user denied this.")
        if name == "AskUserQuestion":
            return PermissionResultAllow(updated_input={**args, "answers": answer.get("answers", {})})
        return PermissionResultAllow()


class App:
    def __init__(self):
        self.cfg = store.load()
        self.sessions, self.pending, self.window = {}, {}, None
        self.loop = asyncio.new_event_loop()
        threading.Thread(target=self.loop.run_forever, daemon=True).start()

    def save(self):
        store.save(self.cfg)

    def emit(self, event, **data):
        if self.window:
            self.window.evaluate_js(f"window.onEvent && onEvent({json.dumps({'event': event, **data})})")

    def on_loop(self, fn, *args):
        self.loop.call_soon_threadsafe(fn, *args)

    def session(self, pid):
        """The project's session, started on first use (call on the loop)."""
        if pid not in self.sessions:
            self.sessions[pid] = Session(self, pid)
        return self.sessions[pid]

    def update(self, pid, changes):
        p = store.project(self.cfg, pid)
        reconnect = any(p.get(k) != v for k, v in changes.items() if k in ("account", "gh", "ponytail", "model", "mode"))
        p.update(changes)
        self.save()
        self.emit("project", project=p)
        if reconnect:
            self.reconnect(lambda s: s.pid == pid)

    def reconnect(self, which):
        """Restart the matching sessions with fresh options, after their current reply."""
        for s in list(self.sessions.values()):
            if which(s):
                self.on_loop(s.send_first, RECONNECT)

    async def ask(self, pid, tool_name, args):
        rid = uuid.uuid4().hex
        self.pending[rid] = fut = self.loop.create_future()
        self.emit("ask", pid=pid, id=rid, tool=tool_name, input=args)
        try:
            return await fut
        finally:
            self.pending.pop(rid, None)


class Api:
    """What the page calls (window.pywebview.api.*). Each call runs on its own thread."""

    def __init__(self, app):
        self._app = app

    @property
    def _cfg(self):
        return self._app.cfg

    def state(self):
        usage = {a["id"]: store.usage_pct(self._cfg, a["id"]) for a in self._cfg["accounts"]}
        return {"projects": self._cfg["projects"], "accounts": self._cfg["accounts"], "usage": usage,
                "active": self._cfg["active"], "claude": bool(store.cli("claude")), "gh": bool(store.cli("gh"))}

    def open_project(self, pid):
        self._cfg["active"] = pid
        self._app.save()
        self._app.on_loop(self._app.session, pid)
        p = store.project(self._cfg, pid)
        s = self._app.sessions.get(pid)
        return {"branch": store.git_branch(p["path"]), "busy": bool(s and s.busy)}

    def add_project(self):
        picked = self._app.window.create_file_dialog(webview.FileDialog.FOLDER)
        if not picked:
            return None
        p = store.new_project(self._cfg, picked[0])
        self._app.save()
        return p

    def remove_project(self, pid):
        self._cfg["projects"] = [p for p in self._cfg["projects"] if p["id"] != pid]
        self._app.save()
        if s := self._app.sessions.pop(pid, None):
            self._app.on_loop(s.task.cancel)

    def update_project(self, pid, changes):
        self._app.update(pid, changes)

    def send(self, pid, text):
        def go():
            s = self._app.session(pid)
            if text.strip() == "/clear":
                s.fresh_session()
            else:
                s.inbox.put_nowait(text)
        self._app.on_loop(go)

    def interrupt(self, pid):
        if (s := self._app.sessions.get(pid)) and s.client:
            asyncio.run_coroutine_threadsafe(s.client.interrupt(), self._app.loop)

    def new_session(self, pid):
        self._app.on_loop(lambda: self._app.session(pid).fresh_session())

    def clear_continue(self, pid):
        self._app.on_loop(lambda: self._app.session(pid).hand_over())

    def set_loop(self, pid, on):
        self._app.update(pid, {"loop": on})
        if on:
            self._app.on_loop(lambda: self._app.session(pid).inbox.put_nowait(LOOP_RULES))

    def answer(self, rid, answer):
        if fut := self._app.pending.get(rid):
            self._app.loop.call_soon_threadsafe(lambda: fut.done() or fut.set_result(answer))

    def open_url(self, url):
        if url.startswith(("https://", "http://")):
            webbrowser.open(url)

    # ---- settings: GitHub · Connections & Skills · AI accounts ----

    def settings(self):
        if not self._cfg["catalog"]:
            self.refresh_catalog()
        return {**self.state(), "gh_users": store.gh_accounts(), "catalog": self._cfg["catalog"],
                "off": self._cfg["off"], "default_account": self._cfg["default_account"]}

    def refresh_catalog(self):
        self._cfg["catalog"] = store.catalog(PLUGIN)
        self._app.save()
        return self._cfg["catalog"]

    def set_off(self, item_id, off, account_id=None):
        """Switch a connection/skill off everywhere (account_id None) or for one AI account."""
        holder = store.account(self._cfg, account_id) if account_id else self._cfg
        holder["off"] = [i for i in holder["off"] if i != item_id] + ([item_id] if off else [])
        self._app.save()
        self._app.reconnect(lambda s: not account_id or s.p["account"] == account_id)

    def update_account(self, account_id, changes):
        """nick / gh (the GitHub account new projects of this AI account start with; existing ones keep theirs)."""
        store.account(self._cfg, account_id).update({k: v for k, v in changes.items() if k in ("nick", "gh")})
        self._app.save()

    def set_default_account(self, account_id):
        self._cfg["default_account"] = account_id
        self._app.save()

    def claude_login(self, nick):
        """`claude setup-token` in a pseudo-terminal; the browser sign-in hands back a long-lived token."""
        try:
            proc = store.setup_token_pty(store.cli("claude") or "claude")
            token = store.read_setup_token(proc, lambda url: self._app.emit("login_url", url=url))
            proc.terminate(force=True)
        except Exception as e:
            return {"error": str(e)}
        return self.claude_add_token(nick, token) if token else {"error": "setup-token ended without a token"}

    def claude_add_token(self, nick, token):
        if not store.OAUTH_TOKEN.match((token or "").strip() + " "):
            return {"error": "That doesn't look like a token from `claude setup-token` (sk-ant-oat…)."}
        entry = store.store_token(self._cfg, nick.strip() or "Account", token)
        self._app.save()
        return entry

    def claude_remove(self, account_id):
        store.forget_token(self._cfg, account_id)
        self._app.save()

    def gh_accounts(self):
        return store.gh_accounts()

    def gh_login(self):
        return store.gh_login(lambda code, url: (self._app.emit("gh_code", code=code, url=url), webbrowser.open(url)))

    def gh_logout(self, user):
        return store.run(["gh", "auth", "logout", "-h", "github.com", "-u", user]).returncode == 0

    def gh_info(self, pid):
        p = store.project(self._cfg, pid)
        fields = "number,title,url,author"
        return {"repo": store.gh_json(["repo", "view", "--json", "nameWithOwner,url"], p["gh"], p["path"]),
                "prs": store.gh_json(["pr", "list", "--json", fields + ",headRefName"], p["gh"], p["path"]) or [],
                "issues": store.gh_json(["issue", "list", "--json", fields], p["gh"], p["path"]) or []}

    def gh_repos(self, user):
        return store.gh_json(["repo", "list", "--limit", "300", "--json", "nameWithOwner,description"], user) or []

    def clone(self, user, repo):
        picked = self._app.window.create_file_dialog(webview.FileDialog.FOLDER)
        if not picked:
            return None
        dest = os.path.join(picked[0], repo.split("/")[-1])
        r = store.run(["gh", "repo", "clone", repo, dest], env={**os.environ, **store.gh_env(user)}, timeout=600)
        if r.returncode:
            return {"error": r.stderr.strip() or "clone failed"}
        p = store.new_project(self._cfg, dest, gh=user)
        self._app.save()
        return p


def hide_console():
    """Windows: a windowed exe has no console, so every `claude`/`gh` it starts would pop one up. Give the app one
    hidden console instead; the children share it."""
    if os.name == "nt":
        import ctypes
        k = ctypes.windll.kernel32
        if not k.GetConsoleWindow() and k.AllocConsole():
            ctypes.windll.user32.ShowWindow(k.GetConsoleWindow(), 0)


def main():
    store.fix_path()
    hide_console()
    app = App()
    app.window = webview.create_window("Clauracle", str(HERE / "index.html"), js_api=Api(app),
                                       width=1280, height=820, min_size=(900, 560))
    webview.start(debug=bool(os.environ.get("CLAURACLE_DEBUG")))


if __name__ == "__main__":
    main()
