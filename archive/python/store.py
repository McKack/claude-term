"""Clauracle persistence and CLI helpers: projects, Claude accounts (tokens in the OS keychain), GitHub accounts via gh."""
import json
import os
import re
import shutil
import subprocess
import threading
import time
import uuid
from pathlib import Path

import keyring

HOME = Path.home() / ".clauracle"
CONFIG = HOME / "config.json"
KEYRING_SERVICE = "clauracle"
MAIN = "Main"  # the account `claude` itself is logged in as (no stored token)
# Settings pages: GitHub (gh's accounts) · Connections & Skills (catalog, "off" = switched off everywhere) ·
# AI accounts (each with a default GitHub account and its own "off" list). Deny-lists, so new things start on.
DEFAULTS = {"projects": [], "accounts": [], "usage": {}, "active": None, "default_account": MAIN,
            "catalog": [], "off": []}
ACCOUNT = {"provider": "claude", "gh": "", "off": []}  # ponytail: provider is always claude until another exists
PROJECT = {"account": MAIN, "gh": "", "mode": "default", "model": "", "ponytail": True, "loop": False,
           "auto_clear": True, "session": None}
NO_WINDOW = 0x08000000 if os.name == "nt" else 0  # no console flash for our own helper processes
_lock = threading.Lock()


def load():
    try:
        cfg = {**DEFAULTS, **json.loads(CONFIG.read_text(encoding="utf-8"))}
    except (OSError, ValueError):
        cfg = json.loads(json.dumps(DEFAULTS))
        _import_claude_term(cfg)
    if not any(a["id"] == MAIN for a in cfg["accounts"]):
        cfg["accounts"].insert(0, {"id": MAIN, "nick": "Main"})
    cfg["accounts"] = [{**ACCOUNT, **a} for a in cfg["accounts"]]
    cfg["projects"] = [{**PROJECT, **p} for p in cfg["projects"]]
    return cfg


def save(cfg):
    with _lock:  # js_api calls arrive on several threads
        HOME.mkdir(parents=True, exist_ok=True)
        CONFIG.write_text(json.dumps(cfg, indent=2), encoding="utf-8")


