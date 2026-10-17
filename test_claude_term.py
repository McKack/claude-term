"""Headless smoke test, no Claude calls: python test_claude_term.py"""
import asyncio
import json
import os
import subprocess
import tempfile
import time
from pathlib import Path

import store

store.CONFIG = Path(tempfile.mkdtemp()) / "config.json"  # never touch the real config or project cache

from textual.widgets import Markdown  # noqa: E402

import claude_term as ct  # noqa: E402
import ui  # noqa: E402
from claude_agent_sdk import AssistantMessage, TextBlock, ToolResultBlock, ToolUseBlock, UserMessage  # noqa: E402

def plain(row):  # a task row draws a rich Table: render that to text
    from rich.console import Console
    c = Console(width=60, record=True, file=open(os.devnull, "w", encoding="utf-8"))
    c.print(ui.TaskPanel.line(row.app.tasks[row.data], time.time()))
    return c.export_text().strip()


HERE = os.path.dirname(os.path.abspath(__file__))
SAVE_UI, ct.ClaudeTerm.save_ui = ct.ClaudeTerm.save_ui, lambda self: None  # each app starts clean; tested at the end
ct.ClaudeTerm.patcher = lambda self, tb, crashed, src: print("UNEXPECTED BUG CAUGHT:\n" + tb)  # never a real patch


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
        assert app.query(".bubble.-claude") and app.query(".bubble.-user"), "chat bubbles on both sides"
        assert len(ui.stamp(time.time())) == 5, "today: time only"
        assert " " in ui.stamp(time.time() - 3 * 86400), "older: date and time"

        # failed tool call: an Explain chip sends the error to the Ask Claude lane
        await app.show(AssistantMessage(content=[ToolUseBlock("t5", "Bash", {"command": "frob"})], model="x"))
        await app.show(UserMessage(content=[ToolResultBlock("t5", "frob: command not found", True)]))
        await pilot.pause()
        chip = app.query_one(".explain", ui.Chip)
        chip.scroll_visible(animate=False)
        await pilot.pause()
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

        # Claude marks plan tasks live (loose title match, unknown ones added); rescans keep what's running
        app.set_plan_tasks([{"title": "Write the docs", "source": "HANDOVER.md", "done": False},
                            {"title": "Fix the paste bug", "source": "HANDOVER.md", "done": False}])
        app.ui_server()  # builds app.ui_tools, as the Code session does
        await app.ui_tools["task"].handler({"title": "fix the paste bug", "status": "in_progress"})
        assert app.tasks["p:1"]["status"] == "in_progress" and app.tasks["p:1"]["started"]
        await pilot.pause()
        row = app.query_one(".task.-in_progress", ui.Chip)
        assert plain(row).startswith("►"), "current task: ►"
        assert row.styles.color == app.query_one("#tasks-title").styles.color, "current task in the TASKS color"
        assert any(plain(c).startswith("□") for c in app.query(".task.-pending"))
        started = app.tasks["p:1"]["started"]
        app.set_plan_tasks([{"title": "Write the docs", "source": "HANDOVER.md", "done": False},
                            {"title": "Fix the paste bug", "source": "HANDOVER.md", "done": False}])
        assert app.tasks["p:1"]["status"] == "in_progress" and app.tasks["p:1"]["started"] == started
        app.set_plan_tasks([{"title": "Fix the paste bug", "source": "HANDOVER.md", "done": True}])
        assert app.tasks["p:0"]["status"] == "completed" and app.tasks["p:0"]["ended"], "ticked in the file: done"
        await pilot.pause()
        assert plain(app.query_one(".task.-completed", ui.Chip)).startswith("■")
        # a Code turn with no task in progress shows as one; finished ones stop at 5, newest first
        for i in range(7):
            app.turn_task(f"turn {i}\n  with  detail")
            assert [t["title"] for t in app.tasks.values() if t["status"] == "in_progress"] == [f"turn {i} with detail"]
            app.end_turn_task()
        await pilot.pause()
        done = [plain(c) for c in app.query(".task.-completed")]
        assert len(done) == 5 and done[0].startswith("■ turn 6"), done
        # a long prompt shows "Loading task summary…" at once, then Haiku's short name for it
        class Named:
            result = ' "Add task delete button." '
        gate, real_query, real_result = asyncio.Event(), ct.query, ct.ResultMessage

        async def name_query(prompt, options):
            assert options.model == ct.SCOUT_MODEL and "delete" in prompt
            await gate.wait()
            yield Named()
        ct.query, ct.ResultMessage = name_query, Named
        app.turn_task("Please add support for choosing to Delete a task when clicking one in the list, thanks")
        await pilot.pause()
        running = next(t for t in app.tasks.values() if t["status"] == "in_progress")
        assert running["title"] == "Loading task summary…"
        gate.set()
        await pilot.pause(0.2)
        assert running["title"] == "Add task delete button", running["title"]
        # the task scout: one tool-less call with the plan files in the prompt, its tokens logged as "scout"
        class Scanned:
            structured_output = {"tasks": [{"title": "Scanned task", "source": "HANDOVER.md", "done": False}]}
            usage = {"input_tokens": 1200, "output_tokens": 80}
        seen = []

        async def scan_query(prompt, options):
            seen.append((prompt, options))
            yield Scanned()
        ct.query, ct.ResultMessage = scan_query, Scanned
        status = ui.Static()
        await app.scan_plans(status)
        prompt, options = seen[0]
        assert options.tools == [] and options.max_turns == 2 and "=== HANDOVER.md ===\n# Handover" in prompt
        assert any(t["title"] == "Scanned task" for t in app.tasks.values())
        assert store.recent_usage()[-1]["src"] == "scout"
        ct.query, ct.ResultMessage = real_query, real_result
        app.end_turn_task()
        app.turn_task("vague prompt")
        await app.ui_tools["task"].handler({"title": "Fix the paste bug", "status": "in_progress"})
        assert not any(k.startswith("t:") and t["status"] == "in_progress" for k, t in app.tasks.items()), \
            "Claude named the real task: the stand-in goes"
        app.turn_task("another")  # something's already in progress: no stand-in
        assert sum(t["status"] == "in_progress" for t in app.tasks.values()) == 1
        app.set_plan_tasks([{"title": "Fix the paste bug", "source": "HANDOVER.md", "done": True}])
        await app.ui_tools["task"].handler({"title": "Something new entirely", "status": "in_progress"})
        assert any(t["title"] == "Something new entirely" and t["status"] == "in_progress" for t in app.tasks.values())

        # Delete from the start dialog: gone, and a rescan of the plan files doesn't bring it back
        app.set_plan_tasks([{"title": "Old idea", "source": "PLAN.md", "done": False},
                            {"title": "Keep me", "source": "PLAN.md", "done": False}])
        task = asyncio.create_task(app.task_clicked("p:0"))
        await pilot.pause()
        app.screen.query_one("#alt").press()
        await task
        assert [t["title"] for k, t in app.tasks.items() if k[0] == "p"] == ["Keep me"] and "old idea" in app.deleted
        assert store.load_project(HERE)["deleted"] == ["old idea"]
        app.set_plan_tasks([{"title": "Old idea", "source": "PLAN.md", "done": False},
                            {"title": "Keep me", "source": "PLAN.md", "done": True}])
        assert [t["title"] for k, t in app.tasks.items() if k[0] == "p"] == ["Keep me"]
        task = asyncio.create_task(app.task_clicked("p:0"))  # a finished one: confirm deletes it
        await pilot.pause()
        await pilot.press("y")
        await task
        assert not any(k[0] == "p" for k in app.tasks)
        app.deleted.clear()

        # Claude's question gets answer buttons; a click pins the answer and sends it quoting the message
        drain(app.code)
        app.bubble(Markdown("Done with A.\n\nShould I do **X** or Y?"), "claude")
        await pilot.pause()
        first = app.claude_bubbles("#chat")[-1]
        await app.ui_tools["reply_options"].handler({"options": ["Do X", "Do Y"]})
        await pilot.pause()
        assert [str(c.render()).strip() for c in first.query(".choice")] == ["Do X", "Do Y", "Something else…"]
        users = len(app.query(".bubble.-user"))
        app.on_chip_pressed(ui.Chip.Pressed(first.query(".choice").first()))
        await pilot.pause()
        sent = drain(app.code)[-1]
        assert "> Should I do **X** or Y?" in sent and sent.endswith("\n\nDo X"), sent
        assert not first.query(".choices") and "Do X" in str(first.query_one(".pinned").render())
        assert len(app.query(".bubble.-user")) == users, "latest message: the pin is the answer, no extra bubble"
        # ↩ on an older message: the prompt gets its @ref; sending pins it there and shows a reply bubble
        app.bubble(Markdown("Something later."), "claude")
        await pilot.pause()
        app.on_chip_pressed(ui.Chip.Pressed(first.query_one(".reply")))
        assert app.query_one("#prompt").text == f"@{first.ref} "
        app.on_prompt_submitted(ui.Prompt.Submitted(f"@{first.ref} actually, Z"))
        await pilot.pause()
        assert drain(app.code)[-1].endswith("\n\nactually, Z") and len(first.query(".pinned")) == 2
        assert len(app.query(".bubble.-user")) == users + 1
        await app.ui_tools["reply_options"].handler({"options": ["Ship it", "Wait for review"]})
        await pilot.pause()
        app.save_screenshot("screenshot-reply.svg")

        # /usage-graph: tokens per minute plus limit and context % over the last hour, from the usage log
        now, acct = time.time(), app.session["account"]
        log = [{"t": now - 50 * 60, "tokens": 9000, "inp": 1000, "cache": 7000, "out": 1000, "src": "scout"},
               {"t": now - 50 * 60, "account": acct, "five_hour": 20.0, "seven_day": 5.0},
               {"t": now - 10 * 60, "tokens": 30000, "inp": 2000, "cache": 26000, "out": 2000, "src": "code"},
               {"t": now - 10 * 60, "account": acct, "five_hour": 48.0, "seven_day": 9.0},
               {"t": now - 60, "context": 41.0}]
        graph = ui.usage_graph(log, acct, {"totalTokens": 82000, "maxTokens": 200000, "percentage": 41.0}, "green",
                               now).plain
        assert "39k this hour (in 3k · cache read 33k · out 3k)" in graph, graph
        assert "by source: code 30k · scout 9k" in graph, graph
        assert "5-HOUR LIMIT  now 48%" in graph and "WEEKLY LIMIT  now 9%" in graph
        assert "CONTEXT WINDOW  82k of 200k tokens  now 41%" in graph and graph.count("█") > 5
        assert ui.bar_rows([0, 4, 8, None], 1, 8) == [" ▄█ "] and ui.bar_rows([16], 2, 16) == ["█", "█"]
        for p in log:
            store.log_usage(**{k: v for k, v in p.items() if k != "t"})
        app.local_command("/usage-graph")
        await pilot.pause()
        assert "TOKENS PER MINUTE" in str(app.query(".graph").last().render())
        app.save_screenshot("screenshot-usage.svg")
        # the once-a-minute sample: limits from the last rate-limit event (a reset window reads 0) and context
        app.cfg["usage"][acct] = {"five_hour": {"utilization": 0.3, "resetsAt": now + 3600},
                                  "seven_day": {"utilization": 0.6, "resetsAt": now - 1}}
        app.context = {"percentage": 12.5}
        app.sample_usage()
        p = store.recent_usage()[-1]
        assert (p["account"], p["five_hour"], p["seven_day"], p["context"]) == (acct, 30.0, 0, 12.5), p

        # context past 50%: queued messages wait, Claude writes the handover, clears, resumes, then they go
        drain(app.code)
        app.context, app.ctx_first = {"totalTokens": 110000, "maxTokens": 200000, "percentage": 55.0}, True
        assert not app.context_full(), "a session's first turn never hands over"
        app.ctx_first = False
        assert app.context_full()
        app.code.inbox.put_nowait("my next question")
        app.hand_over_context()
        assert app.held == ["my next question"] and "HANDOVER.md" in drain(app.code)[0]
        assert "Armed" in (await app.ui_tools["clear_and_continue"].handler({}))["content"][0]["text"]
        app.clear_continue()
        await pilot.pause()
        assert drain(app.code)[-2:] == [ct.CONTEXT_CONTINUE, "my next question"] and not app.ctx_handover
        assert app.context is None and not app.held
        app.cfg["context_clear"] = False
        app.context = {"percentage": 90.0}
        assert not app.context_full(), "setting off: no handover"
        app.cfg["context_clear"], app.context = True, None

        # pasting text into the prompt inserts it once
        prompt = app.query_one("#prompt")
        prompt.text = ""
        prompt.focus()
        app.post_message(ui.events.Paste("hello paste"))  # as the terminal delivers it: to the app, then forwarded
        await pilot.pause()
        assert prompt.text == "hello paste", repr(prompt.text)
        prompt.text = ""

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
        # while a lane works, a "Thinking… 0:00" row sits under its last output; it goes when the turn ends
        app.code.busy, app.code.doing, app.code.since = True, "Running Bash", time.time()
        await pilot.pause(0.3)
        row = app.code.working
        assert row and row.parent is app.query_one("#chat") and "Running Bash…" in str(row.content)
        app.say("note", "more output")
        await pilot.pause()
        assert app.query_one("#chat").children[-1] is row, "the working row stays last"
        app.save_screenshot("screenshot-working.svg")
        assert app.query_one("#chat").is_anchored
        app.code.busy = False
        await pilot.pause(0.3)
        assert not row.parent and not app.code.working
        # the API cost estimate is hidden unless View → Show API cost estimate is on
        info = lambda: str(app.query_one("#menu-info").content)
        assert "$" not in info(), info()
        app.do("cost")
        assert "API est." in info() and store.load()["show_cost"]
        app.do("cost")
        assert "$" not in info()
        # your bubble wraps a long line, folds a long message, and a click unfolds it
        app.bubble(msg := ui.UserText("word " * 200), "user")
        await pilot.pause()
        assert msg.size.width <= msg.parent.size.width and msg.size.height > 1, "wraps, not clipped"
        assert "show all" in str(msg.content) and len(str(msg.content)) < 700
        await pilot.click(msg)
        await pilot.pause()
        assert str(msg.content).count("word") == 200 and "show less" in str(msg.content)
        short = ui.UserText("hi")
        assert not short.has_class("fold")
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
        assert not app.armed and app.session_id is None and drain(app.code) == [ct.RECONNECT, ct.LOOP_RULES]
        assert not scouted, "no second scan: the handover edit's turn already rescanned"
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
        found = [{"id": "a", "title": "Reuse Chip", "why": "x", "kind": "reuse", "prompt": "use Chip",
                  "recommended": True},
                 {"id": "b", "title": "Drop lib", "why": "y", "kind": "library", "prompt": "drop it",
                  "recommended": False, "links": ["https://example.com"]}]
        app.push_screen(ui.Form(app.advice_form(found)), lambda a: app.hand_to_claude(
            [found[int(i)] for i in a["pick"]], a["note"]))
        await pilot.pause(0.3)
        await pilot.press("ctrl+s")
        await pilot.pause()
        sent = drain(app.code)
        assert len(sent) == 1 and sent[0][:2] == ("advisor", "a") and "use Chip" in sent[0][2], sent
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
        prompts, models, real_query, real_result = [], [], ct.query, ct.ResultMessage

        async def fake_query(prompt, options):
            prompts.append(prompt)
            models.append((options.model, options.effort))
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
        assert models[0] == ("claude-sonnet-5-5", "medium"), models  # the automatic check: Sonnet, medium
        app.session.update(model="claude-opus-5-5", effort="high")
        app.advise(manual=True)  # /advisor: your session's model and effort
        await pilot.pause(0.3)
        drain(app.code)
        assert models[-1] == ("claude-opus-5-5", "high"), models
        await pilot.press("escape")  # a manual run asks first; leave it "waiting for you"
        await pilot.pause()
        app.session.update(model="", effort="")
        ct.query, ct.ResultMessage, app.cfg["advisor"]["mode"] = real_query, real_result, "off"
        store.save(app.cfg)

        # Advisor tab: the log renders newest first; a note is kept; an applied suggestion undoes by itself
        log = store.load_project(app.cwd, "advisor")["log"]
        assert [e["type"] for e in log] == ["run", "suggestion"] * 3, log
        assert [e["status"] for e in log if e["type"] == "suggestion"] == ["applying", "applying", "suggested"]
        app.switch_tab("advisor")
        await pilot.pause()
        assert app.query_one("#advisor").display and not app.query_one("#chat").display
        assert len(app.query(".adv-card")) == 3 and app.lane is app.code
        app.save_screenshot("screenshot-advisor.svg")
        eid = log[-1]["id"]
        app.run_worker(app.advisor_button("note", eid))
        await pilot.pause(0.3)
        app.screen.query_one(ui.TextArea).text = "too slow"
        await pilot.press("ctrl+s")
        await pilot.pause()
        assert app.advisor_entry(eid)["notes"][0]["text"] == "too slow"
        real_cwd2, app.cwd = app.cwd, tempfile.mkdtemp()
        g = lambda *a: subprocess.run(["git", "-C", app.cwd, *a], capture_output=True, check=True)
        g("init", "-q"), g("config", "user.email", "t@t"), g("config", "user.name", "t")
        Path(app.cwd, "a.txt").write_text("one\n")
        g("add", "-A"), g("commit", "-qm", "init")
        before = await app.snapshot()
        Path(app.cwd, "a.txt").write_text("two\n")
        Path(app.cwd, "new.txt").write_text("new\n")
        after = await app.snapshot()
        assert before and after and before != after
        app.advice_applied(eid, before, after)
        Path(app.cwd, "b.txt").write_text("later\n")  # a later change survives the undo
        app.run_worker(app.undo_advice(app.advisor_entry(eid)))
        await pilot.pause(0.3)
        await pilot.press("y")
        await pilot.pause(0.5)
        assert Path(app.cwd, "a.txt").read_text() == "one\n" and not Path(app.cwd, "new.txt").exists()
        assert Path(app.cwd, "b.txt").exists() and app.advisor_entry(eid)["status"] == "undone"
        app.cwd = real_cwd2
        app.switch_tab("code")

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

        # ≡ scan: a spinner runs in the header and the empty list until the scout returns
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
        assert str(app.query_one("#tasks-refresh").render()) == "≡" and not app.query(".scanning")
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

    # self-heal: a worker error in our own code goes to the patcher once; a leftover crash file too
    tb = 'Traceback:\n  File "x\\site.py", line 1, in f\n  File "D:\\a\\ui.py", line 42, in g\nKeyError: 1\n'
    assert ct.own_bug(tb) == "ui.py:42 KeyError" and ct.own_bug('File "x/textual/a.py", line 3\nOops') is None
    patches = []
    ct.CRASH_FILE.parent.mkdir(parents=True, exist_ok=True)
    ct.CRASH_FILE.write_text(tb, encoding="utf-8")
    app = ct.ClaudeTerm(HERE)
    app.agent_loop = app.scout = lambda: None
    app.patcher = lambda tb, crashed, src: patches.append((tb, crashed))
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause(0.2)
        assert patches == [(tb, True)] and not ct.CRASH_FILE.exists(), "crash file goes to the patcher"
        app.healed.clear()

        async def boom():
            ct.own_bug(None)  # AttributeError inside claude_term.py
        for _ in range(2):
            app.run_worker(boom(), exit_on_error=False)
            await pilot.pause(0.2)
        assert len(patches) == 2 and "claude_term.py" in patches[1][0] and not patches[1][1], "once per bug"
        app.patched({"fixed": True, "summary": "guarded the None"})
        await pilot.pause()
        toast = [n for n in app._notifications if n.title == "Fix ready"]
        assert toast and "app.do('restart')" in toast[0].message and "restored" in toast[0].message
        from textual.content import Content
        assert "Restart now" in Content.from_markup(toast[0].message).plain
    print(f"ok (replayed {len(sessions) and 1} session)")


