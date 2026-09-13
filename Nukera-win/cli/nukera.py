"""Nukera CLI - start, stop, status for the zapret-based censorship bypass."""

import ctypes
import http.client
import os
import re
import shutil
import socket
import ssl
import subprocess
import sys
import time
import uuid
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait

# The bundled Python uses a python313._pth file, so the script's directory is
# NOT added to sys.path automatically; add it so `import tgproxy` works in the
# CLI and in the worker (both launch cli/nukera.py).
_CLI_DIR = os.path.dirname(os.path.abspath(__file__))
if _CLI_DIR not in sys.path:
    sys.path.insert(0, _CLI_DIR)

import tgproxy

APP_VERSION = "0.7.0"
DEFAULT_STRATEGY = "general (ALT11)"

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ENGINE = os.path.join(ROOT, "engine")
BIN = os.path.join(ENGINE, "bin")
LISTS = os.path.join(ENGINE, "lists")
STRATEGIES = os.path.join(ENGINE, "strategies")
STATE = os.path.join(ROOT, "state")
PID_FILE = os.path.join(STATE, "nukera.pid")
LOG_FILE = os.path.join(STATE, "winws.log")
WORKER_PID = os.path.join(STATE, "worker.pid")
JOBS = os.path.join(STATE, "jobs")
STRATEGY_FILE = os.path.join(STATE, "strategy.txt")
ADAPT_ACTIVE = os.path.join(STATE, "adapt.active")
ADAPT_CANCEL = os.path.join(STATE, "adapt.cancel")
ADAPT_JOB = os.path.join(STATE, "adapt.job")
ADAPT_TIMEOUT = 900
ADAPT_PROBE_TIMEOUT = 3.0
TGPROXY_SECRET = os.path.join(STATE, "tgproxy.secret")
TGPROXY_PID = os.path.join(STATE, "tgproxy.pid")
TGPROXY_READY = os.path.join(STATE, "tgproxy.ready")

HOSTS_FILE = os.path.join(
    os.environ.get("SystemRoot", r"C:\Windows"),
    "System32", "drivers", "etc", "hosts",
)
TELEGRAM_IP = "149.154.167.220"
TELEGRAM_HOSTS_BEGIN = "# Nukera Telegram Web hosts begin"
TELEGRAM_HOSTS_END = "# Nukera Telegram Web hosts end"
TELEGRAM_LIST_BEGIN = "# Nukera Telegram begin"
TELEGRAM_LIST_END = "# Nukera Telegram end"
# Telegram Web domains from engine/.service/hosts (Flowseal zapret-discord-youtube).
# All Telegram IPs are DPI-banned, so these domains are pinned to one known-good
# Telegram datacenter IP; the browser never uses the banned DNS-resolved addresses.
TELEGRAM_WEB_DOMAINS = [
    "my.telegram.org", "desktop.telegram.org", "macos.telegram.org",
    "oauth.telegram.org", "oauth.tg.dev",
    "cdn.telesco.pe", "cdn1.telesco.pe", "cdn2.telesco.pe", "cdn3.telesco.pe",
    "cdn4.telesco.pe", "cdn5.telesco.pe", "cdn6.telesco.pe",
    "core.telegram.org", "zws4.web.telegram.org",
    "vesta.web.telegram.org", "vesta-1.web.telegram.org",
    "venus-1.web.telegram.org", "venus.web.telegram.org",
    "telegram.me", "telegram.dog", "telegram.space", "telesco.pe", "tg.dev",
    "telegram.org", "t.me", "api.telegram.org", "td.telegram.org",
    "web.telegram.org",
    "kws2-1.web.telegram.org", "kws2.web.telegram.org",
    "kws4-1.web.telegram.org", "kws4.web.telegram.org",
    "zws2-1.web.telegram.org", "zws2.web.telegram.org",
    "zws4-1.web.telegram.org",
]
TELEGRAM_HOSTS_LINES = ["%s %s" % (TELEGRAM_IP, d) for d in TELEGRAM_WEB_DOMAINS]
# Telegram-CLI/DC ranges (https://core.telegram.org/resources/cidr.txt), merged
# into ipset-all.txt so winws desyncs Telegram traffic too (same as Zapret-GUI).
TELEGRAM_IP_RANGES = [
    "91.108.56.0/22", "91.108.4.0/22", "91.108.8.0/22", "91.108.16.0/22",
    "91.108.12.0/22", "149.154.160.0/20", "91.105.192.0/23", "91.108.20.0/22",
    "185.76.151.0/24",
    "2001:b28:f23d::/48", "2001:b28:f23f::/48", "2001:67c:4e8::/48",
    "2001:b28:f23c::/48", "2a0a:f280::/32",
]
GENERAL_LIST = os.path.join(LISTS, "list-general.txt")
IPALL_LIST = os.path.join(LISTS, "ipset-all.txt")
IPALL_BACKUP = IPALL_LIST + ".backup"

# Meta Web hosts, ported 1:1 from nukera-linux config/hosts/magila-cdn.txt
# (MagilaWEB/unblock-youtube-discord). Edge IPs that accept TLS handshakes
# with the original SNI even when the DNS-resolved IPs are DPI-blocked.
META_HOSTS_BEGIN = "# Nukera Meta hosts begin"
META_HOSTS_END = "# Nukera Meta hosts end"
META_LIST_BEGIN = "# Nukera Meta begin"
META_LIST_END = "# Nukera Meta end"
META_HOSTS = [
    # Instagram
    ("157.240.0.174", "instagram.com"),
    ("157.240.0.174", "www.instagram.com"),
    ("157.240.0.174", "b.i.instagram.com"),
    ("157.240.0.174", "help.instagram.com"),
    ("57.144.244.192", "static.cdninstagram.com"),
    ("57.144.244.192", "graph.instagram.com"),
    ("57.144.244.192", "i.instagram.com"),
    ("57.144.244.192", "api.instagram.com"),
    ("31.13.66.63", "scontent.cdninstagram.com"),
    ("31.13.66.63", "scontent-hel3-1.cdninstagram.com"),
    ("31.13.66.63", "scontent-hel3-2.cdninstagram.com"),
    ("31.13.66.63", "scontent-hel2-1.cdninstagram.com"),
    ("31.13.66.63", "scontent-lga3-1.cdninstagram.com"),
    ("31.13.66.63", "scontent-lax3-1.cdninstagram.com"),
    ("31.13.66.63", "scontent-iad3-1.cdninstagram.com"),
    ("31.13.66.63", "scontent-ord2-1.cdninstagram.com"),
    ("31.13.66.63", "scontent-dfw5-1.cdninstagram.com"),
    ("31.13.66.63", "cdninstagram.com"),
    # Facebook
    ("157.240.0.35", "fbcdn.net"),
    ("157.240.0.35", "www.fbcdn.net"),
    ("157.240.0.35", "facebook.com"),
    ("157.240.0.35", "www.facebook.com"),
    ("157.240.0.35", "fb.com"),
    ("157.240.0.35", "fbsbx.com"),
    ("57.144.244.128", "static.xx.fbcdn.net"),
    ("57.144.244.128", "scontent.xx.fbcdn.net"),
    ("57.144.244.34", "z-p42-chat-e2ee-ig.facebook.com"),
    # WhatsApp
    ("57.144.244.1", "whatsapp.com"),
    ("57.144.244.1", "www.whatsapp.com"),
    ("57.144.244.1", "web.whatsapp.com"),
    ("57.144.244.1", "faq.whatsapp.com"),
    ("57.144.245.32", "static.whatsapp.net"),
    ("57.144.245.32", "scontent.whatsapp.net"),
    ("57.144.244.1", "blog.whatsapp.com"),
    ("57.144.244.1", "g.whatsapp.net"),
    ("57.144.244.1", "whatsappbusiness.com"),
]
# Common Meta/WhatsApp CDN ranges merged into ipset-all.txt so winws desyncs
# Meta traffic at the IP level too. (The full stock ipset-all.txt is restored
# on start anyway, which already covers these — the marked block exists so the
# mapping stands alone even if the full restore is skipped.)
META_IP_RANGES = [
    "157.240.0.0/16",
    "31.13.24.0/21",
    "31.13.64.0/18",
    "179.60.192.0/22",
    "185.60.216.0/22",
    "69.171.128.0/17",
    "66.220.144.0/20",
    "45.64.40.0/22",
    "57.144.0.0/17",
    "103.4.96.0/22",
    "129.134.0.0/17",
    "185.107.56.0/22",
    "2a03:2880::/32",
    "2620:0:1c00::/40",
]
META_HOSTS_LINES = ["%s %s" % (ip, dom) for ip, dom in META_HOSTS]
META_DOMAINS = sorted({d for _, d in META_HOSTS})