def _import_claude_term(cfg):
    """First run: bring over the accounts claude-term already set up (tokens copied to our keychain entry)."""
    try:
        old = json.loads((Path.home() / ".claude-term" / "config.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return
    for a in old.get("accounts", []):
        if a.get("id") and a["id"] != MAIN and (tok := keyring.get_password("claude-term", a["id"])):
            keyring.set_password(KEYRING_SERVICE, a["id"], tok)
            cfg["accounts"].append({"id": a["id"], "nick": a.get("nick", a["id"])})


def account(cfg, account_id):
    return next((a for a in cfg["accounts"] if a["id"] == account_id), None) or cfg["accounts"][0]


def new_project(cfg, path, **extra):
    """A project is set up by the default AI account and takes that account's GitHub account. From then on the
    GitHub account belongs to the project: switching its AI account later doesn't change it."""
    acct = account(cfg, extra.pop("account", cfg["default_account"]))
    p = {**PROJECT, "id": uuid.uuid4().hex[:8], "name": os.path.basename(os.path.normpath(path)), "path": path,
         "account": acct["id"], "gh": acct["gh"], **extra}
    cfg["projects"].append(p)
    return p


def project(cfg, pid):
    return next((p for p in cfg["projects"] if p["id"] == pid), None)


# ---- PATH: a .app or desktop launcher doesn't get the login shell's PATH (claude, gh live there) ----

def fix_path():
    extra = [str(Path.home() / d) for d in (".local/bin", ".claude/local", ".npm-global/bin")]
    if os.name != "nt" and (sh := os.environ.get("SHELL")):
        try:
            out = subprocess.run([sh, "-lc", 'printf "__P__%s" "$PATH"'], capture_output=True, text=True,
                                 timeout=5).stdout
            extra += out.split("__P__")[-1].split(os.pathsep)
        except (OSError, subprocess.SubprocessError):
            pass
    extra += ["/opt/homebrew/bin", "/usr/local/bin"] if os.name != "nt" else []
    have = os.environ.get("PATH", "").split(os.pathsep)
    os.environ["PATH"] = os.pathsep.join(have + [d for d in extra if d and d not in have])


# ---- Claude accounts --------------------------------------------------------------

def account_env(account_id):
    """Env for a claude process: Main uses the CLI's own login, the others their setup-token."""
    if account_id == MAIN:
        return {}
    token = keyring.get_password(KEYRING_SERVICE, account_id)
    return {"CLAUDE_CODE_OAUTH_TOKEN": token} if token else {}


def store_token(cfg, nickname, token):
    ids = {a["id"] for a in cfg["accounts"]}
    base = re.sub(r"\W+", "-", nickname).strip("-") or "account"
    account_id = next(c for i in range(1000) if (c := f"{base}-{i}" if i else base) not in ids)
    keyring.set_password(KEYRING_SERVICE, account_id, token.strip())
    cfg["accounts"].append(entry := {**ACCOUNT, "id": account_id, "nick": nickname})
    return entry


def forget_token(cfg, account_id):
    cfg["accounts"] = [a for a in cfg["accounts"] if a["id"] != account_id or a["id"] == MAIN]
    for p in cfg["projects"]:
        if p["account"] == account_id:
            p["account"] = MAIN
    if cfg["default_account"] == account_id:
        cfg["default_account"] = MAIN
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
    if raw.get("status") == "rejected":  # hit a wall: that window is full until it resets
        entry.setdefault(raw.get("rateLimitType") or "five_hour", {"resetsAt": raw.get("resetsAt")})["utilization"] = 1
    cfg["usage"][account_id] = {**cfg["usage"].get(account_id, {}), **entry}


def usage_pct(cfg, account_id, now=None):
    """Highest utilization (0-100) across windows that haven't reset yet; None if never seen."""
    now, windows = now or time.time(), cfg["usage"].get(account_id)
    if not windows:
        return None
    return round(100 * max((w["utilization"] for w in windows.values()
                            if not w.get("resetsAt") or w["resetsAt"] > now), default=0))


# `claude setup-token` draws with Ink, which prints nothing through plain pipes: it needs a pseudo-terminal.
# Both patterns need a character after the match, so a read that stops mid-token doesn't return half of it.
SIGNIN_URL = re.compile(r"https://\S+?/oauth/authorize\?[^\s\x1b\x07]+(?=[\s\x1b\x07])")
OAUTH_TOKEN = re.compile(r"sk-ant-oat\d+-[\w-]{20,}(?=[^\w-])")


class _PosixPty:
    def __init__(self, cli):
        import pty
        self.pid, self.fd = pty.fork()
        if self.pid == 0:  # child: wide terminal so the token and URL don't wrap, then become setup-token
            import fcntl, struct, termios
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 50, 1000, 0, 0))
            os.execvp(cli, [cli, "setup-token"])

    def read(self, n):
        try:
            data = os.read(self.fd, n)
        except OSError:
            data = b""
        if not data:
            raise EOFError
        return data.decode(errors="replace")

    def terminate(self, force=True):
        try:
            os.kill(self.pid, 9)
        except OSError:
            pass


def setup_token_pty(cli):
    if os.name == "nt":
        from winpty import PtyProcess
        return PtyProcess.spawn(f'"{cli}" setup-token', dimensions=(50, 1000))
    return _PosixPty(cli)


def read_setup_token(proc, on_url):
    """Blocking: reads the setup-token screen, calls on_url(url) once it shows, returns the token (None if it quit)."""
    out, url = "", None
    while True:
        try:
            out += proc.read(4096)
        except EOFError:
            return None
        if not url and (m := SIGNIN_URL.search(out)):
            on_url(url := m[0])
        if m := OAUTH_TOKEN.search(out):
            return m[0]


