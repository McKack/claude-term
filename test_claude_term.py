"""Headless smoke test, no Claude calls: python test_claude_term.py"""
import asyncio
import json
import os
import subprocess
import tempfile
from pathlib import Path

import store

store.CONFIG = Path(tempfile.mkdtemp()) / "config.json"  # never touch the real config or project cache

from textual.widgets import Markdown  # noqa: E402

import claude_term as ct  # noqa: E402
import ui  # noqa: E402
from claude_agent_sdk import AssistantMessage, TextBlock, ToolResultBlock, ToolUseBlock, UserMessage  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
SAVE_UI, ct.ClaudeTerm.save_ui = ct.ClaudeTerm.save_ui, lambda self: None  # each app starts clean; tested at the end


def drain(lane):
    items = []
    while not lane.inbox.empty():
        items.append(lane.inbox.get_nowait())
    return items


async def main():
    app = ct.ClaudeTerm(HERE)
    app.agent_loop = app.ask_loop = app.scout = lambda: None  # stubs: no real CLI
    async with app.run_test(size=(150, 46)) as pilot:
        await pilot.pause(0.3)
        assert app.theme == "classic"
        assert "▐▛███▜▌" in str(app.query_one("#logo").render()), "crab logo"
        assert len({len(line) for line in ui.LOGO.splitlines()}) == 1, "crab lines must be equal width"
        app.save_screenshot("screenshot-welcome.svg")

        app.act("theme:matrix")
        await pilot.pause(0.2)
        vars_ = app.theme_variables
        assert vars_["block-cursor-foreground"] != vars_["block-cursor-background"], "menu highlight must invert"
        app.do("theme")
        await pilot.pause()
        await pilot.press("down")
        await pilot.pause()
        assert app.theme == "amber"
        await pilot.press("escape")
        await pilot.pause()
        assert app.theme == "matrix" and store.load()["theme"] == "matrix"

        # menu bar: hovering another title switches menus, clicking one outside opens it
        view, helpm = app.query_one("#m-view").region, app.query_one("#m-help").region
        assert [c.id for c in app.query("#menubar Chip")] == ["m-claude", "m-view", "m-help"], "Claude menu first"
        await pilot.press("alt+c")
        await pilot.pause()
        assert isinstance(app.screen, ui.PopupMenu)

        # submenu: opens beside its entry on top of the parent (which stays open), left goes back,
        # a pick closes both and acts
        parent = app.screen
        parent.open_sub("sub:permissions")
        await pilot.pause(0.3)
        sub = app.screen.query_one("#menu")
        assert app.screen is not parent and parent in app.screen_stack, "the parent menu stays open"
        assert "Web search" in str(sub.get_option_at_index(sub.option_count - 1).prompt)
        assert sub.region.x >= parent.query_one("#menu").region.right, "submenu sits beside the parent"
        await pilot.press("left")
        await pilot.pause(0.3)
        assert app.screen is parent and app.query_one("#m-claude").has_class("-open")
        parent.open_sub("sub:permissions")
        await pilot.pause(0.3)
        app.screen.dismiss("mode:acceptEdits")
        await pilot.pause(0.3)
        assert app.cfg["mode"] == "acceptEdits" and len(app.screen_stack) == 1, app.screen_stack
        app.set_mode("default")
        await pilot.press("alt+c")
        await pilot.pause()
        await pilot.hover(None, offset=(view.x + 1, view.y))
        await pilot.pause()
        assert isinstance(app.screen, ui.PopupMenu) and app.query_one("#m-view").has_class("-open")
        await pilot.click(None, offset=(helpm.x + 1, helpm.y))
        await pilot.pause()
        assert app.query_one("#m-help").has_class("-open")
        await pilot.press("escape")
        await pilot.pause()

        # composer: slash list, enter sends, shift+tab cycles mode
        app.query_one("#prompt").focus()
        await pilot.press("/", "h", "e")
        await pilot.pause()
        assert app.query_one("#slash").has_class("-on")
        await pilot.press("enter")
        await pilot.pause()
        assert "this UI:" in str(list(app.query(".note"))[-1].render()) and app.code.inbox.empty()
        await pilot.press(*"hello", "enter")
        assert drain(app.code) == ["hello"]
        await pilot.press("shift+tab")
        assert app.cfg["mode"] == "acceptEdits"

        # model switch whose client call fails: reported, app keeps running
        class Broken:
            async def set_model(self, m):
                raise RuntimeError("nope")
        app.code.client = Broken()
        app.act("model:claude-opus-5-5")
        await pilot.pause(0.2)
        assert app.is_running and "switching model failed" in str(list(app.query(".err"))[-1].render())
        app.code.client = None

        # attachments: pasted path → chip → content blocks; empty paste → clipboard
        txt = Path(tempfile.mkdtemp()) / "notes.txt"
        txt.write_text("remember the rabbit", encoding="utf-8")
        assert app.handle_paste(f'"{txt}"') and app.attachments == [str(txt)]
        pasted = []
        app.paste_clipboard = lambda: pasted.append(1)
        assert app.handle_paste("") and pasted
        assert not app.handle_paste("plain words")
        app.send("read this")
        item = drain(app.code)[0]
        assert isinstance(item, list) and "remember the rabbit" in item[0]["text"] and not app.attachments

        # Ask Claude tab: own lane, own view, starts lazily
        await pilot.press("f2")
        await pilot.pause()
        assert app.tab == "ask" and app.query_one("#ask").display and not app.query_one("#chat").display
        app.send("what is a monad?")
        assert app.ask.started and drain(app.ask) == ["what is a monad?"] and app.code.inbox.empty()
        await pilot.press("f2")
        assert app.tab == "code"

        # tool call + result, edit diff
        await app.show(AssistantMessage(content=[
            TextBlock("hi **neo**"), ToolUseBlock("t1", "Bash", {"command": "ls"}),
            ToolUseBlock("t2", "Edit", {"file_path": "x.py", "old_string": "a = 1\n", "new_string": "a = 2\nb = 3\n"}),
        ], model="x"))
        await app.show(UserMessage(content=[ToolResultBlock("t1", "a.txt"), ToolResultBlock("t2", "ok")]))
        await pilot.pause()
        assert not app.tools, "tool result not matched to its call"

        # failed tool call: an Explain chip sends the error to the Ask Claude lane
        await app.show(AssistantMessage(content=[ToolUseBlock("t5", "Bash", {"command": "frob"})], model="x"))
        await app.show(UserMessage(content=[ToolResultBlock("t5", "frob: command not found", True)]))
        await pilot.pause()
        chip = app.query_one(".explain", ui.Chip)
        await pilot.click(chip)
        sent = drain(app.ask)
        assert app.tab == "ask" and len(sent) == 1 and "frob: command not found" in sent[0], sent
        await pilot.press("f2")

        # tasks: Claude's list keeps timers across rewrites; finished ones get a duration
        todo = lambda s1, s2: AssistantMessage(content=[ToolUseBlock("t3", "TodoWrite", {"todos": [  # noqa: E731
            {"content": "Jack in", "status": s1, "activeForm": "Jacking in"},
            {"content": "Find Morpheus", "status": s2, "activeForm": "Finding Morpheus"}]})], model="x")
        await app.show(todo("in_progress", "pending"))
        started = app.tasks["c:Jack in"]["started"]
        await app.show(todo("completed", "in_progress"))
        assert app.tasks["c:Jack in"]["started"] == started and app.tasks["c:Jack in"]["ended"]
        await app.show(AssistantMessage(content=[ToolUseBlock("t4", "TaskCreate", {"subject": "Dodge bullets", "description": ""})], model="x"))
        await app.show(UserMessage(content=[ToolResultBlock("t4", "created")], tool_use_result={"task": {"id": "9", "subject": "Dodge bullets"}}))
        await pilot.pause()
        heads = [str(h.render()) for h in app.query(".task-head")]
        assert heads == ["IN PROGRESS", "UP NEXT", "FINISHED"], heads

        # plan tasks: clicking one while Claude's task runs is refused; once clear, confirm starts it
        app.set_plan_tasks([{"title": "Ship v1", "detail": "Tag and release.", "source": "PLAN.md", "done": False}])
        await app.task_clicked("p:0")
        assert app.tasks["p:0"]["status"] == "pending", "should refuse while another task runs"
        await app.show(todo("completed", "completed"))
        task = asyncio.create_task(app.task_clicked("p:0"))
        await pilot.pause()
        assert isinstance(app.screen, ui.Form) and app.screen.query_one("#f0").styles.min_height.value == 8
        app.screen.query_one("#f0").text = "Skip the changelog.\nTag it v1.0.0."
        await pilot.press("ctrl+s")
        await task
        sent = drain(app.code)[0]
        assert app.tasks["p:0"]["status"] == "in_progress" and "Ship v1" in sent and "Tag it v1.0.0." in sent, sent
        assert store.load_project(HERE)["tasks"][0]["status"] == "in_progress", "plan state saved"

        # add account: setup-token runs in a (fake) pseudo-console and the token fills itself in
        class FakePty:
            def __init__(self):
                self.chunks, self.written = ["https://claude.com/cai/oauth/authorize?x=1\r\n"], []

            def read(self, n):
                if self.chunks:
                    return self.chunks.pop(0)
                if self.written:
                    return "token: \x1b[33msk-ant-oat01-" + "a" * 40 + "\x1b[39m"
                import time
                time.sleep(0.05)
                return ""

            def write(self, s):
                self.written.append(s)

            def isalive(self):
                return True

            def terminate(self, force=False):
                pass
        fake = FakePty()
        real_pty, store.setup_token_pty = store.setup_token_pty, lambda cli: fake
        add = ui.AddAccount()
        app.push_screen(add)
        await pilot.pause()
        await pilot.click("#signin")
        await pilot.pause(0.3)
        assert add.query_one("#st-code").display and "Open the sign-in page" in str(add.query_one("#st-status").render())
        add.query_one("#st-code").value = "CODE123"
        add.query_one("#st-code").focus()
        await pilot.press("enter")
        await pilot.pause(0.3)
        assert fake.written == ["CODE123\r"] and add.query_one("#acct-token").value == "sk-ant-oat01-" + "a" * 40
        add.query_one("#acct-token").value = ""
        add.query_one("#acct-token").focus()
        add.query_one("#acct-token").post_message(ui.events.Paste("sk-ant-oat01-abc\ndef\n"))
        await pilot.pause()
        assert add.query_one("#acct-token").value == "sk-ant-oat01-abcdef", repr(add.query_one("#acct-token").value)
        store.setup_token_pty = real_pty
        add.dismiss(None)
        await pilot.pause()

        # account manager: rename, reorder, threshold, defaults; Save writes config
        app.cfg["accounts"].append({"id": "Work", "nick": "Work"})  # a token account (no real token needed here)
        app.do("accounts")
        await pilot.pause(0.3)
        mgr = app.screen
        assert isinstance(mgr, ui.AccountManager) and len(mgr.query(".acct-row")) == 2
        nick = mgr.query(".acct-nick").first()
        nick.value = "Personal"
        await pilot.pause()
        await mgr.on_chip_pressed(ui.Chip.Pressed(ui.Chip("↓", data=("down", 0))))  # Main below Work
        mgr.query_one("#auto-on").value = True
        mgr.query_one("#auto-at").value = "80"
        mgr.query_one("#def-effort").value = "high"
        mgr.save()
        await pilot.pause(0.2)
        saved = store.load()
        assert [a["id"] for a in saved["accounts"]] == ["Work", "Main"], saved["accounts"]
        assert saved["accounts"][1]["nick"] == "Personal" and saved["auto_switch"] == {"on": True, "at": 80}
        assert saved["defaults"]["effort"] == "high"

        # session overrides don't touch the defaults; New session starts from them again
        app.act("effort:low")
        assert app.session["effort"] == "low" and store.load()["defaults"]["effort"] == "high"
        drain(app.code)
        app.do("new")
        assert app.session["effort"] == "high"
        drain(app.code)

        # auto-switch: a usage event over the threshold hands the session to the next account
        from claude_agent_sdk import RateLimitEvent, RateLimitInfo
        app.session["account"] = app.code.account = "Work"
        soon = __import__("time").time() + 3600
        hot = RateLimitEvent(RateLimitInfo("allowed", raw={"status": "allowed", "unifiedWindows": {
            "five_hour": {"utilization": 0.85, "resetsAt": soon}}}), "u", "s")
        app.note_usage(hot, "Work")
        assert app.session["account"] == "Main" and ct.RECONNECT in drain(app.code)
        assert "Work 85%" not in str(app.query_one("#picker").render()) and "Personal" in str(app.query_one("#picker").render())
        app.note_usage(hot, "Work")  # a late event from the old process must not switch again
        assert app.session["account"] == "Main" and not drain(app.code)

        # MCP ui tool: side panel
        app.ui_server()
        await app.ui_tools["show_panel"].handler({"title": "PLAN", "markdown": "# step 1\nfollow the white rabbit"})
        await pilot.pause()
        assert app.query_one("#panel", Markdown).display and app.query_one("#side").has_class("-on")
        await pilot.pause(0.3)
        app.save_screenshot("screenshot-chat.svg")

        # clear-continue: toggle on kicks off, the tool arms, the turn's end clears and continues
        loop_tool, scouted = app.ui_tools["clear_and_continue"].handler, []
        app.scout = lambda: scouted.append(1)
        saved_usage, app.cfg["usage"]["Main"] = app.cfg["usage"].get("Main"), {"five_hour": {"utilization": 0.1}}
        assert "OFF" in (await loop_tool({}))["content"][0]["text"] and not app.armed
        app.do("loop")
        assert app.looping and app.cwd in store.load()["loop_dirs"] and drain(app.code) == [ct.LOOP_RULES]
        assert "clear-continue" in str(app.query_one("#menu-info").render())
        assert "Armed" in (await loop_tool({}))["content"][0]["text"] and app.armed
        app.session_id = "old"
        app.clear_continue()
        assert not app.armed and app.session_id is None and drain(app.code) == [ct.RECONNECT, ct.LOOP_RULES] and scouted
        app.cfg["usage"]["Main"]["five_hour"]["utilization"] = 0.85  # at the limit: refuse and switch off
        assert "NOT cleared" in (await loop_tool({}))["content"][0]["text"] and not app.looping and not app.armed
        app.cfg["usage"]["Main"] = saved_usage
        if saved_usage is None:
            del app.cfg["usage"]["Main"]
        store.save(app.cfg)

        # ask_user: a form with Claude's recommendations pre-selected; ctrl+s accepts them as they are
        spec = {"title": "STORAGE", "question": "Pick one. Example: [sqlite docs](https://sqlite.org)", "fields": [
            {"id": "db", "type": "radio", "label": "Database", "recommended": "sqlite",
             "options": [{"value": "sqlite", "description": "stdlib"}, {"value": "postgres"}]},
            {"id": "extras", "type": "checkbox", "label": "Extras", "options": ["wal", "fts", "backup"],
             "recommended": ["wal", "backup"]},
            {"id": "orm", "type": "select", "label": "ORM", "options": ["none", "peewee"], "recommended": "none"},
            {"id": "name", "type": "text", "label": "File name", "default": "app.db"},
            {"id": "tests", "type": "switch", "label": "Add tests", "recommended": True}]}
        task = asyncio.create_task(app.ui_tools["ask_user"].handler(spec))
        await pilot.pause(0.3)
        assert isinstance(app.screen, ui.Form)
        app.save_screenshot("screenshot-form.svg")
        await pilot.press("ctrl+s")
        answer = json.loads((await task)["content"][0]["text"])
        assert answer == {"db": "sqlite", "extras": ["wal", "backup"], "orm": "none", "name": "app.db", "tests": True,
                          "note": ""}, answer

        # advisor: its suggestions become a checkbox form; confirmed ones go to the Code session
        found = [{"title": "Reuse Chip", "why": "x", "kind": "reuse", "prompt": "use Chip", "recommended": True},
                 {"title": "Drop lib", "why": "y", "kind": "library", "prompt": "drop it", "recommended": False,
                  "links": ["https://example.com"]}]
        app.push_screen(ui.Form(app.advice_form(found)), lambda a: app.hand_to_claude(
            [found[int(i)] for i in a["pick"]], a["note"]))
        await pilot.pause(0.3)
        await pilot.press("ctrl+s")
        await pilot.pause()
        sent = drain(app.code)[0]
        assert "use Chip" in sent and "drop it" not in sent, sent
        app.cfg["advisor"]["every"], ran = 2, []
        app.advise = lambda manual=False: ran.append(1)
        app.count_turn(), app.count_turn()
        assert not ran, "advisor is off by default"
        app.do("advisor")
        await pilot.pause(0.3)
        app.screen.query_one("#f0", ui.RadioSet).query(ui.RadioButton)[1].value = True  # Suggest
        await pilot.press("ctrl+s")
        await pilot.pause()
        assert store.load()["advisor"]["mode"] == "suggest"
        app.count_turn(), app.count_turn()
        assert ran == [1]
        app.cfg["advisor"]["mode"] = "off"
        store.save(app.cfg)

        # advisor remembers what it already suggested, per project, across restarts
        class FakeResult:
            structured_output = {"suggestions": [{"title": "Cache scout results", "why": "x", "kind": "performance",
                                                  "prompt": "cache it", "recommended": True}]}
        prompts, real_query, real_result = [], ct.query, ct.ResultMessage

        async def fake_query(prompt, options):
            prompts.append(prompt)
            yield FakeResult()
        ct.query, ct.ResultMessage, app.cfg["advisor"]["mode"] = fake_query, FakeResult, "act"
        del app.advise  # the real worker again
        app.advise()
        await pilot.pause(0.3)
        drain(app.code)
        assert store.load_project(app.cwd, "advisor")["titles"] == ["Cache scout results"]
        app.advise()
        await pilot.pause(0.3)
        drain(app.code)
        assert "Already suggested, don't repeat: ['Cache scout results']" in prompts[1], prompts[1][-300:]
        ct.query, ct.ResultMessage, app.cfg["advisor"]["mode"] = real_query, real_result, "off"
        store.save(app.cfg)

        # New twice in a row: the second welcome must wait for the first to go (DuplicateIds crash)
        app.do("new"), app.do("new")
        await pilot.pause(0.3)
        assert len(app.query("#welcome")) == 1

        # handover writer: New session after some work offers to update HANDOVER.md; nothing is saved unconfirmed
        real_cwd, app.cwd = app.cwd, tempfile.mkdtemp()
        handover = os.path.join(app.cwd, "HANDOVER.md")
        Path(handover).write_text("# Plan\n- [ ] ship it\n", encoding="utf-8")

        class Drafted:
            structured_output = {"markdown": "# Plan\n- [x] ship it (verified)\n- [ ] next thing"}

        async def draft_query(prompt, options):
            assert "ship it" in prompt and options.tools == ["Read", "Glob", "Grep"], "read-only, sees the old file"
            yield Drafted()
        ct.query, ct.ResultMessage = draft_query, Drafted
        app.session_id, app.worked = "s1", True
        app.do("new")
        await pilot.pause(0.3)
        await pilot.press("n")  # not now: straight to a new session, file untouched
        await pilot.pause(0.3)
        assert app.session_id is None and not app.worked and "ship it\n" in Path(handover).read_text(encoding="utf-8")
        app.session_id, app.worked = "s2", True
        app.do("new")
        await pilot.pause(0.3)
        await pilot.press("y")  # update → a diff to confirm
        await pilot.pause(0.5)
        assert isinstance(app.screen, ui.Confirm) and "next thing" in str(app.screen.query_one("#dlg-body").render())
        await pilot.press("y")
        await pilot.pause(0.3)
        assert "- [x] ship it (verified)" in Path(handover).read_text(encoding="utf-8") and app.session_id is None
        app.session_id, app.worked, app.cfg["loop_dirs"] = "s3", True, [app.cwd]
        assert not app.handover_due(), "the clear-continue loop writes its own handover"
        app.cfg["loop_dirs"], app.cwd, app.worked = [], real_cwd, False

        # commit writer: Haiku drafts from the diff, the user edits in a form, claude-term commits everything
        repo = tempfile.mkdtemp()
        git = lambda *a: subprocess.run(["git", *a], cwd=repo, capture_output=True, text=True).stdout.strip()  # noqa: E731
        git("init", "-q"), git("config", "user.email", "t@t"), git("config", "user.name", "t")
        Path(repo, "a.txt").write_text("one\n")
        git("add", "-A"), git("commit", "-qm", "first")
        Path(repo, "a.txt").write_text("two\n")
        Path(repo, "b.txt").write_text("new\n")

        class Message:
            structured_output = {"message": "Tweak a, add b"}

        async def message_query(prompt, options):
            assert "-one" in prompt and "+two" in prompt and "b.txt" in prompt and "first" in prompt, prompt
            yield Message()
        ct.query, ct.ResultMessage, app.cwd = message_query, Message, repo
        app.commit()
        await pilot.pause(0.5)
        assert isinstance(app.screen, ui.Form) and app.screen.query_one("#f0").text == "Tweak a, add b"
        app.screen.query_one("#f0").text = "Tweak a, add b (edited)"
        await pilot.press("ctrl+s")
        await pilot.pause(0.5)
        assert git("log", "-1", "--format=%s") == "Tweak a, add b (edited)" and git("status", "--porcelain") == ""
        app.cwd = tempfile.mkdtemp()  # not a repo: a warning, no dialog
        app.commit()
        await pilot.pause(0.5)
        assert not isinstance(app.screen, ui.Form)
        app.cwd = real_cwd
        ct.query, ct.ResultMessage = real_query, real_result

        # permission dialog: deny
        task = asyncio.create_task(app.can_use_tool("Bash", {"command": "rm x"}, None))
        await pilot.pause()
        await pilot.press("n")
        assert (await task).behavior == "deny"

        # AskUserQuestion: pick the 2nd option
        q = {"question": "Red or blue?", "header": "Pill", "multiSelect": False,
             "options": [{"label": "Red", "description": "truth"}, {"label": "Blue", "description": "sleep"}]}
        task = asyncio.create_task(app.can_use_tool("AskUserQuestion", {"questions": [q]}, None))
        await pilot.pause()
        await pilot.press("down", "enter")
        assert (await task).updated_input["answers"] == {"Red or blue?": "Blue"}

        # resume panel + replay of a real session in this folder (if any exist)
        await pilot.press("ctrl+r")
        await pilot.pause()
        assert app.query_one("#resume").has_class("-on")
        sessions = app.recent_sessions()
        if sessions:
            await app.resume_session(sessions[0].session_id)
            await pilot.pause()
            assert app.session_id == sessions[0].session_id and ct.RECONNECT in drain(app.code)
            assert app.query_one("#resume").has_class("-on") and app.query_one("#side").has_class("-on"), \
                "resuming must leave both side panels open"

    # matrix theme: menus
    app = ct.ClaudeTerm(HERE)
    app.agent_loop = app.scout = lambda: None
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause(0.2)
        app.apply_theme("matrix", save=False)

        # open menu title: inverted with real colors (not the terminal default)
        await pilot.press("alt+v")
        await pilot.pause()
        title = app.query_one("#m-view").styles
        assert title.color.ansi is None and title.color != title.background, "menu bar highlight must invert"

        # hovering an entry moves the selection to it
        menu = app.screen.query_one("#menu")
        r = menu.region
        await pilot.hover(None, offset=(r.x + 3, r.y + 4))
        await pilot.pause()
        assert menu.highlighted == 3, menu.highlighted

        # a fast mouse can fire two closes before the menu is gone: only the first may count
        menu_screen = app.screen
        menu_screen.dismiss("menu:help")
        menu_screen.dismiss("menu:claude")
        await pilot.pause(0.3)
        assert app.is_running and len(app.screen_stack) == 2 and app.query_one("#m-help").has_class("-open")
        await pilot.press("escape")
        await pilot.pause()

        # a menu action that opens a dialog must open it *after* the menu is gone
        app.action_menu("view")
        await pilot.pause()
        await pilot.press("enter")  # Theme…
        await pilot.pause(0.3)
        assert isinstance(app.screen, ui.ThemePicker) and len(app.screen_stack) == 2, app.screen_stack
        await pilot.press("escape")
        await pilot.pause()

        app.act("theme:amber")
        assert app.theme == "amber", "other themes paint their own background"

    # first run without the CLI: install card instead of welcome, agent not started
    real, ct.find_claude = ct.find_claude, lambda: None
    app = ct.ClaudeTerm(HERE)
    started = []
    app.agent_loop = lambda: started.append(1)
    async with app.run_test(size=(150, 46)) as pilot:
        await pilot.pause(0.2)
        assert app.query(ui.InstallCard) and not started
    ct.find_claude = real

    # restart: exits with a command that starts this script again, in this folder, back in this session;
    # a start with --resume goes straight into that session instead of the welcome screen
    app = ct.ClaudeTerm(HERE)
    app.agent_loop = app.scout = lambda: None
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause(0.2)
        app.session_id = "abc123"
        app.act("restart")
        await pilot.pause()
    assert app.restart_cmd[1:] == [os.path.join(HERE, "claude_term.py"), HERE, "--resume", "abc123"], app.restart_cmd
    resumed = []
    app = ct.ClaudeTerm(HERE, resume="abc123")
    app.agent_loop = app.scout = lambda: None

    async def fake_resume(sid):
        resumed.append(sid)
    app.resume_session = fake_resume
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause(0.2)
        assert resumed == ["abc123"] and not app.query("#welcome")
    # collapsed panels are rails with counts; rails and « » buttons open and close them; the input grows
    app = ct.ClaudeTerm(HERE)
    app.agent_loop = app.scout = lambda: None
    async with app.run_test(size=(150, 46)) as pilot:
        await pilot.pause(0.2)
        app.set_plan_tasks([{"title": f"task {i}", "source": "H.md", "done": i == 0} for i in range(4)])
        await pilot.pause(0.2)
        side, rail_side, rail_resume = app.query_one("#side"), app.query_one("#rail-side"), app.query_one("#rail-resume")
        assert side.has_class("-on") and not rail_side.display and rail_resume.display
        assert "S\nE\nS" in str(rail_resume.render()), "sessions rail spells its title down the column"
        assert app.rail_text("»", "S", 12, "just") == "»\n\nS\n\n1\n2\n\nn\no\nw", "facts run down the 3-cell rail"

        # ↻ scan: a spinner runs in the header and the empty list until the scout returns
        gate = asyncio.Event()

        async def slow_scan(status):
            await gate.wait()
            status.update("done")
        saved_tasks, app.tasks = app.tasks, {}
        app.scan_plans = slow_scan
        ct.ClaudeTerm.scout(app)
        await pilot.pause(0.3)
        spin = str(app.query_one("#tasks-refresh").render())
        assert spin in "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏" and app.query(".scanning"), "spinner while scanning"
        gate.set()
        await pilot.pause(0.2)
        assert str(app.query_one("#tasks-refresh").render()) == "↻" and not app.query(".scanning")
        app.tasks = saved_tasks
        app.render_tasks()
        await pilot.pause(0.1)
        await pilot.click("#side-close")
        await pilot.pause(0.2)
        assert not side.has_class("-on") and rail_side.display and str(rail_side.render()).endswith("\n3"), \
            "the tasks rail counts unfinished tasks"
        app.set_plan_tasks([{"title": "new", "source": "H.md"}])
        await pilot.pause(0.2)
        assert not side.has_class("-on"), "a collapsed panel stays collapsed when tasks change"
        await pilot.click("#rail-resume")
        await pilot.pause(0.2)
        assert app.query_one("#resume").has_class("-on") and not rail_resume.display
        app.save_screenshot("screenshot-rails.svg")
        await pilot.click("#resume-close")
        await pilot.click("#rail-side")
        await pilot.pause(0.2)
        assert side.has_class("-on") and not app.query_one("#resume").has_class("-on")
        prompt = app.query_one("#prompt")
        prompt.text = "\n".join(f"line {i}" for i in range(15))
        await pilot.pause(0.2)
        assert prompt.size.height >= 15, "the input grows with what's typed"

    # closed or crashed: the next start comes back with the draft, attachments, panels, tab and session
    app = ct.ClaudeTerm(HERE)
    app.agent_loop = app.scout = lambda: None
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause(0.2)
        app.query_one("#prompt").text = "half-typed thought"
        app.add_attachments([os.path.join(HERE, "store.py")])
        app.query_one("#side").remove_class("-on")
        app.do("resume")
        app.session_id = "abc123"
        SAVE_UI(app)  # what the 2s timer does; no clean exit needed
    resumed = []
    app = ct.ClaudeTerm(HERE)
    app.agent_loop = app.scout = lambda: None

    async def fake_resume(sid):
        resumed.append(sid)
    app.resume_session = fake_resume
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause(0.2)
        assert app.query_one("#prompt").text == "half-typed thought" and resumed == ["abc123"]
        assert app.attachments == [os.path.join(HERE, "store.py")]
        assert app.query_one("#resume").has_class("-on") and not app.query_one("#side").has_class("-on")
    print(f"ok (replayed {len(sessions) and 1} session)")


asyncio.run(main())