# GitHub + Discord host pins we already ship in engine/.service/hosts but never
# applied. Sidequest: use every hosts/list we have.
GITHUB_HOSTS_BEGIN = "# Nukera GitHub hosts begin"
GITHUB_HOSTS_END = "# Nukera GitHub hosts end"
GITHUB_IP = "146.75.22.132"
GITHUB_DOMAINS = [
    "objects.githubusercontent.com", "raw.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "private-user-images.githubusercontent.com", "gist.githubusercontent.com",
    "avatars.githubusercontent.com", "media.githubusercontent.com",
    "avatars0.githubusercontent.com", "avatars1.githubusercontent.com",
    "avatars2.githubusercontent.com", "avatars3.githubusercontent.com",
    "avatars4.githubusercontent.com", "avatars5.githubusercontent.com",
    "avatars6.githubusercontent.com", "avatars7.githubusercontent.com",
    "avatars8.githubusercontent.com", "camo.githubusercontent.com",
    "cloud.githubusercontent.com",
]
GITHUB_HOSTS_LINES = ["%s %s" % (GITHUB_IP, d) for d in GITHUB_DOMAINS]

DISCORD_HOSTS_BEGIN = "# Nukera Discord hosts begin"
DISCORD_HOSTS_END = "# Nukera Discord hosts end"
DISCORD_IPS = ["162.159.138.232", "162.159.137.232",
               "162.159.128.233", "162.159.135.232"]
DISCORD_DOMAINS = ["discord.com", "updates.discord.com"]
DISCORD_HOSTS_LINES = ["%s %s" % (ip, d) for ip in DISCORD_IPS for d in DISCORD_DOMAINS]

PROBE_HOSTS = [("Discord", "discord.com", "/"), ("YouTube", "www.youtube.com", "/generate_204")]
TLS_TESTS = [("H", None), ("12", "1.2"), ("13", "1.3")]
PING_HOSTS = ["1.1.1.1", "8.8.8.8"]
TOKEN_LABEL = {"ok": "OK", "ssl": "SSL", "unsup": "UNS", "timeout": "TMO", "err": "ERR"}
PERFECT_SCORE = len(PROBE_HOSTS) * len(TLS_TESTS)
WINWS_READY = "windivert initialized"

WINWS = os.path.join(BIN, "winws.exe")


def _remove_quiet(path):
    try:
        if os.path.exists(path):
            os.remove(path)
    except OSError:
        pass


def _read_any_text(path, default=""):
    for enc in ("utf-8-sig", "utf-8", "utf-16", "cp1251", "latin-1"):
        try:
            with open(path, "r", encoding=enc) as f:
                return f.read()
        except (OSError, UnicodeError):
            continue
    return default


def _strip_marked_block(text, begin, end):
    return re.sub(
        r"(?ims)^%s[^\n]*\n.*?^%s[^\n]*\n?" % (re.escape(begin), re.escape(end)),
        "", text,
    )


def _write_hosts(text):
    tmp = HOSTS_FILE + ".nukera.tmp"
    for attempt in range(12):
        if os.path.exists(HOSTS_FILE):
            _run(["attrib", "-R", HOSTS_FILE])
        try:
            with open(tmp, "w", encoding="utf-8") as f:
                f.write(text)
            os.replace(tmp, HOSTS_FILE)
            return
        except OSError:
            if attempt == 11:
                raise
            time.sleep(0.5)


def _hosts_block_active(begin, alternatives=()):
    text = _read_any_text(HOSTS_FILE)
    return begin in text or any(a in text for a in alternatives)


def _set_hosts_block(begin, end, lines):
    text = _strip_marked_block(_read_any_text(HOSTS_FILE), begin, end)
    block = [begin, "# pinned by Nukera against DPI/IP bans"] + list(lines) + [end]
    if text and not text.endswith("\n"):
        text += "\n"
    if text and not text.startswith("#"):
        text = "\n" + text  # keep clear of the original hosts header samples
    text = text + "\n".join(block) + "\n"
    _write_hosts(text)


def _remove_hosts_block(begin, end):
    text = _strip_marked_block(_read_any_text(HOSTS_FILE), begin, end)
    text = text.rstrip("\r\n") + "\n"
    _write_hosts(text)


def _strip_lists_block(path, begin, end):
    text = _read_any_text(path, "")
    return re.sub(
        r"(?ims)^%s[^\n]*\n.*?^%s[^\n]*\n?" % (re.escape(begin), re.escape(end)),
        "", text,
    ).rstrip("\r\n") + "\n"


def _set_lists_block(path, begin, end, entries):
    text = _strip_lists_block(path, begin, end)
    block = "%s\n%s\n%s\n" % (begin, "\n".join(entries), end)
    text = (text if text.endswith("\n") else text + "\n") + block
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def _remove_lists_block(path, begin, end):
    text = _strip_lists_block(path, begin, end)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def _flush_dns():
    _run(["ipconfig", "/flushdns"])


def _restore_full_ipset():
    # The stock ipset-all.txt (all blocked CDN ranges) was replaced with a
    # placeholder during early bring-up. Restore the full list from the backup
    # we keep so winws desyncs every reachable blocked network, not just the
    # telegram/meta ranges we append below.
    if os.path.exists(IPALL_BACKUP) and _read_any_text(IPALL_LIST).strip() == "203.0.113.113/32":
        shutil.copy2(IPALL_BACKUP, IPALL_LIST)


