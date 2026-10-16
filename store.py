"""Persistence: preferences, accounts (tokens live in Windows Credential Manager), session helpers."""
import hashlib
import json
import os
import re
import subprocess
import time
from pathlib import Path

import keyring

CONFIG = Path.home() / ".claude-term" / "config.json"
KEYRING_SERVICE = "claude-term"
MAIN = "Main"  # id of the account `claude` itself is logged in as (no stored token)
DEFAULTS = {
    "theme": "classic", "mode": "default", "ask_model": "claude-sonnet-5-5",
    "accounts": [{"id": MAIN, "nick": "Main"}],  # priority order, first = preferred
    "auto_switch": {"on": False, "at": 90},  # % of the 5-hour or weekly window
    "defaults": {"model": "", "effort": "", "web": True},  # new sessions start here; the chat menu overrides
    "usage": {},  # account id -> last seen rate-limit windows
    "loop_dirs": [],  # project folders with the clear-continue loop on
    "advisor": {"mode": "off", "every": 5},  # off | suggest | act; looks over the session every N Code turns
}


def load():
    try:
        cfg = {**DEFAULTS, **json.loads(CONFIG.read_text(encoding="utf-8"))}
    except (OSError, ValueError):
        cfg = json.loads(json.dumps(DEFAULTS))
    # older configs: accounts were {"name"}, Main implicit, and model/effort/web were top-level
    accts = [a if "id" in a else {"id": a["name"], "nick": a["name"]} for a in cfg["accounts"]]
    if not any(a["id"] == MAIN for a in accts):
        accts.insert(0, {"id": MAIN, "nick": "Main"})
    cfg["accounts"] = accts
    for key in ("model", "effort", "web"):
        if key in cfg:
            cfg["defaults"] = {**cfg["defaults"], key: cfg.pop(key)}
    cfg.pop("account", None)
    cfg.pop("rain_anim", None)  # code rain was removed
    return cfg


def save(cfg):
    CONFIG.parent.mkdir(parents=True, exist_ok=True)
    CONFIG.write_text(json.dumps(cfg, indent=2), encoding="utf-8")


# ---- accounts -----------------------------------------------------------------

def nick(cfg, account_id):
    return next((a["nick"] for a in cfg["accounts"] if a["id"] == account_id), account_id)


def account_env(account_id):
    """Env for the claude process: Main uses the CLI's own login, others a setup-token."""
    if account_id == MAIN:
        return {}
    token = keyring.get_password(KEYRING_SERVICE, account_id)
    return {"CLAUDE_CODE_OAUTH_TOKEN": token} if token else {}


def store_token(cfg, nickname, token):
    """Saves a setup-token under a fresh id and returns the new account entry (not yet in cfg)."""
    ids = {a["id"] for a in cfg["accounts"]}
    base = re.sub(r"\W+", "-", nickname).strip("-") or "account"
    account_id = next(f"{base}-{i}" if i else base for i in range(100) if (f"{base}-{i}" if i else base) not in ids)
    keyring.set_password(KEYRING_SERVICE, account_id, token.strip())
    return {"id": account_id, "nick": nickname}


def forget_token(account_id):
    try:
        keyring.delete_password(KEYRING_SERVICE, account_id)
    except keyring.errors.PasswordDeleteError:
        pass


def record_usage(cfg, account_id, raw):
    """Keep the windows from a rate-limit event: {"five_hour": {"utilization", "resetsAt"}, "seven_day": ...}."""
    windows = raw.get("unifiedWindows") or {}
    if not windows and raw.get("rateLimitType"):
        windows = {raw["rateLimitType"]: {"utilization": raw.get("utilization"), "resetsAt": raw.get("resetsAt")}}
    entry = {k: {"utilization": v.get("utilization") or 0, "resetsAt": v.get("resetsAt")} for k, v in windows.items()}
    if raw.get("status") == "rejected":  # hit a wall: treat that window as full until it resets
        w = entry.setdefault(raw.get("rateLimitType") or "five_hour", {"resetsAt": raw.get("resetsAt")})
        w["utilization"] = 1.0
    cfg["usage"][account_id] = {**cfg["usage"].get(account_id, {}), **entry}


def usage_pct(cfg, account_id, now=None):
    """Highest utilization (0-100) across windows that haven't reset yet; None if never seen."""
    now = now or time.time()
    windows = cfg["usage"].get(account_id)
    if not windows:
        return None
    live = [w["utilization"] for w in windows.values() if not w.get("resetsAt") or w["resetsAt"] > now]
    return round(100 * max(live, default=0))


def usage_text(cfg, account_id, now=None):
    now = now or time.time()
    windows = cfg["usage"].get(account_id) or {}
    labels = {"five_hour": "5h", "seven_day": "week"}
    parts = [f"{labels.get(k, k)} {0 if w.get('resetsAt') and w['resetsAt'] <= now else round(100 * w['utilization'])}%"
             for k, w in windows.items() if k in labels]
    return " · ".join(parts) or "usage not seen yet"


def pick_account(cfg, after=None, now=None):
    """First account in priority order that's under the auto-switch limit (after `after`, wrapping around).
    With auto-switch off, simply the top one (or `after`'s successor when asked)."""
    ids = [a["id"] for a in cfg["accounts"]]
    if after in ids:
        i = ids.index(after)
        ids = ids[i + 1:] + ids[:i]  # the ones after it, wrapping; not `after` itself
    limit = cfg["auto_switch"]["at"] if cfg["auto_switch"]["on"] else 101
    return next((i for i in ids if (usage_pct(cfg, i, now) or 0) < limit), None)


