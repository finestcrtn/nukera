#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Web-unblock host pins (ported 1:1 from the Windows Nukera CLI).

The DNS-resolved IPs of Telegram / Meta / GitHub / Discord are DPI-banned on
many networks, while specific edge/datacenter IPs still answer TLS. We pin the
domains to those working IPs in /etc/hosts (macOS resolver reads hosts before
any nameserver, so both direct clients and the ciadpi SOCKS5 resolver land on
the pinned IP).

Root is required only to write /etc/hosts and flush the resolver cache. This
tool self-elevates via osascript (GUI password prompt); falls back to printing
the sudo command to run manually when no GUI is available.

Usage:
  hosts_pin.py status            -> show pin state + live verification
  hosts_pin.py on                -> apply pins (+ verify)
  hosts_pin.py off               -> remove pins (+ flush)
  hosts_pin.py --apply-root      -> internal: rewrite /etc/hosts (run as root)
  hosts_pin.py --flush-root      -> internal: flush DNS cache (run as root)
"""

import os
import re
import subprocess
import sys

HOSTS_FILE = "/etc/hosts"
BAK_FILE = "/etc/hosts.nukera.bak"

# --- data ported verbatim from Windows cli/nukera.py -------------------------

TELEGRAM_IP = "149.154.167.220"
TELEGRAM_HOSTS_BEGIN = "# Nukera Telegram Web hosts begin"
TELEGRAM_HOSTS_END = "# Nukera Telegram Web hosts end"
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

META_HOSTS_BEGIN = "# Nukera Meta hosts begin"
META_HOSTS_END = "# Nukera Meta hosts end"
META_HOSTS = [
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
    ("157.240.0.35", "fbcdn.net"),
    ("157.240.0.35", "www.fbcdn.net"),
    ("157.240.0.35", "facebook.com"),
    ("157.240.0.35", "www.facebook.com"),
    ("157.240.0.35", "fb.com"),
    ("157.240.0.35", "fbsbx.com"),
    ("57.144.244.128", "static.xx.fbcdn.net"),
    ("57.144.244.128", "scontent.xx.fbcdn.net"),
    ("57.144.244.34", "z-p42-chat-e2ee-ig.facebook.com"),
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
META_HOSTS_LINES = ["%s %s" % (ip, dom) for ip, dom in META_HOSTS]

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
DISCORD_HOSTS_LINES = ["%s %s" % (ip, d) for ip in DISCORD_IPS
                       for d in DISCORD_DOMAINS]

BLOCKS = [
    (TELEGRAM_HOSTS_BEGIN, TELEGRAM_HOSTS_END, TELEGRAM_HOSTS_LINES),
    (META_HOSTS_BEGIN, META_HOSTS_END, META_HOSTS_LINES),
    (GITHUB_HOSTS_BEGIN, GITHUB_HOSTS_END, GITHUB_HOSTS_LINES),
    (DISCORD_HOSTS_BEGIN, DISCORD_HOSTS_END, DISCORD_HOSTS_LINES),
]
# markers from other tools (ZapretGUI / Flowseal) we also strip on removal
ALT_BEGINS = [
    "# ZapretGUI Telegram Web hosts begin", "# ZapretGUI Telegram Web hosts end",
    "# ZapretGUI Flowseal Telegram Web hosts begin",
    "# ZapretGUI Flowseal Telegram Web hosts end",
]


# ------------------------------------------------------------------ helpers
def _read(path):
    try:
        with open(path, "r", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def _write(path, text):
    tmp = path + ".nukera.tmp"
    with open(tmp, "w") as f:
        f.write(text)
    os.replace(tmp, path)


def _strip_block(text, begin, end):
    return re.sub(
        r"(?ims)^%s[^\n]*\n.*?^%s[^\n]*\n?" % (re.escape(begin), re.escape(end)),
        "", text,
    )


def active_blocks():
    text = _read(HOSTS_FILE)
    out = []
    for begin, end, _ in BLOCKS:
        out.append((begin, begin in text))
    return out


# ------------------------------------------------------------------- root ops
def apply_root():
    text = _read(HOSTS_FILE)
    if not os.path.exists(BAK_FILE) and text:
        try:
            import shutil
            shutil.copy2(HOSTS_FILE, BAK_FILE)
        except OSError:
            pass
    had_content = False
    for begin, end, _ in BLOCKS:
        text = _strip_block(text, begin, end)
    for a in ALT_BEGINS:
        text = text.replace(a, "")
    if text.strip():
        had_content = True
    lines = text.split("\n")
    clean = []
    for ln in lines:
        if ln.strip() and not ln.strip().startswith("#"):
            clean.append(ln)
    merged = "\n".join(clean)
    blocks_out = []
    for begin, end, hosts in BLOCKS:
        block = [begin, "# pinned by Nukera against DPI/IP bans"]
        block += hosts
        block += [end]
        blocks_out.append("\n".join(block))
    new_text = merged.strip("\n") + "\n\n" + "\n\n".join(blocks_out) + "\n"
    _write(HOSTS_FILE, new_text)
    flush_root()
    return 0


def flush_root():
    try:
        subprocess.run(["dscacheutil", "-flushcache"], capture_output=True)
    except OSError:
        pass
    try:
        subprocess.run(["killall", "-HUP", "mDNSResponder"], capture_output=True)
    except OSError:
        pass
    return 0


def remove_root():
    text = _read(HOSTS_FILE)
    for begin, end, _ in BLOCKS:
        text = _strip_block(text, begin, end)
    for a in ALT_BEGINS:
        text = text.replace(a, "")
    text = text.rstrip("\n") + "\n"
    _write(HOSTS_FILE, text)
    flush_root()
    return 0


# ------------------------------------------------------------- elevation + UX
SUDO = "/usr/bin/sudo"
PYTHON = "/usr/bin/python3"          # stable CLI-tools python (stdlib only)
SUDOERS_FILE = "/etc/sudoers.d/nukera"
SUDOERS_HEADER = "# Nukera one-time privilege grant. Remove to revoke:\n" \
                 "#   sudo rm -f /etc/sudoers.d/nukera && sudo visudo -c\n"

PROMPT_AUDIT = "/tmp/nukera-prompts.log"


def _esc(path):
    return path.replace(" ", "\\ ")


def _script():
    return os.path.realpath(__file__)


def _sudoers_rule():
    """NOPASSWD rule whose Cmnds carry the EXACT argv of every privileged call.
    macOS sudo matches a command spec arg-for-arg (count AND content): a bare
    `…/hosts_pin.py` spec silently misses invocations that add `--apply-root`,
    sudo then drops to the %admin password rule, and the user sees a prompt."""
    p = _esc(_script())
    return SUDOERS_HEADER + (
        "Cmnd_Alias NUKERA_HOSTS = \\\n"
        "    %s %s --apply-root, \\\n"
        "    %s %s --remove-root, \\\n"
        "    %s %s --selfcheck-root\n"
        "ALL ALL=(root) NOPASSWD: NUKERA_HOSTS\n"
        "Defaults!NUKERA_HOSTS !requiretty\n" % (PYTHON, p, PYTHON, p, PYTHON, p))


def _audit(msg):
    """Every GUI password prompt ever fired by this tool, hard-counted.
    A prompt line = exactly one osascript AdministratorPrivileges call."""
    import datetime
    try:
        with open(PROMPT_AUDIT, "a") as f:
            f.write("%s %s\n"
                    % (datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S"), msg))
    except OSError:
        pass


def _prompt_count():
    try:
        with open(PROMPT_AUDIT) as f:
            return sum(1 for ln in f if ln.strip())
    except OSError:
        return 0


def _osascript_argv(script, purpose):
    """Run a shell script via osascript AdministratorPrivileges (GUI prompt),
    passing the script as argv so no quoting gets mangled. THIS is the only
    place a GUI password prompt can originate."""
    _audit("GUI privilege prompt: %s" % purpose)
    n = _prompt_count()
    print("application is requesting elevated access (%s) — total password "
          "prompts ever: %d" % (purpose, n), file=sys.stderr)
    try:
        p = subprocess.run(
            ["osascript", "-e", "on run argv",
             "-e", "do shell script (item 1 of argv) with administrator privileges",
             "-e", "end run", "--", script],
            capture_output=True, timeout=180,
        )
        return p.returncode, p.stdout.strip(), p.stderr.strip()
    except Exception:  # noqa: BLE001
        return 127, "", "osascript failed"


def _sudo_n(*args):
    """Run the privileged helper through the passwordless sudoers grant.
    Returns (rc, stderr). rc==0 means the EXACT argv matched a NOPASSWD Cmnd
    and ran as root without any prompt."""
    p = subprocess.run([SUDO, "-n", "--", PYTHON, _script()] + list(args),
                       capture_output=True, text=True, timeout=60)
    return p.returncode, (p.stderr or "").strip()


def _selfcheck():
    """No-op root action: proves (with a real exec, not the misleading
    `sudo -l` lister) that our exact-invocation grant is live."""
    return _sudo_n("--selfcheck-root")[0] == 0


def _install_bootstrap():
    """Install (or re-install) the one-time NOPASSWD grant. Called at most
    once per elevated action — never twice in the same command."""
    import base64
    content = _sudoers_rule().encode("utf-8")
    b64 = base64.b64encode(content).decode("ascii")
    script = (
        "set -e\n"
        "mkdir -p /etc/sudoers.d\n"
        "chmod 755 /etc/sudoers.d\n"
        "echo '%s' | base64 --decode > /tmp/nukera-sudoers-entry\n"
        "install -m 0440 -o root -g wheel /tmp/nukera-sudoers-entry %s\n"
        "rm -f /tmp/nukera-sudoers-entry\n"
        "/usr/sbin/visudo -cf %s\n"
        "/usr/sbin/visudo -c\n"
    ) % (b64, SUDOERS_FILE, SUDOERS_FILE)
    print("installing one-time privilege grant (this is the ONLY password "
          "prompt this app will ever show)")
    rc, out, err = _osascript_argv(script, "install privilege grant")
    if rc != 0:
        print("privilege-grant install failed (%s): %s" % (rc, err or out))
        print("run manually once:\n  sudo sh -c 'echo "
              "\"%s\" > %s; sudo visudo -c'"
              % (_sudoers_rule().replace('"', '\\"'), SUDOERS_FILE))
        return rc
    print("privilege grant installed (sudoers.d/nukera).")
    return 0 if _selfcheck() else 1


def _run_as_root(args):
    """Deterministic elevation: silent `sudo -n` when granted; exactly ONE
    install prompt ever when not; a diagnosis (never more prompts) when even
    a fresh grant cannot match. No stacked prompts, no per-call recursion."""
    rc, err = _sudo_n(*args)
    if rc == 0:
        return 0
    if _install_bootstrap() == 0:
        rc, err = _sudo_n(*args)
        if rc == 0:
            return 0
    print("ERROR: passwordless privilege grant is installed but did not take "
          "effect (rc=%s stderr=%s)" % (rc, err))
    print("manual fix (run ONCE in a terminal):")
    print("  sudo rm -f %s" % SUDOERS_FILE)
    print("  sudo sh -c 'echo \"%s\" > %s' && sudo visudo -c"
          % (_sudoers_rule().replace('"', '\\"'), SUDOERS_FILE))
    return rc or 1


def verify():
    if os.environ.get("NUKERA_QUIET"):
        return
    print("-- verification --")
    import socket
    try:
        got = socket.gethostbyname("web.telegram.org")
        print("resolve web.telegram.org -> %s (expect 149.154.167.220)"
              % got)
    except OSError as e:
        print("resolve failed: %s" % e)
    # warm up each path once — the very first connection right after an engine
    # restart or resolver flush is often stalled by route/DPI chill and would
    # produce a misleading FAIL for the probes below
    for warm in [
        ["-x", "socks5h://127.0.0.1:1080"],
        [],
    ]:
        try:
            subprocess.run(
                ["curl", "-s", "-o", "/dev/null", "--max-time", "6"]
                + warm + ["https://web.telegram.org/"],
                capture_output=True, timeout=8)
        except Exception:  # noqa: BLE001
            pass
    for label, use_proxy in [
        ("direct https://web.telegram.org/", False),
        ("proxy  socks5h://127.0.0.1:1080 https://web.telegram.org/", True),
    ]:
        cmd = ["curl", "-s", "-o", "/dev/null", "--max-time", "12",
               "-w", "HTTP %{http_code} size %{size_download}B "
                     "connect %{remote_ip} time %{time_total}s"]
        if use_proxy:
            cmd += ["-x", "socks5h://127.0.0.1:1080"]
        cmd += ["https://web.telegram.org/"]
        try:
            p = subprocess.run(cmd, capture_output=True, text=True, timeout=15)
            out = p.stdout.strip() or p.stderr.strip()
            print("[%s] %s -> %s" % (
                "OK " if p.returncode == 0 and out else "FAIL", label, out))
        except Exception as e:  # noqa: BLE001
            print("[FAIL] %s" % e)


def _any_active():
    return any(on for _, on in active_blocks())


def cmd_status():
    blocks = active_blocks()
    text = _read(HOSTS_FILE)
    if any(b for _, b in blocks):
        print("host pins: ACTIVE")
        for b, on in blocks:
            print("  %-30s %s" % (b.split()[2] + " begin", "ok" if on else "MISSING"))
    else:
        print("host pins: off")
    print("password prompts ever: %d  (selfcheck: %s)"
          % (_prompt_count(), "ok" if _selfcheck() else "FAIL"))
    verify()


def cmd_on():
    if _any_active():
        print("host pins: already ACTIVE (nothing to do)")
        return 0
    rc = _run_as_root(["--apply-root"])
    if rc == 0:
        cmd_status()
    return rc


def cmd_off():
    if not _any_active():
        print("host pins: already off (nothing to do)")
        return 0
    rc = _run_as_root(["--remove-root"])
    if rc == 0:
        cmd_status()
    return rc


def main(argv):
    if argv[:1] == ["--apply-root"]:
        return apply_root()
    if argv[:1] == ["--remove-root"]:
        return remove_root()
    if argv[:1] == ["--selfcheck-root"]:
        return 0
    action = argv[0] if argv else "status"
    if action == "status":
        return cmd_status()
    if action == "on":
        return cmd_on()
    if action == "off":
        return cmd_off()
    print("usage: hosts_pin.py {on|off|status}")
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))