def _apply_telegram_on():
    _set_hosts_block(TELEGRAM_HOSTS_BEGIN, TELEGRAM_HOSTS_END, TELEGRAM_HOSTS_LINES)
    _set_lists_block(GENERAL_LIST, TELEGRAM_LIST_BEGIN, TELEGRAM_LIST_END, TELEGRAM_WEB_DOMAINS)
    _set_lists_block(IPALL_LIST, TELEGRAM_LIST_BEGIN, TELEGRAM_LIST_END, TELEGRAM_IP_RANGES)


def _apply_meta_on():
    _set_hosts_block(META_HOSTS_BEGIN, META_HOSTS_END, META_HOSTS_LINES)
    _set_lists_block(GENERAL_LIST, META_LIST_BEGIN, META_LIST_END, META_DOMAINS)
    _set_lists_block(IPALL_LIST, META_LIST_BEGIN, META_LIST_END, META_IP_RANGES)


def _apply_github_on():
    _set_hosts_block(GITHUB_HOSTS_BEGIN, GITHUB_HOSTS_END, GITHUB_HOSTS_LINES)


def _apply_discord_on():
    _set_hosts_block(DISCORD_HOSTS_BEGIN, DISCORD_HOSTS_END, DISCORD_HOSTS_LINES)


def _web_unblock_on():
    _restore_full_ipset()
    n_meta = len(META_DOMAINS)
    _apply_telegram_on()
    _apply_meta_on()
    _apply_github_on()
    _apply_discord_on()
    _flush_dns()
    print("Web unblock active (during run):")
    print("  Telegram: %d domains -> %s" % (len(TELEGRAM_WEB_DOMAINS), TELEGRAM_IP))
    print("  Meta (Instagram/Facebook/WhatsApp): %d domains -> working edge IPs"
          % n_meta)
    print("  GitHub: %d domains, Discord: %d domains pinned." % (
        len(GITHUB_DOMAINS), len(DISCORD_DOMAINS)))
    print("  Full ipset-all (%d ranges) loaded; DNS flushed." % _ipall_range_count())


def _web_unblock_off():
    for begin, end in (
        (TELEGRAM_HOSTS_BEGIN, TELEGRAM_HOSTS_END),
        ("# ZapretGUI Telegram Web hosts begin", "# ZapretGUI Telegram Web hosts end"),
        ("# ZapretGUI Flowseal Telegram Web hosts begin",
         "# ZapretGUI Flowseal Telegram Web hosts end"),
        (META_HOSTS_BEGIN, META_HOSTS_END),
        (GITHUB_HOSTS_BEGIN, GITHUB_HOSTS_END),
        (DISCORD_HOSTS_BEGIN, DISCORD_HOSTS_END),
    ):
        _remove_hosts_block(begin, end)
    for path, begin, end in (
        (GENERAL_LIST, TELEGRAM_LIST_BEGIN, TELEGRAM_LIST_END),
        (IPALL_LIST, TELEGRAM_LIST_BEGIN, TELEGRAM_LIST_END),
        (GENERAL_LIST, META_LIST_BEGIN, META_LIST_END),
        (IPALL_LIST, META_LIST_BEGIN, META_LIST_END),
    ):
        _remove_lists_block(path, begin, end)
    _flush_dns()
    print("Web unblock removed: hosts file restored, lists cleaned, DNS flushed.")


def _ipall_range_count():
    try:
        return len([1 for ln in _read_any_text(IPALL_LIST).splitlines()
                    if "/" in ln and ln.strip() and not ln.startswith("#")])
    except Exception:
        return 0


def _telegram_proxy_secret():
    return tgproxy.load_or_create_secret(TGPROXY_SECRET)


def _tgproxy_pid():
    try:
        with open(TGPROXY_PID, "r", encoding="utf-8") as f:
            return int(f.read().strip())
    except Exception:
        return None


def _tgproxy_alive():
    pid = _tgproxy_pid()
    return bool(pid and _pid_alive(pid))


def _telegram_proxy_start():
    """Spawn the proxy as a dedicated process (the vendored proxy holds
    module-global event-loop state, so a fresh process per run is reliable).
    It is lifecycle-tied to the bypass: start brings it up, stop kills it.
    Returns (ok, pid)."""
    if _tgproxy_alive():
        print("Telegram Desktop proxy: already running on %s:%s (MTProto)."
              % (tgproxy.TGPROXY_HOST, tgproxy.TGPROXY_PORT))
        return True, _tgproxy_pid()
    _remove_quiet(TGPROXY_PID)
    _remove_quiet(TGPROXY_READY)
    secret = _telegram_proxy_secret()
    py = sys.executable
    pyw = os.path.splitext(py)[0] + "w.exe"
    if os.path.exists(pyw):
        py = pyw
    server = os.path.join(_CLI_DIR, "tgproxy.py")
    p = subprocess.Popen(
        [py, server, str(tgproxy.TGPROXY_PORT), secret, TGPROXY_READY],
        cwd=ROOT,
        creationflags=subprocess.CREATE_NO_WINDOW | 0x00000008,
    )
    deadline = time.time() + 10.0
    while time.time() < deadline:
        if p.poll() is not None:
            sys.stderr.write("Telegram proxy process exited early.\n")
            return False, None
        if os.path.exists(TGPROXY_READY):
            break
        time.sleep(0.2)
    if not os.path.exists(TGPROXY_READY):
        _kill(p.pid)
        sys.stderr.write(
            "Telegram proxy failed to start (no ready signal within 10s).\n")
        return False, None
    try:
        s = socket.create_connection(
            (tgproxy.TGPROXY_HOST, tgproxy.TGPROXY_PORT), timeout=2.0)
        s.close()
    except OSError:
        _kill(p.pid)
        sys.stderr.write("Telegram proxy started but is not listening.\n")
        return False, None
    with open(TGPROXY_PID, "w", encoding="utf-8") as f:
        f.write("%s\n" % p.pid)
    print("Telegram Desktop proxy: running on %s:%s (MTProto)."
          % (tgproxy.TGPROXY_HOST, tgproxy.TGPROXY_PORT))
    return True, p.pid


def _telegram_proxy_stop():
    pid = _tgproxy_pid()
    if pid and _pid_alive(pid):
        _kill(pid)
    _remove_quiet(TGPROXY_PID)
    _remove_quiet(TGPROXY_READY)


def _tgproxy_status_text():
    if _tgproxy_alive():
        return "enabled (%s:%s)" % (tgproxy.TGPROXY_HOST, tgproxy.TGPROXY_PORT)
    return "disabled"


def cmd_tgproxy_setup():
    # The proxy only runs while the bypass is running; setup just points
    # Telegram at it (future GUI shows the button only in running mode).
    if not _running():
        print("Bypass is not running. Start Nukera first: 'nukera start'.")
        return 1
    try:
        ok, _ = _telegram_proxy_start()
    except Exception as e:
        sys.stderr.write("Telegram proxy failed to start: %s\n" % str(e))
        return 1
    if not ok:
        return 1
    secret = _telegram_proxy_secret()
    print("TGPROXY_LINK=%s" % tgproxy.make_proxy_link(tgproxy.TGPROXY_PORT, secret))
    print("Telegram Desktop proxy is running on %s:%s."
          % (tgproxy.TGPROXY_HOST, tgproxy.TGPROXY_PORT))
    return 0