# Every non-ASCII glyph the UI can draw, all present in Cascadia Mono (Windows Terminal's default font), so none
# falls back to an emoji font. To add one, check the font has it first (fontTools is a dev-only extra):
#   python -c "from fontTools.ttLib import TTFont; print(ord('X') in TTFont(r'C:\Windows\Fonts\CascadiaMono.ttf').getBestCmap())"
ALLOWED_GLYPHS = set("«±·»×“”…←↑→↓↔−∞≡─│└▁▂▃▄▅▆▇█▌▐▘▛▜▝■□▣▴▸►▼▾◄◇◉○●✓⠇⠋⠏⠙⠦⠧⠴⠸⠹⠼")


def check_glyphs():
    """Non-ASCII outside comments (strings, docstrings, code) must be in ALLOWED_GLYPHS."""
    import tokenize
    bad = []
    for name in ("claude_term.py", "ui.py", "store.py", "attach.py"):
        with open(os.path.join(HERE, name), "rb") as f:
            for tok in tokenize.tokenize(f.readline):
                if tok.type in (tokenize.COMMENT, tokenize.ENCODING):
                    continue
                for i, line in enumerate(tok.string.split("\n")):
                    bad += [f"{name}:{tok.start[0] + i} {ch!r} U+{ord(ch):04X}" for ch in line
                            if ord(ch) > 127 and ch not in ALLOWED_GLYPHS]
    assert not bad, "glyphs not in ALLOWED_GLYPHS (check them against the font):\n" + "\n".join(bad)


check_glyphs()
asyncio.run(main())
