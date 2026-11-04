"""Headless check: no window, no Claude calls (a fake client stands in). Prints ok."""
import asyncio
import re
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

import store

store.HOME = Path(tempfile.mkdtemp())
store.CONFIG = store.HOME / "config.json"
store.CONFIG.write_text("{}", encoding="utf-8")  # an existing config: no claude-term import in tests
import clauracle  # noqa: E402

sent, events = [], []


class FakeClient:
    def __init__(self, options):
        self.options = options

    async def __aenter__(self):
        FakeClient.last = self
        return self

    async def __aexit__(self, *a):
        pass

    async def query(self, item):
        sent.append(item)

    async def receive_response(self):
        if FakeClient.during:  # what Claude does mid-turn (once), e.g. call clear_and_continue
            during, FakeClient.during = FakeClient.during, None
            FakeClient.last_tool = await during()
        return
        yield

    async def get_context_usage(self):
        return {"percentage": FakeClient.pct}


FakeClient.pct, FakeClient.during = 10, None
clauracle.ClaudeSDKClient = FakeClient
clauracle.store.cli = lambda name: None
app = clauracle.App()
app.emit = lambda event, **d: events.append((event, d))
api = clauracle.Api(app)


def wait(cond, what):
    end = time.time() + 5
    while not cond():
        assert time.time() < end, what
        time.sleep(0.02)


def call(fn):  # run on the app's loop and wait
    async def go():
        return fn()
    return asyncio.run_coroutine_threadsafe(go(), app.loop).result(5)


# a project set up by the default account takes its GitHub account
store.account(app.cfg, "Main")["gh"] = "octo"
p = store.new_project(app.cfg, tempfile.mkdtemp())
assert p["gh"] == "octo"
api.send(p["id"], "hello")
wait(lambda: sent == ["hello"], "message reaches claude")
s = app.sessions[p["id"]]
opts = FakeClient.last.options
assert opts.cwd == p["path"] and opts.plugins[0]["path"].endswith("plugin") and "Ponytail" in opts.system_prompt["append"]

# switching the AI account keeps the project's GitHub account, and reconnects
first = FakeClient.last
api.update_project(p["id"], {"account": "Main", "ponytail": False})
wait(lambda: FakeClient.last is not first, "reconnect after a change")
assert p["gh"] == "octo" and "Ponytail" not in FakeClient.last.options.system_prompt["append"]

# switched-off connections/skills become deny rules, globally or per account
api.set_off("mcp:claude_ai_Gmail", True)
api.set_off("skill:ponytail:ponytail-audit", True, "Main")
wait(lambda: set(FakeClient.last.options.disallowed_tools) == {"mcp__claude_ai_Gmail", "Skill(ponytail:ponytail-audit)"},
     "reconnect with the switched-off items denied")

# clear-continue: refused when the loop is off, armed during a handover, then a fresh session continues
def cc():
    return asyncio.run_coroutine_threadsafe(s.clear_and_continue.handler({}), app.loop).result(5)["content"][0]["text"]


assert "OFF" in cc() and not s.armed
sent.clear()
p["session"] = "abc"
FakeClient.during = lambda: s.clear_and_continue.handler({})
call(lambda: s.hand_over())  # the handover turn calls the tool, ends -> fresh session
wait(lambda: any(x.startswith("continue") for x in sent), "fresh session continues")
assert "HANDOVER.md" in sent[0] and FakeClient.last_tool["content"][0]["text"].startswith("Armed")
assert p["session"] is None and ("cleared", {"pid": p["id"]}) in events

# the loop: usage past the limit refuses and switches the loop off
p["loop"] = True
store.record_usage(app.cfg, "Main", {"status": "allowed", "unifiedWindows": {"five_hour": {"utilization": 0.9}}})
assert "NOT cleared" in cc() and not p["loop"] and not s.armed

# auto-clear: past 50% (not on a session's first turn) the session hands over
sent.clear()
FakeClient.pct = 70
api.send(p["id"], "one")
wait(lambda: len(sent) >= 2 and "context window is at 70%" in sent[-1], "auto handover at 70%")

# the page's script parses
html = Path("index.html").read_text(encoding="utf-8")
if shutil.which("node"):
    js = store.HOME / "page.js"
    js.write_text(re.search(r"<script>(.*)</script>", html, re.S)[1], encoding="utf-8")
    r = subprocess.run(["node", "--check", str(js)], capture_output=True, text=True)
    assert r.returncode == 0, r.stderr
assert clauracle.summary("Bash", {"command": "ls\n-la"}) == "ls"
print("ok")