def _is_admin():
    try:
        return bool(ctypes.windll.shell32.IsUserAnAdmin())
    except Exception:
        return False


def _read_worker_pid():
    if not os.path.exists(WORKER_PID):
        return None
    try:
        with open(WORKER_PID) as f:
            return int(f.read().strip())
    except Exception:
        return None


def _write_worker_pid(pid):
    with open(WORKER_PID, "w") as f:
        f.write("%s\n" % pid)


def _worker_alive():
    pid = _read_worker_pid()
    return bool(pid) and _pid_alive(pid)


def _run(cmd, check=False):
    kwargs = {}
    if os.name == "nt":
        kwargs["creationflags"] = subprocess.CREATE_NO_WINDOW
    try:
        p = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            **kwargs
        )
        if check and p.returncode != 0:
            sys.stderr.write(p.stderr or "")
        return p.returncode, (p.stdout or ""), (p.stderr or "")
    except FileNotFoundError:
        return -1, "", "command not found: %r" % cmd[0]


def _pid_alive(pid):
    if not pid:
        return False
    code, out, _ = _run(["tasklist", "/FI", "PID eq %s" % pid, "/NH"])
    return code == 0 and str(pid) in out


def _winws_processes():
    ps = (
        "$OutputEncoding=[Console]::OutputEncoding=[Text.Encoding]::UTF8; "
        "Get-CimInstance Win32_Process -Filter \"Name='winws.exe'\" | "
        "ForEach-Object { \"$($_.ProcessId)`t$($_.ExecutablePath)`t$($_.CommandLine)\" }"
    )
    _, out, _ = _run(["powershell", "-NoProfile", "-NonInteractive", "-Command", ps])
    result = []
    for line in out.splitlines():
        parts = line.split("\t") if "\t" in line else line.split("  ")
        if not parts or not parts[0].strip().isdigit():
            continue
        result.append((int(parts[0].strip()), parts[1], parts[2] if len(parts) > 2 else ""))
    return result


def _is_ours(proc):
    pid, exe, cmdline = proc
    hay = ((exe or "") + " " + (cmdline or "")).lower().replace("/", "\\")
    return "winws.exe" in hay and os.path.normpath(BIN).lower() in hay


def _find_our_winws():
    return [p for p in _winws_processes() if _is_ours(p)]


def _tracked_pid():
    if not os.path.exists(PID_FILE):
        return None
    try:
        with open(PID_FILE) as f:
            return int(f.read().strip())
    except Exception:
        _clear_pid()
        return None


def _write_pid(pid):
    with open(PID_FILE, "w") as f:
        f.write("%s\n" % pid)


def _clear_pid():
    if os.path.exists(PID_FILE):
        try:
            os.remove(PID_FILE)
        except OSError:
            pass


def _running():
    pid = _tracked_pid()
    if pid and _pid_alive(pid):
        return pid
    ours = _find_our_winws()
    if ours:
        return ours[0][0]
    return None


def _setup_user_lists():
    placeholders = {
        "ipset-exclude-user.txt": "203.0.113.113/32",
        "list-general-user.txt": "# Never leave this file empty\ndomain.example.abc",
        "list-exclude-user.txt": "domain.example.abc",
    }
    for name, content in placeholders.items():
        path = os.path.join(LISTS, name)
        if not os.path.exists(path):
            with open(path, "w") as f:
                f.write(content + "\n")


def _enable_timestamps():
    _run(["netsh", "interface", "tcp", "set", "global", "timestamps=enabled"])


def _natural_key(name):
    return [int(t) if t.isdigit() else t for t in re.split(r"(\d+)", name)]


def _list_strategies():
    try:
        names = [os.path.splitext(f)[0] for f in os.listdir(STRATEGIES) if f.lower().endswith(".bat")]
    except OSError:
        names = []
    return sorted(names, key=_natural_key)


def _strategy_path(name):
    return os.path.join(STRATEGIES, name + ".bat")


def _split_args(text):
    tokens, cur, inq = [], "", False
    for ch in text:
        if ch == '"':
            inq = not inq
        elif ch.isspace() and not inq:
            if cur:
                tokens.append(cur)
                cur = ""
        else:
            cur += ch
    if cur:
        tokens.append(cur)
    return tokens


def _extract_launch_parts(bat):
    with open(bat, "r", encoding="utf-8", errors="replace") as f:
        raw = f.read()
    lines = raw.splitlines()
    start = None
    for i, line in enumerate(lines):
        if "%BIN%winws.exe" in line:
            start = i
            break
    if start is None:
        raise ValueError("winws launch command not found")
    collected = []
    for line in lines[start:]:
        line = line.strip()
        if not line:
            continue
        collected.append(line)
        if not line.rstrip().endswith("^"):
            break
    marker = '"%BIN%winws.exe"'
    if marker not in collected[0]:
        raise ValueError("winws marker not found")
    preamble = collected[0].split(marker, 1)[1].strip().rstrip("^").strip()
    parts = [preamble]
    for raw_line in collected[1:]:
        seg = raw_line.rstrip().rstrip("^").strip()
        if seg:
            parts.append(seg)
    return parts


def _strategy_args(name):
    parts = _extract_launch_parts(_strategy_path(name))
    full = " ".join(parts)
    full = full.replace("%BIN%", BIN + os.sep).replace("%LISTS%", LISTS + os.sep)
    full = full.replace("%GameFilterTCP%", "12").replace("%GameFilterUDP%", "12")
    return _split_args(full)


def _current_strategy():
    try:
        with open(STRATEGY_FILE) as f:
            name = f.read().strip()
        if name and os.path.exists(_strategy_path(name)):
            return name
    except (OSError, ValueError):
        pass
    return DEFAULT_STRATEGY


def _save_strategy(name):
    try:
        with open(STRATEGY_FILE, "w") as f:
            f.write(name + "\n")
        return True
    except OSError:
        return False


def _zapret_service_running():
    code, out, _ = _run(["sc", "query", "zapret"])
    if code != 0:
        return False
    state = "STOPPED"
    for line in out.splitlines():
        if "STATE" in line:
            state = line.split(":", 1)[-1].strip()
    return state.upper().startswith("RUNNING")