# ---- GitHub through gh: each account's token is handed to whoever needs it, nobody touches `gh auth switch` ----

def gh_accounts():
    try:
        out = run(["gh", "auth", "status", "--json", "hosts"]).stdout
        return [a["login"] for a in json.loads(out)["hosts"].get("github.com", []) if a.get("state") == "success"]
    except (OSError, ValueError, KeyError):
        return []


def gh_env(user):
    """GH_TOKEN for gh, and git told to ask gh for github.com credentials (which then uses that same token)."""
    if not user:
        return {}
    token = run(["gh", "auth", "token", "-h", "github.com", "-u", user]).stdout.strip()
    if not token:
        return {}
    return {"GH_TOKEN": token, "GIT_CONFIG_COUNT": "2",
            "GIT_CONFIG_KEY_0": "credential.https://github.com.helper", "GIT_CONFIG_VALUE_0": "",
            "GIT_CONFIG_KEY_1": "credential.https://github.com.helper", "GIT_CONFIG_VALUE_1": "!gh auth git-credential"}


def gh_json(args, user, cwd=None):
    """`gh <args>` as `user`, parsed JSON (None on failure)."""
    r = run(["gh", *args], cwd=cwd, env={**os.environ, **gh_env(user)})
    try:
        return json.loads(r.stdout) if r.returncode == 0 else None
    except ValueError:
        return None


GH_CODE = re.compile(r"one-time code: (\w{4}-\w{4})")


def gh_login(on_code):
    """`gh auth login --web` for one more account. Without a terminal gh prints the device code and polls."""
    p = subprocess.Popen(["gh", "auth", "login", "--web", "-h", "github.com", "-p", "https", "--skip-ssh-key"],
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                         creationflags=NO_WINDOW, env={**os.environ, "GH_BROWSER": "echo", "BROWSER": "echo"})
    for line in p.stdout:
        if m := GH_CODE.search(line):
            on_code(m[1], "https://github.com/login/device")
    return p.wait() == 0


# ---- Connections & Skills: what the installed claude can use, so each account can switch things off ----

MCP_LINE = re.compile(r"^(\S.*?): .* - (✔|✓|!|✘|✗) ?(.*)$")


def catalog(plugin_dir):
    """MCP servers (from `claude mcp list`, the one place that also knows claude.ai connectors) and skills
    (yours, installed plugins', Clauracle's own). [{"id", "label", "kind", "note"}]."""
    items = []
    for line in run([cli("claude") or "claude", "mcp", "list"], timeout=120).stdout.splitlines():
        if m := MCP_LINE.match(line.strip()):
            items.append({"id": "mcp:" + re.sub(r"\W", "_", m[1]), "label": m[1], "kind": "mcp", "note": m[3]})
    roots = [("", Path.home() / ".claude" / "skills"), ("clauracle:", Path(plugin_dir) / "skills")]
    try:
        installed = json.loads((Path.home() / ".claude/plugins/installed_plugins.json").read_text(encoding="utf-8"))
        roots += [(name.split("@")[0] + ":", Path(v[0]["installPath"]) / "skills")
                  for name, v in installed.get("plugins", {}).items() if v]
    except (OSError, ValueError, KeyError, IndexError):
        pass
    for prefix, root in roots:
        for f in sorted(root.glob("*/SKILL.md")):
            items.append({"id": f"skill:{prefix}{f.parent.name}", "label": prefix + f.parent.name, "kind": "skill",
                          "note": prefix.rstrip(":") or "your skills"})
    return items


def deny_rules(off):
    """Catalog ids switched off -> permission rules for disallowed_tools."""
    # ponytail: an off MCP server still starts, its tools are just denied; strict_mcp_config if that matters
    return [f"mcp__{i[4:]}" if i.startswith("mcp:") else f"Skill({i[6:]})" for i in off]