def open_setup_token_window():
    """New console running `claude setup-token`; the person copies the token it prints."""
    subprocess.Popen('start "claude setup-token" cmd /k claude setup-token', shell=True)


# ---- per-project cache (plan tasks found by the scout) ------------------------------

def _project_file(cwd, kind=""):  # kind: "" = plan tasks, "ui" = drafts, panels, last session
    name = hashlib.sha1(cwd.lower().encode()).hexdigest()[:16] + (f".{kind}" if kind else "")
    return CONFIG.parent / "projects" / (name + ".json")


def load_project(cwd, kind=""):
    try:
        return json.loads(_project_file(cwd, kind).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def save_project(cwd, data, kind=""):
    f = _project_file(cwd, kind)
    f.parent.mkdir(parents=True, exist_ok=True)
    f.write_text(json.dumps(data, indent=1), encoding="utf-8")


PLAN_NAME = re.compile(r"^(handover|plan|todo|roadmap|tasks|next|backlog|claude)[\w.-]*\.md$", re.I)


def has_plan_files(cwd):
    """Cheap pre-check before spending a model call: any plan-like markdown at the top, docs/ or .claude/?"""
    for sub in ("", "docs", ".claude", os.path.join(".claude", "plans")):
        try:
            if any(PLAN_NAME.match(n) for n in os.listdir(os.path.join(cwd, sub))):
                return True
        except OSError:
            pass
    return False


# ---- facts for the welcome screen ---------------------------------------------

def first_name():
    try:
        acct = json.loads((Path.home() / ".claude.json").read_text(encoding="utf-8")).get("oauthAccount") or {}
    except (OSError, ValueError):
        acct = {}
    name = acct.get("displayName") or (acct.get("emailAddress") or "").split("@")[0].split(".")[0]
    return (name or os.environ.get("USERNAME", "")).split(" ")[0].title()


def git_facts(cwd):
    """'main · 3 uncommitted' or '' outside a repo."""
    try:
        out = subprocess.run(["git", "status", "--porcelain", "-b"], cwd=cwd, capture_output=True,
                             text=True, timeout=3).stdout.splitlines()
    except (OSError, subprocess.SubprocessError):
        return ""
    if not out:
        return ""
    branch = out[0].removeprefix("## ").split("...")[0]
    dirty = len(out) - 1
    return f"{branch} · {dirty} uncommitted" if dirty else f"{branch} · clean"


def git_since(cwd, ms):
    """While you were away: '3 new commits, latest “fix X”' since a ms timestamp, or ''."""
    try:
        subjects = subprocess.run(["git", "log", f"--since=@{int(ms / 1000)}", "--format=%s"], cwd=cwd,
                                  capture_output=True, text=True, timeout=3).stdout.splitlines()
    except (OSError, subprocess.SubprocessError):
        return ""
    if not subjects:
        return ""
    n = len(subjects)
    return f"{n} new commit{'s' * (n > 1)} since, latest “{subjects[0][:50]}”"


def ago(ms):
    s = max(0, time.time() - ms / 1000)
    for unit, size in (("d", 86400), ("h", 3600), ("m", 60)):
        if s >= size:
            return f"{int(s // size)}{unit} ago"
    return "just now"


if __name__ == "__main__":
    assert ago(time.time() * 1000 - 7200_000) == "2h ago" and ago(time.time() * 1000) == "just now"
    assert account_env(MAIN) == {}
    assert PLAN_NAME.match("HANDOVER.md") and PLAN_NAME.match("todo-2026.md") and not PLAN_NAME.match("README.md")

    import tempfile
    CONFIG = Path(tempfile.mkdtemp()) / "config.json"
    CONFIG.write_text(json.dumps({"model": "claude-opus-5-5", "web": False, "account": "Work",
                                  "accounts": [{"name": "Work"}]}), encoding="utf-8")
    cfg = load()  # old layout migrates: Main first, defaults gathered, no top-level account
    assert [a["id"] for a in cfg["accounts"]] == [MAIN, "Work"] and "account" not in cfg
    assert cfg["defaults"] == {"model": "claude-opus-5-5", "effort": "", "web": False}

    soon, now = time.time() + 3600, time.time()
    record_usage(cfg, MAIN, {"status": "allowed", "unifiedWindows": {
        "five_hour": {"utilization": 0.93, "resetsAt": soon}, "seven_day": {"utilization": 0.4, "resetsAt": soon}}})
    assert usage_pct(cfg, MAIN) == 93 and usage_pct(cfg, "Work") is None
    assert usage_text(cfg, MAIN) == "5h 93% · week 40%"
    assert usage_pct(cfg, MAIN, now=soon + 1) == 0, "a reset window no longer counts"
    assert pick_account(cfg) == MAIN, "auto-switch off: top of the list"
    cfg["auto_switch"] = {"on": True, "at": 90}
    assert pick_account(cfg) == "Work" and pick_account(cfg, after="Work") is None  # Main is over the limit
    record_usage(cfg, "Work", {"status": "rejected", "rateLimitType": "five_hour", "resetsAt": soon})
    assert usage_pct(cfg, "Work") == 100 and pick_account(cfg) is None
    repo = tempfile.mkdtemp()
    assert git_since(repo, 0) == "", "not a repo"
    g = lambda *a: subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", *a], cwd=repo,
                                  capture_output=True)
    g("init"), g("commit", "--allow-empty", "-m", "one"), g("commit", "--allow-empty", "-m", "two")
    assert git_since(repo, (now - 60) * 1000) == "2 new commits since, latest “two”"
    assert git_since(repo, (time.time() + 60) * 1000) == ""
    print("ok", first_name(), git_facts("."))