def _alt11_args(g_tcp="12", g_udp="12"):
    a = lambda s: os.path.join(LISTS, s)
    b = lambda s: os.path.join(BIN, s)
    return [
        "--wf-tcp=80,443,2053,2083,2087,2096,8443,%s" % g_tcp,
        "--wf-udp=443,19294-19344,50000-50100,%s" % g_udp,
        "--filter-udp=443",
        "--hostlist=" + a("list-general.txt"),
        "--hostlist=" + a("list-general-user.txt"),
        "--hostlist-exclude=" + a("list-exclude.txt"),
        "--hostlist-exclude=" + a("list-exclude-user.txt"),
        "--ipset-exclude=" + a("ipset-exclude.txt"),
        "--ipset-exclude=" + a("ipset-exclude-user.txt"),
        "--dpi-desync=fake",
        "--dpi-desync-repeats=11",
        "--dpi-desync-fake-quic=" + b("quic_initial_www_google_com.bin"),
        "--new",
        "--filter-udp=19294-19344,50000-50100",
        "--filter-l7=discord,stun",
        "--dpi-desync=fake",
        "--dpi-desync-fake-discord=" + b("ACTIVE_DISCORD_UDP.bin"),
        "--dpi-desync-fake-stun=" + b("ACTIVE_DISCORD_UDP.bin"),
        "--dpi-desync-repeats=6",
        "--new",
        "--filter-tcp=2053,2083,2087,2096,8443",
        "--hostlist-domains=discord.media",
        "--dpi-desync=fake,multisplit",
        "--dpi-desync-split-seqovl=681",
        "--dpi-desync-split-pos=1",
        "--dpi-desync-fooling=ts",
        "--dpi-desync-repeats=8",
        "--dpi-desync-split-seqovl-pattern=" + b("tls_clienthello_www_google_com.bin"),
        "--dpi-desync-fake-tls=" + b("tls_clienthello_www_google_com.bin"),
        "--new",
        "--filter-tcp=443",
        "--hostlist=" + a("list-google.txt"),
        "--ip-id=zero",
        "--dpi-desync=fake,multisplit",
        "--dpi-desync-split-seqovl=681",
        "--dpi-desync-split-pos=1",
        "--dpi-desync-fooling=ts",
        "--dpi-desync-repeats=8",
        "--dpi-desync-split-seqovl-pattern=" + b("tls_clienthello_www_google_com.bin"),
        "--dpi-desync-fake-tls=" + b("tls_clienthello_www_google_com.bin"),
        "--new",
        "--filter-tcp=80,443",
        "--hostlist=" + a("list-general.txt"),
        "--hostlist=" + a("list-general-user.txt"),
        "--hostlist-exclude=" + a("list-exclude.txt"),
        "--hostlist-exclude=" + a("list-exclude-user.txt"),
        "--ipset-exclude=" + a("ipset-exclude.txt"),
        "--ipset-exclude=" + a("ipset-exclude-user.txt"),
        "--dpi-desync=fake,multisplit",
        "--dpi-desync-split-seqovl=664",
        "--dpi-desync-split-pos=1",
        "--dpi-desync-fooling=ts",
        "--dpi-desync-repeats=8",
        "--dpi-desync-split-seqovl-pattern=" + b("tls_clienthello_max_ru.bin"),
        "--dpi-desync-fake-tls=" + b("stun2.bin"),
        "--dpi-desync-fake-tls=" + b("tls_clienthello_max_ru.bin"),
        "--dpi-desync-fake-http=" + b("tls_clienthello_max_ru.bin"),
        "--new",
        "--filter-udp=443",
        "--ipset=" + a("ipset-all.txt"),
        "--hostlist-exclude=" + a("list-exclude.txt"),
        "--hostlist-exclude=" + a("list-exclude-user.txt"),
        "--ipset-exclude=" + a("ipset-exclude.txt"),
        "--ipset-exclude=" + a("ipset-exclude-user.txt"),
        "--dpi-desync=fake",
        "--dpi-desync-repeats=11",
        "--dpi-desync-fake-quic=" + b("quic_initial_www_google_com.bin"),
        "--new",
        "--filter-tcp=80,443,8443",
        "--ipset=" + a("ipset-all.txt"),
        "--hostlist-exclude=" + a("list-exclude.txt"),
        "--hostlist-exclude=" + a("list-exclude-user.txt"),
        "--ipset-exclude=" + a("ipset-exclude.txt"),
        "--ipset-exclude=" + a("ipset-exclude-user.txt"),
        "--dpi-desync=fake,multisplit",
        "--dpi-desync-split-seqovl=664",
        "--dpi-desync-split-pos=1",
        "--dpi-desync-fooling=ts",
        "--dpi-desync-repeats=8",
        "--dpi-desync-split-seqovl-pattern=" + b("tls_clienthello_max_ru.bin"),
        "--dpi-desync-fake-tls=" + b("stun2.bin"),
        "--dpi-desync-fake-tls=" + b("tls_clienthello_max_ru.bin"),
        "--dpi-desync-fake-http=" + b("tls_clienthello_max_ru.bin"),
        "--new",
        "--filter-tcp=" + g_tcp,
        "--ipset=" + a("ipset-all.txt"),
        "--ipset-exclude=" + a("ipset-exclude.txt"),
        "--ipset-exclude=" + a("ipset-exclude-user.txt"),
        "--dpi-desync=fake,multisplit",
        "--dpi-desync-any-protocol=1",
        "--dpi-desync-cutoff=n4",
        "--dpi-desync-split-seqovl=664",
        "--dpi-desync-split-pos=1",
        "--dpi-desync-fooling=ts",
        "--dpi-desync-repeats=8",
        "--dpi-desync-split-seqovl-pattern=" + b("tls_clienthello_max_ru.bin"),
        "--dpi-desync-fake-tls=" + b("stun2.bin"),
        "--dpi-desync-fake-tls=" + b("tls_clienthello_max_ru.bin"),
        "--dpi-desync-fake-http=" + b("tls_clienthello_max_ru.bin"),
        "--dpi-desync-fake-unknown=" + b("stun2.bin"),
        "--dpi-desync-fake-unknown=" + b("tls_clienthello_max_ru.bin"),
        "--new",
        "--filter-udp=" + g_udp,
        "--ipset=" + a("ipset-all.txt"),
        "--ipset-exclude=" + a("ipset-exclude.txt"),
        "--ipset-exclude=" + a("ipset-exclude-user.txt"),
        "--dpi-desync=fake",
        "--dpi-desync-repeats=10",
        "--dpi-desync-any-protocol=1",
        "--dpi-desync-fake-unknown-udp=" + b("ACTIVE_GAME_UDP.bin"),
        "--dpi-desync-cutoff=n4",
    ]


def _log_tell():
    if not os.path.exists(LOG_FILE):
        return 0
    with open(LOG_FILE, "rb") as f:
        return os.fstat(f.fileno()).st_size


def _log_since(off):
    if not os.path.exists(LOG_FILE):
        return ""
    with open(LOG_FILE, "rb") as f:
        f.seek(off)
        return f.read().decode("utf-8", errors="replace")