def run(cmd, cwd=None, env=None, timeout=60):
    try:
        return subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, encoding="utf-8",
                              errors="replace", timeout=timeout, creationflags=NO_WINDOW)
    except (OSError, subprocess.SubprocessError) as e:
        return subprocess.CompletedProcess(cmd, 1, "", str(e))


def git_branch(cwd):
    out = run(["git", "status", "--porcelain", "-b"], cwd=cwd, timeout=5).stdout.splitlines()
    if not out:
        return ""
    dirty = len(out) - 1
    return out[0].removeprefix("## ").split("...")[0] + (f" · {dirty} changed" if dirty else "")


def cli(name):
    return shutil.which(name)


if __name__ == "__main__":
    import tempfile
    CONFIG = Path(tempfile.mkdtemp()) / "config.json"
    HOME = CONFIG.parent
    cfg = load()
    cfg["accounts"][0]["gh"] = "octo"
    p = new_project(cfg, os.path.join("x", "My App"))
    assert p["name"] == "My App" and p["account"] == MAIN and p["gh"] == "octo" and project(cfg, p["id"]) is p
    cfg["accounts"][0]["gh"] = "other"
    assert p["gh"] == "octo", "the project keeps the GitHub account it was set up with"
    assert new_project(cfg, "y", gh="cloner")["gh"] == "cloner"
    assert deny_rules(["mcp:claude_ai_Gmail", "skill:ponytail:ponytail-audit"]) == [
        "mcp__claude_ai_Gmail", "Skill(ponytail:ponytail-audit)"]
    m = MCP_LINE.match("claude.ai Gmail: https://gmailmcp.googleapis.com/mcp/v1 - ✔ Connected")
    assert m[1] == "claude.ai Gmail" and m[3] == "Connected" and not MCP_LINE.match("Checking MCP server health…")
    save(cfg)
    assert load()["projects"][0]["ponytail"] is True
    assert account_env(MAIN) == {} and gh_env("") == {}

    class FakePty:  # the screen arrives in pieces; the token is split across reads
        def __init__(self, chunks):
            self.chunks = chunks

        def read(self, n):
            if not self.chunks:
                raise EOFError
            return self.chunks.pop(0)
    tok, urls = "sk-ant-oat01-" + "Ab_9-" * 18, []
    screen = ["\x1b]8;;https://claude.com/cai/oauth/authorize?code=true&x=1", "\x07more\r\n",
              "token:\x1b[33m" + tok[:30], tok[30:] + "\x1b[39mStore this token securely."]
    assert read_setup_token(FakePty(screen), urls.append) == tok
    assert urls == ["https://claude.com/cai/oauth/authorize?code=true&x=1"]
    assert read_setup_token(FakePty(["no luck"]), urls.append) is None
    assert GH_CODE.search("! First copy your one-time code: 47D7-FFD1")[1] == "47D7-FFD1"

    soon = time.time() + 3600
    record_usage(cfg, MAIN, {"status": "allowed", "unifiedWindows": {
        "five_hour": {"utilization": 0.93, "resetsAt": soon}, "seven_day": {"utilization": 0.4, "resetsAt": soon}}})
    assert usage_pct(cfg, MAIN) == 93 and usage_pct(cfg, "x") is None and usage_pct(cfg, MAIN, now=soon + 1) == 0
    record_usage(cfg, "W", {"status": "rejected", "rateLimitType": "five_hour", "resetsAt": soon})
    assert usage_pct(cfg, "W") == 100
    p["account"] = "W"
    cfg["accounts"].append({**ACCOUNT, "id": "W", "nick": "W"})
    forget_token(cfg, "W")
    assert p["account"] == MAIN and "W" not in [a["id"] for a in cfg["accounts"]]
    print("ok", gh_accounts(), git_branch("."))