def _wait_winws_ready(pid, since, timeout=4.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if not _pid_alive(pid):
            return False
        if WINWS_READY in _log_since(since):
            return True
        time.sleep(0.05)
    return _pid_alive(pid)


def _spawn_winws(name=None):
    if not name:
        name = _current_strategy()
    try:
        args = _strategy_args(name)
    except Exception:
        args = _alt11_args()
    since = _log_tell()
    with open(LOG_FILE, "ab") as logf:
        p = subprocess.Popen(
            [WINWS] + args,
            stdin=subprocess.DEVNULL,
            stdout=logf,
            stderr=logf,
            creationflags=subprocess.CREATE_NO_WINDOW | 0x00000008,
        )
    if p.poll() is not None:
        return None, since
    return p.pid, since


def _kill(pid):
    _run(["taskkill", "/PID", str(pid), "/F"])


def _stop_running():
    targets = []
    pid = _tracked_pid()
    if pid and _pid_alive(pid):
        targets.append(pid)
    for proc in _find_our_winws():
        if proc[0] not in targets:
            targets.append(proc[0])
    if not targets:
        return False
    for t in targets:
        _kill(t)
    _clear_pid()
    return True


def _tail_log(n=4000):
    if not os.path.exists(LOG_FILE):
        return ""
    with open(LOG_FILE, "rb") as f:
        f.seek(0, 2)
        size = f.tell()
        f.seek(max(0, size - n))
        return f.read().decode("utf-8", errors="replace").strip()


def _https_ctx(version):
    if not version:
        return None
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    ctx.check_hostname = True
    ctx.verify_mode = ssl.CERT_REQUIRED
    if version == "1.2":
        ctx.maximum_version = ssl.TLSVersion.TLSv1_2
    elif version == "1.3":
        ctx.minimum_version = ssl.TLSVersion.TLSv1_3
    return ctx


def _probe_https(host, path, version, timeout=ADAPT_PROBE_TIMEOUT):
    try:
        conn = http.client.HTTPSConnection(host, timeout=timeout, context=_https_ctx(version))
        try:
            conn.request("HEAD", path)
            conn.getresponse().read()
        finally:
            conn.close()
        return "ok"
    except Exception as e:
        msg = str(e).lower()
        if "certificate" in msg or ("self" in msg and "signed" in msg) or "verify" in msg:
            return "ssl"
        if "could not resolve" in msg:
            return "ssl"
        if "unsupported protocol" in msg or "wrong version number" in msg:
            return "unsup"
        if isinstance(e, socket.timeout) or "timed out" in msg:
            return "timeout"
        return "err"


def _ping_ok(host):
    code, _, _ = _run(["ping", "-n", "1", "-w", "1500", host])
    return code == 0


def _test_strategy(cancel_check=None):
    jobs = []
    for label, host, path in PROBE_HOSTS:
        for vlab, ver in TLS_TESTS:
            jobs.append((host, path, ver))
    ex = ThreadPoolExecutor(max_workers=len(jobs))
    fut_idx = {ex.submit(_probe_https, h, p, v): i for i, (h, p, v) in enumerate(jobs)}
    pending = set(fut_idx)
    results_map = {}
    try:
        while pending:
            done, pending = wait(pending, timeout=0.2, return_when=FIRST_COMPLETED)
            for f in done:
                results_map[fut_idx[f]] = f.result()
            if cancel_check and cancel_check():
                break
    finally:
        ex.shutdown(wait=False, cancel_futures=True)
    cells = ["timeout"] * len(jobs)
    for i, tok in results_map.items():
        cells[i] = tok
    ok = sum(1 for c in cells if c == "ok")
    return {"cells": cells, "ok": ok, "total": len(cells), "ping_ok": 0}


def _adapt_begin():
    try:
        os.makedirs(STATE, exist_ok=True)
    except OSError:
        pass
    _remove_quiet(ADAPT_CANCEL)
    try:
        with open(ADAPT_ACTIVE, "w") as f:
            f.write(time.strftime("%Y-%m-%d %H:%M:%S") + "\n")
    except OSError:
        pass


def _adapt_end():
    _remove_quiet(ADAPT_ACTIVE)
    _remove_quiet(ADAPT_CANCEL)


def _adapt_cancel_requested():
    return os.path.exists(ADAPT_CANCEL)


def _adapt_active():
    return os.path.exists(ADAPT_ACTIVE) and _worker_alive()


def _write_adapt_job(job):
    try:
        with open(ADAPT_JOB, "w") as f:
            f.write(job + "\n")
    except OSError:
        pass


def _read_adapt_job():
    try:
        with open(ADAPT_JOB) as f:
            v = f.read().strip()
        return v or None
    except OSError:
        return None


def _request_adapt_cancel():
    try:
        with open(ADAPT_CANCEL, "w") as f:
            f.write("1\n")
        return True
    except OSError:
        return False


def _wait_adapt_finished(timeout=40):
    job = _read_adapt_job()
    code_path = os.path.join(JOBS, job + ".code") if job else None
    deadline = time.time() + timeout
    while time.time() < deadline:
        if code_path and os.path.exists(code_path):
            return True
        if not os.path.exists(ADAPT_ACTIVE):
            return True
        if not _worker_alive():
            return True
        time.sleep(0.25)
    return False


def _restart_bypass(name, was_running):
    if not was_running:
        return
    _setup_user_lists()
    _enable_timestamps()
    pid, since = _spawn_winws(name)
    if pid and _wait_winws_ready(pid, since, 4.0):
        _write_pid(pid)
        print("Bypass restarted with %s (PID %s)." % (name, pid))
    else:
        print("Failed to restart bypass with %s." % name)


def cmd_adapt():
    _setup_user_lists()
    _restore_full_ipset()
    _enable_timestamps()
    if _zapret_service_running():
        sys.stderr.write(
            "A 'zapret' Windows service is running. Stop/remove it first"
            " (engine\\service.bat -> Remove Services), then run adapt again.\n"
        )
        return 1
    strategies = _list_strategies()
    if not strategies:
        sys.stderr.write("no strategy files found in engine/strategies\n")
        return 1
    prev = _current_strategy()
    was_running = _running()
    if _stop_running():
        print("Stopped the running bypass to test strategies.")
    _adapt_begin()
    print("Adapt: testing %d strategies (takes a couple of minutes;" % len(strategies))
    print("  send 'nukera stop' or press Ctrl+C to cancel)...")
    results = {}
    tested = 0
    failed_start = 0
    cancelled = False
    perfect = False
    try:
        base_ping = 0
        for host in PING_HOSTS:
            if _ping_ok(host):
                base_ping += 1
        cur_pid = None
        for i, name in enumerate(strategies, 1):
            if _adapt_cancel_requested():
                cancelled = True
                break
            if cur_pid:
                _kill(cur_pid)
                cur_pid = None
            try:
                _strategy_args(name)
            except Exception as e:
                print("[%d/%d] %s: skipped (%r)" % (i, len(strategies), name, e))
                failed_start += 1
                continue
            pid, since = _spawn_winws(name)
            if pid:
                cur_pid = pid
            if not pid or not _wait_winws_ready(pid, since, 3.0):
                print("[%d/%d] %s: failed to start" % (i, len(strategies), name))
                failed_start += 1
                continue
            r = _test_strategy(cancel_check=_adapt_cancel_requested)
            if cancelled or _adapt_cancel_requested():
                cancelled = True
                break
            tested += 1
            r["ping_ok"] = base_ping
            results[name] = r
            print("[%d/%d] %s: score %d/%d, ping %d/%d" % (
                i, len(strategies), name, r["ok"], r["total"], r["ping_ok"], len(PING_HOSTS)))
            if r["ok"] == PERFECT_SCORE:
                perfect = True
                print("Perfect score - no further testing needed.")
                break
    finally:
        _stop_running()
        _adapt_end()
    print("")
    if cancelled:
        print("Adapt test INTERRUPTED. Results NOT applied; keeping %s." % prev)
        _restart_bypass(prev, was_running)
        return 0
    print("\nStrategy results:\n")
    header = "%-34s %-4s %-4s %-4s %-4s %-4s %-4s  ping  score" % (
        "Strategy", "D-H", "D-12", "D-13", "Y-H", "Y-12", "Y-13")
    print(header)
    print("-" * len(header))
    rows = sorted(results.items(), key=lambda kv: (kv[1]["ok"], kv[1]["ping_ok"]), reverse=True)
    for name, r in rows:
        labels = [TOKEN_LABEL[c] for c in r["cells"]]
        print("%-34s %-4s %-4s %-4s %-4s %-4s %-4s  %d/%d  %d" % (
            name, labels[0], labels[1], labels[2], labels[3], labels[4], labels[5],
            r["ping_ok"], len(PING_HOSTS), r["ok"]))
    if not rows:
        print("No strategy could be tested (all failed to start).")
        print("Keeping %s." % prev)
        _restart_bypass(prev, was_running)
        return 1
    if perfect:
        best_name, best_r = rows[0]
        _save_strategy(best_name)
        chosen = best_name
        print("\nBest config (perfect score found early): %s" % best_name)
    elif failed_start * 2 > len(strategies):
        print("Most strategies failed to start - the test may be unreliable.")
        print("Keeping %s; NOT applying a possibly bad result." % prev)
        _restart_bypass(prev, was_running)
        return 0
    else:
        best_name, best_r = rows[0]
        if best_r["ok"] > 0:
            _save_strategy(best_name)
            chosen = best_name
            print("\nBest config: %s" % best_name)
        else:
            chosen = prev
            print("\nNo strategy passed a probe. Keeping the current default: %s" % prev)
    _restart_bypass(chosen, was_running)
    print("\nSaved choice: %s (used by 'nukera start')." % chosen)
    return 0


def _launch_worker():
    python = sys.executable
    pythonw = os.path.splitext(python)[0] + "w.exe"
    if os.path.exists(pythonw):
        python = pythonw
    if _is_admin():
        subprocess.Popen(
            [python, __file__, "worker"],
            cwd=ROOT,
            creationflags=subprocess.CREATE_NO_WINDOW | 0x00000008,
        )
        return True
    params = '"%s" worker' % __file__
    result = ctypes.windll.shell32.ShellExecuteW(None, "runas", python, params, ROOT, 0)
    return result > 32


def _ensure_worker():
    if _worker_alive():
        return True
    if os.path.exists(WORKER_PID):
        try:
            os.remove(WORKER_PID)
        except OSError:
            pass
    if not _launch_worker():
        return False
    waited = 0
    while waited < 60:
        if _worker_alive():
            return True
        time.sleep(0.5)
        waited += 1
    return False


def _call_worker(cmd, timeout=300):
    try:
        os.makedirs(JOBS, exist_ok=True)
    except OSError as e:
        sys.stderr.write("failed to create jobs dir: %s\n" % e)
        return 1
    job = uuid.uuid4().hex
    if cmd == "adapt":
        _write_adapt_job(job)
    req = os.path.join(JOBS, job + ".req")
    res = os.path.join(JOBS, job + ".res")
    code_file = os.path.join(JOBS, job + ".code")
    try:
        with open(req, "w") as f:
            f.write(cmd + "\n")
    except OSError as e:
        sys.stderr.write("failed to contact worker: %s\n" % e)
        return 1
    deadline = time.time() + timeout
    seen = 0
    while not os.path.exists(code_file) and time.time() < deadline:
        if os.path.exists(res):
            with open(res, "rb") as f:
                f.seek(seen)
                chunk = f.read()
            if chunk:
                sys.stdout.write(chunk.decode("utf-8", errors="replace"))
                sys.stdout.flush()
                seen += len(chunk)
        time.sleep(0.25)
    code = 1
    try:
        if os.path.exists(res):
            with open(res, "rb") as f:
                f.seek(seen)
                chunk = f.read()
            if chunk:
                sys.stdout.write(chunk.decode("utf-8", errors="replace"))
                sys.stdout.flush()
        if os.path.exists(code_file):
            code = int(open(code_file, "r").read().strip())
    except (ValueError, OSError) as e:
        sys.stderr.write("worker returned a broken response: %s\n" % e)
        code = 1
    if not os.path.exists(code_file):
        sys.stderr.write("worker did not respond; run status to check.\n")
        code = 1
    for f in (req, res, code_file):
        for _ in range(3):
            try:
                os.remove(f)
                break
            except OSError:
                time.sleep(0.1)
    return code


def _worker_process_one(req_path):
    job = os.path.basename(req_path)[:-4]
    res_path = os.path.join(JOBS, job + ".res")
    code_path = os.path.join(JOBS, job + ".code")
    cmd = ""
    try:
        with open(req_path) as f:
            cmd = f.read().strip()
    except OSError:
        return
    if cmd == "shutdown":
        with open(res_path, "w", encoding="utf-8") as res:
            res.write("worker stopped\n")
        with open(code_path, "w") as f:
            f.write("0\n")
        try:
            os.remove(req_path)
            os.remove(WORKER_PID)
        except OSError:
            pass
        os._exit(0)
    code = 1
    try:
        with open(res_path, "w", encoding="utf-8", buffering=1) as res:
            old_out, old_err = sys.stdout, sys.stderr
            sys.stdout, sys.stderr = res, res
            try:
                code = _execute(cmd)
                sys.stdout.flush()
                sys.stderr.flush()
            finally:
                sys.stdout, sys.stderr = old_out, old_err
    except Exception as e:
        code = 1
        try:
            with open(res_path, "a", encoding="utf-8") as res:
                import traceback
                res.write("ERROR: %r\n%s\n" % (e, "".join(
                    traceback.format_exception(type(e), e, e.__traceback__))))
        except OSError:
            pass
    try:
        with open(code_path, "w") as f:
            f.write("%s\n" % code)
    except OSError:
        pass
    try:
        os.remove(req_path)
    except OSError:
        pass


def cmd_worker():
    if not _is_admin():
        sys.stderr.write("worker must be started elevated\n")
        return 1
    try:
        os.makedirs(JOBS, exist_ok=True)
        for f in os.listdir(JOBS):
            try:
                os.remove(os.path.join(JOBS, f))
            except OSError:
                pass
    except OSError:
        pass
    _remove_quiet(ADAPT_ACTIVE)
    _remove_quiet(ADAPT_CANCEL)
    _remove_quiet(ADAPT_JOB)
    _write_worker_pid(os.getpid())
    while True:
        try:
            items = os.listdir(JOBS)
        except OSError:
            items = []
        reqs = sorted(i for i in items if i.endswith(".req"))
        for r in reqs:
            try:
                _worker_process_one(os.path.join(JOBS, r))
            except SystemExit:
                try:
                    os.remove(WORKER_PID)
                except OSError:
                    pass
                return 0
            except Exception:
                pass
        time.sleep(0.25)


def cmd_start():
    _setup_user_lists()
    _enable_timestamps()
    if _zapret_service_running():
        sys.stderr.write(
            "A 'zapret' Windows service is running. Stop/remove it first"
            " (engine\\service.bat -> Remove Services), then run start again.\n"
        )
        return 1
    if _running():
        print("already running")
        return 0
    _web_unblock_on()
    name = _current_strategy()
    pid, since = _spawn_winws(name)
    if not pid:
        sys.stderr.write("winws failed to start:\n%s\n" % _tail_log())
        _web_unblock_off()
        return 1
    if not _wait_winws_ready(pid, since, 4.0):
        sys.stderr.write("winws failed to initialize:\n%s\n" % _tail_log())
        _stop_running()
        _web_unblock_off()
        return 1
    _write_pid(pid)
    print("Nukera started (PID %s)." % pid)
    print("Strategy: %s" % name)
    try:
        _telegram_proxy_start()
    except Exception as e:
        sys.stderr.write("Warning: Telegram Desktop proxy failed to start: %s\n" % str(e))
    print("Waiting a moment, then check 'nukera status' or test a site.")
    return 0


def cmd_stop():
    stopped = _stop_running()
    _web_unblock_off()
    _telegram_proxy_stop()
    if not stopped:
        print("not running")
        return 0
    print("Nukera stopped.")
    return 0


def cmd_status():
    name = _current_strategy()
    pid = _running()
    if pid:
        print("Status: running")
        print("PID: %s" % pid)
        print("Strategy: %s" % name)
    else:
        print("Status: not running")
        print("Strategy: %s" % name)
    print("Telegram Web: %s" % (
        "enabled" if _hosts_block_active(
            TELEGRAM_HOSTS_BEGIN,
            ("# ZapretGUI Telegram Web hosts begin",
             "# ZapretGUI Flowseal Telegram Web hosts begin")) else "disabled"))
    print("Meta (Instagram/Facebook/WhatsApp): %s" % (
        "enabled" if _hosts_block_active(META_HOSTS_BEGIN) else "disabled"))
    print("GitHub pins: %s" % (
        "enabled" if _hosts_block_active(GITHUB_HOSTS_BEGIN) else "disabled"))
    print("Discord pins: %s" % (
        "enabled" if _hosts_block_active(DISCORD_HOSTS_BEGIN) else "disabled"))
    print("Telegram Desktop proxy: %s" % _tgproxy_status_text())
    return 0


def cmd_menu():
    while True:
        print("")
        print("Nukera %s - censorship bypass" % APP_VERSION)
        print("  [1] Start")
        print("  [2] Stop")
        print("  [3] Status")
        print("  [4] Adapt (find best strategy)")
        print("  [5] Setup Telegram Desktop proxy")
        print("  [0] Exit")
        try:
            choice = input("Select: ").strip().lower()
        except (EOFError, KeyboardInterrupt):
            print("")
            break
        if choice in ("1", "start"):
            dispatch("start")
        elif choice in ("2", "stop"):
            dispatch("stop")
        elif choice in ("3", "status"):
            dispatch("status")
        elif choice in ("4", "adapt"):
            dispatch("adapt")
        elif choice in ("5", "setup_telegram", "telegram"):
            dispatch("setup_telegram")
        elif choice in ("0", "q", "quit", "exit", ""):
            break
        else:
            print("Unknown option.")
    return 0


def _execute(cmd):
    cmd = cmd.lower()
    if cmd == "start":
        return cmd_start()
    if cmd == "stop":
        return cmd_stop()
    if cmd == "status":
        return cmd_status()
    if cmd == "adapt":
        return cmd_adapt()
    if cmd == "menu":
        return cmd_menu()
    if cmd == "worker":
        return cmd_worker()
    if cmd == "tgproxy-setup":
        return cmd_tgproxy_setup()
    print("unknown command: %s" % cmd)
    print("usage: nukera {start|stop|status|adapt|setup_telegram}")
    return 2


def dispatch(cmd):
    cmd = cmd.lower()
    if cmd == "adapt":
        if _adapt_active():
            print("An adapt test is already running.")
            return 0
        if not _ensure_worker():
            sys.stderr.write(
                "Administrator permission was not granted. Nukera must run as"
                " admin to manage the bypass.\n"
            )
            return 1
        try:
            return _call_worker("adapt", timeout=ADAPT_TIMEOUT)
        except KeyboardInterrupt:
            _request_adapt_cancel()
            _wait_adapt_finished()
            print("\nAdapt test stopped by user. Result NOT applied.")
            return 130
    if cmd in ("start", "stop"):
        # Always run in the worker (also when already admin): the Telegram
        # Desktop proxy is a thread of the worker process, so without the
        # worker the proxy would die with a short-lived CLI process.
        if not _ensure_worker():
            sys.stderr.write(
                "Administrator permission was not granted. Nukera must run as"
                " admin to manage the bypass.\n"
            )
            return 1
        if cmd == "stop" and _adapt_active():
            _request_adapt_cancel()
            _wait_adapt_finished()
            print("Adapt test stopped; bypass not left running.")
            return 0
        if cmd == "start" and _adapt_active():
            _request_adapt_cancel()
            _wait_adapt_finished()
            print("Adapt test stopped before start.")
        return _call_worker(cmd, timeout=300)
    if cmd == "setup_telegram":
        if not _ensure_worker():
            sys.stderr.write(
                "Administrator permission was not granted. Nukera must run as"
                " admin to manage the bypass.\n"
            )
            return 1
        code = _call_worker("tgproxy-setup", timeout=120)
        if code != 0:
            return code
        try:
            secret = open(TGPROXY_SECRET, "r", encoding="utf-8").read().strip()
        except OSError:
            secret = ""
        link = tgproxy.make_proxy_link(tgproxy.TGPROXY_PORT, secret)
        print("Link: %s" % link)
        try:
            os.startfile(link)
            print("Telegram was opened with the proxy prompt - click Connect/OK.")
        except OSError as e:
            print("Could not open Telegram automatically: %s" % e)
            print("Add the proxy manually in Telegram: Settings -> Advanced ->"
                  " Connection Type -> Use custom proxy -> MTProto Proxy.")
            print("  Server: %s" % tgproxy.TGPROXY_HOST)
            print("  Port: %s" % tgproxy.TGPROXY_PORT)
            print("  Secret: dd%s" % secret)
        return 0
    return _execute(cmd)


def main():
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(line_buffering=True)
        except Exception:
            pass
    argv = sys.argv[1:]
    if not argv:
        return cmd_menu()
    return dispatch(" ".join(argv))


if __name__ == "__main__":
    sys.exit(main())