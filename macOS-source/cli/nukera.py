#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Nukera macOS CLI — start/stop/status for the ciadpi SOCKS5 bypass engine.

Phase 1: ciadpi only. tpws is intentionally dead code (never invoked).
Everything runs as the normal user EXCEPT `web on/off`, which elevates to
root once (osascript GUI prompt) to write the /etc/hosts web-unblock pins.

Interface (mirrors the Windows run_nukera.bat pattern):
  ./nukera           -> interactive TUI menu ([1] Start [2] Stop [3] Status
                        [4] Eval strategies [5] Change strategy [6] Telegram [0] Exit;
                        argv: start|stop|status|eval|strategy|web)
  ./nukera start     -> start the bypass; if already running: do nothing, show status
  ./nukera stop      -> stop the bypass;  if not running:    do nothing, show status
  ./nukera status    -> print status + strategy (machine-readable key: value lines)
  ./nukera eval      -> run strategy auto-eval (winner applies only on full run)
  ./nukera strategy [args|--show|--list] -> set current strategy from pasted ciadpi args
  ./nukera telegram -> open Telegram app with the connect-proxy prompt (one-time wiring; proxy lifecycle is start/stop)
  ./nukera web {on|off|status} -> pin DPI-banned domains to working IPs (/etc/hosts)
"""

import os
import shlex
import signal
import socket
import subprocess
import sys
import time

APP_NAME = "nukera"
VERSION = "0.1.0"

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ENGINE_DIR = os.path.join(ROOT, "engine")
CIADPI = os.path.join(ENGINE_DIR, "ciadpi")
STRATEGY_DIR = os.path.join(ENGINE_DIR, "strategies")
STATE_DIR = os.path.join(ROOT, "state")
PID_FILE = os.path.join(STATE_DIR, "nukera.pid")
LOG_FILE = os.path.join(STATE_DIR, "ciadpi.log")
STRATEGY_FILE = os.path.join(STATE_DIR, "strategy.txt")
PROXY_SERVICES_FILE = os.path.join(STATE_DIR, "proxy_services.txt")
CONFIG_FILE = os.path.join(STATE_DIR, "config.json")
TGPROXY_SCRIPT = os.path.join(ROOT, "cli", "tgproxy.py")
TGPROXY_VENV = os.path.join(STATE_DIR, "venv")
TGPROXY_VENV_PY = os.path.join(TGPROXY_VENV, "bin", "python3")
_BUNDLED = os.environ.get("NUKERA_BUNDLED") == "1"


def _tgproxy_python():
    """Interpreter for the Telegram ws-proxy. In bundle mode the generic
    bundled Python already carries the two pip deps (cryptography/cffi),
    so no venv is needed. Otherwise use (and build, on first run) the
    per-root venv."""
    if _BUNDLED:
        return sys.executable
    return TGPROXY_VENV_PY
TGPROXY_SECRET = os.path.join(STATE_DIR, "tgproxy.secret")
TGPROXY_PID = os.path.join(STATE_DIR, "tgproxy.pid")
TGPROXY_READY = os.path.join(STATE_DIR, "tgproxy.ready")
TGPROXY_HOST = "127.0.0.1"
TGPROXY_PORT = 1443

DEFAULT_PORT = 1080
DEFAULT_STRATEGY = "custom"
START_TIMEOUT = 4.0
STOP_GRACE = 2.0
BOOTSTRAP_SERVICES = ["Wi-Fi", "Ethernet", "USB 10/100/1000 LAN"]


def _sh(args, timeout=None):
    try:
        p = subprocess.run(args, capture_output=True, timeout=timeout)
        return (p.returncode,
                (p.stdout or b"").decode("utf-8", "replace"),
                (p.stderr or b"").decode("utf-8", "replace"))
    except subprocess.TimeoutExpired:
        return 124, "", "timeout"
    except Exception:  # noqa: BLE001
        return 127, "", "error"


def _read_text(path):
    try:
        with open(path, "r") as f:
            return f.read().strip()
    except OSError:
        return ""


def _write_text(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)


def _load_config():
    import json
    try:
        with open(CONFIG_FILE, "r") as f:
            cfg = json.load(f)
    except (OSError, ValueError):
        cfg = {}
    if not isinstance(cfg, dict):
        cfg = {}
    cfg.setdefault("port", DEFAULT_PORT)
    return cfg


# ---------------------------------------------------------------- strategy
def _strategy_list():
    names = []
    for f in sorted(os.listdir(STRATEGY_DIR)):
        if f.endswith(".args"):
            names.append(f[: -len(".args")])
    return names


def _saved_strategy():
    name = _read_text(STRATEGY_FILE) or DEFAULT_STRATEGY
    if os.path.exists(os.path.join(STRATEGY_DIR, name + ".args")):
        return name
    return DEFAULT_STRATEGY


def _strategy_args(name):
    text = _read_text(os.path.join(STRATEGY_DIR, name + ".args"))
    return shlex.split(text) if text else []

def cmd_strategy(argv=None):
    """Change the current desync strategy from pasted ciadpi args.

    Unsupported / CLI-managed / junk tokens are dropped (in order, duplicates
    kept), the cleaned args are saved as strategy 'custom', and the engine
    restarts with them if it is running. With --show (or no args) prints the
    current strategy."""
    raw = " ".join(argv or []).strip()
    if not raw or raw == "--show":
        name = _saved_strategy()
        print("current strategy: %s" % name)
        print("args: %s" % " ".join(_strategy_args(name)))
        print("file: %s" % os.path.join(STRATEGY_DIR, name + ".args"))
        return 0
    if raw == "--list":
        for name in _strategy_list():
            print(name)
        return 0
    try:
        sys.path.insert(0, ROOT)
        from tools.args_filter import filter_strategy_args
    except ImportError:
        print("error: tools/args_filter.py missing — cannot filter args")
        return 1
    kept, dropped = filter_strategy_args(raw)
    if dropped:
        print("dropped (unsupported / CLI-managed / junk): %s" % " ".join(dropped))
    if not kept:
        print("error: no usable strategy args found")
        return 1
    path = os.path.join(STRATEGY_DIR, "custom.args")
    _write_text(path, " ".join(kept) + "\n")
    _write_text(STRATEGY_FILE, "custom")
    if _engine_running(_load_config()["port"])[0]:
        print("saved as strategy 'custom': %s" % " ".join(kept))
        print("restarting engine to apply...")
        cmd_stop()
        cmd_start()
    else:
        print("saved as strategy 'custom': %s" % " ".join(kept))
        print("run `nukera start` to apply")
    return 0

# ---------------------------------------------------------------- proxy
def _network_services():
    rc, out, _ = _sh(["networksetup", "-listallnetworkservices"])
    services = []
    for line in out.splitlines():
        line = line.strip()
        if not line or line.startswith("*") or line.startswith("An asterisk"):
            continue
        services.append(line)
    return services


def _active_services():
    """Services that exist and are up (getinfo succeeds)."""
    up = []
    for svc in _network_services():
        rc, _, _ = _sh(["networksetup", "-getinfo", svc])
        if rc == 0:
            up.append(svc)
    return up


def _proxy_state(services):
    result = {}
    for svc in services:
        rc, out, _ = _sh(["networksetup", "-getsocksfirewallproxy", svc])
        result[svc] = rc == 0 and "Enabled: Yes" in out
    return result


def _set_proxy(services, port, on):
    changed = []
    for svc in services:
        if on:
            _sh(["networksetup", "-setsocksfirewallproxy", svc, "127.0.0.1", str(port)])
            rc, _, _ = _sh(["networksetup", "-setsocksfirewallproxystate", svc, "on"])
        else:
            rc, _, _ = _sh(["networksetup", "-setsocksfirewallproxystate", svc, "off"])
        if rc == 0:
            changed.append(svc)
    return changed


def _socks_ready(port, timeout=START_TIMEOUT):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            s = socket.create_connection(("127.0.0.1", port), timeout=0.5)
            try:
                s.sendall(b"\x05\x01\x00")
                if s.recv(2) == b"\x05\x00":
                    return True
            finally:
                s.close()
        except OSError:
            pass
        time.sleep(0.15)
    return False


# ---------------------------------------------------------------- engine
def _pid_alive(pid):
    if not pid:
        return False
    try:
        os.kill(pid, 0)
        return True
    except PermissionError:
        # The process exists but belongs to another user (shared Mac).
        # It is alive — treat it as such; ownership is checked separately.
        return True
    except OSError:
        return False


def _engine_cmd(pid):
    rc, out, _ = _sh(["ps", "-p", str(pid), "-o", "command="])
    return out.strip() if rc == 0 else ""


def _is_our_engine(pid, port):
    cmd = _engine_cmd(pid)
    if "ciadpi" not in cmd:
        return False
    tokens = shlex.split(cmd)
    for i, t in enumerate(tokens):
        if t == "-p":
            if i + 1 < len(tokens) and tokens[i + 1] == str(port):
                return True
        elif t == "-p%s" % port:
            return True
    return False


def _all_engine_pids():
    rc, out, _ = _sh(["pgrep", "-f", "engine/ciadpi"])
    pids = []
    if rc == 0:
        for line in out.splitlines():
            try:
                pids.append(int(line))
            except (ValueError, TypeError):
                pass
    return pids


def _port_serves_socks(port, timeout=1.0):
    """Pure reachability truth: does SOMETHING on 127.0.0.1:port answer the
    SOCKS5 greeting? Machine-global check that works regardless of which
    macOS user account started the engine."""
    try:
        s = socket.create_connection(("127.0.0.1", port), timeout=timeout)
        try:
            s.settimeout(timeout)
            s.sendall(b"\x05\x01\x00")
            return s.recv(2) == b"\x05\x00"
        finally:
            s.close()
    except OSError:
        return False


def _engine_running(port=DEFAULT_PORT):
    try:
        pid = int(_read_text(PID_FILE))
    except (ValueError, TypeError):
        pid = None
    if pid and _pid_alive(pid) and _is_our_engine(pid, port):
        return True, pid
    for p in _all_engine_pids():
        if _pid_alive(p) and _is_our_engine(p, port):
            return True, p
    # No pid we can prove owns it, but something is serving the proxy port
    # (e.g. an engine started by another user on this Mac). The bypass is
    # machine-global: report it as running so the UI shows the true state.
    if _port_serves_socks(port):
        return True, None
    return False, None


def _kill_engine(port=DEFAULT_PORT):
    targets = []
    try:
        pid = int(_read_text(PID_FILE))
    except (ValueError, TypeError):
        pid = None
    if pid and _pid_alive(pid) and _is_our_engine(pid, port):
        targets.append(pid)
    for p in _all_engine_pids():
        if _pid_alive(p) and _is_our_engine(p, port) and p not in targets:
            targets.append(p)
    killed = False
    for pid in targets:
        for sig in (signal.SIGTERM, signal.SIGKILL):
            try:
                os.kill(pid, sig)
            except OSError:
                pass
            deadline = time.time() + STOP_GRACE
            while _pid_alive(pid) and time.time() < deadline:
                time.sleep(0.1)
            if not _pid_alive(pid):
                killed = True
                break
    if not killed and targets:
        try:
            os.kill(targets[0], signal.SIGKILL)
        except OSError:
            pass
    try:
        os.remove(PID_FILE)
    except OSError:
        pass
    return killed


def _log_boot_time():
    last = ""
    for line in _read_text(LOG_FILE).splitlines():
        if line.startswith("=== ") and line.endswith(" ==="):
            last = line
    return last[4:-4] if last else ""


# ---------------------------------------------------------------- commands
def _bypass_live(port=DEFAULT_PORT):
    """'Running' means the bypass actually carries traffic machine-wide:
    some engine answers on the proxy port AND the system proxy is enabled
    on at least one active network service."""
    running, _ = _engine_running(port)
    if not running:
        return False
    return any(_proxy_state(_active_services()).values())


def cmd_status():
    cfg = _load_config()
    running, pid = _engine_running(cfg["port"])
    services = _active_services()
    states = _proxy_state(services)
    enabled = [s for s in services if states.get(s)]
    live = _bypass_live(cfg["port"])
    since = _log_boot_time()
    strat_name = _saved_strategy()
    strat_args = " ".join(_strategy_args(strat_name))
    print("running: %s" % ("true" if live else "false"))
    print("pid: %s" % (pid if pid else "-"))
    print("engine: ciadpi")
    print("port: %d" % cfg["port"])
    print("strategy: %s" % (strat_args or strat_name))
    print("strategy_name: %s" % strat_name)
    print("proxy: %s" % ("enabled" if enabled else "disabled"))
    if enabled:
        print("proxy_services: %s" % ", ".join(enabled))
    if since:
        print("since: %s" % since)
    print("tgproxy: %s" % _tgproxy_status_text())
    print("status: %s" % ("running" if live else "not running"))
    return 0


def cmd_start():
    cfg = _load_config()
    port = cfg["port"]
    if _bypass_live(port):
        return cmd_status()  # fully running (engine + proxy): no-op, show status

    engine_up, engine_pid = _engine_running(port)
    strategy = _saved_strategy()
    args = ["-p", str(port)] + _strategy_args(strategy)
    pid = engine_pid

    if not engine_up:
        if not os.path.exists(CIADPI):
            print("error: engine binary not found: %s" % CIADPI)
            return 1
        os.makedirs(STATE_DIR, exist_ok=True)
        proc = None
        # ciadpi does not set SO_REUSEADDR: after a quick stop/start (or a
        # just-released port), the kernel may still hold the port in TIME_WAIT
        # and the first spawn dies with "bind: Address already in use". Retry
        # a few times before reporting a real failure.
        for attempt in range(1, 6):
            with open(LOG_FILE, "a") as log:
                log.write("=== %s ===\n" % time.strftime("%Y-%m-%d %H:%M:%S"))
                log.write("nukera start (attempt %d): %s %s\n"
                          % (attempt, CIADPI, " ".join(args)))
                log.flush()
                try:
                    proc = subprocess.Popen(
                        [CIADPI] + args,
                        stdout=log,
                        stderr=log,
                        start_new_session=True,
                        close_fds=True,
                        preexec_fn=lambda: signal.signal(signal.SIGPIPE,
                                                         signal.SIG_IGN),
                    )
                except OSError as e:
                    print("error: failed to launch engine: %s" % e)
                    return 1
            if _socks_ready(port):
                break
            _kill_engine(port)
            if attempt < 5:
                time.sleep(2.5)
            proc = None
        if proc is None:
            tail = "\n".join(_read_text(LOG_FILE).splitlines()[-6:])
            print("error: engine did not come up on 127.0.0.1:%d" % port)
            if tail:
                print("--- engine log tail ---\n%s" % tail)
            return 1
        os.makedirs(STATE_DIR, exist_ok=True)
        _write_text(PID_FILE, str(proc.pid))
        pid = proc.pid
    # engine already up (possibly under another user on this Mac): still
    # (re-)attach the machine-global layers below.

    services = [s for s in BOOTSTRAP_SERVICES if s in _active_services()]
    changed = []
    if services:
        changed = _set_proxy(services, port, True)
        _write_text(PROXY_SERVICES_FILE, "\n".join(changed) + ("\n" if changed else ""))
    else:
        print("warning: no active network service found; proxy not set")

    print("started. engine=ciadpi pid=%s port=%d strategy=%s" % (pid, port, strategy))
    if changed:
        print("system socks proxy enabled: %s" % ", ".join(changed))
    _apply_host_pins()
    try:
        _telegram_proxy_start()
    except Exception as e:
        print("warning: Telegram Desktop proxy failed to start: %s" % e)
    return 0


def cmd_stop():
    cfg = _load_config()
    port = cfg["port"]
    running, pid = _engine_running(port)
    killed = False
    if running:
        killed = _kill_engine(port)

    # Disable the proxy on every service that has it on NOW (plus anything we
    # previously recorded). Proxy + engine are machine-global, so a stale or
    # foreign-user record must not stop us from turning the proxy off — that
    # is what actually cuts the bypass for ALL users on this Mac.
    recorded = [s for s in _read_text(PROXY_SERVICES_FILE).splitlines() if s]
    on_now = [s for s in _active_services() if _proxy_state([s]).get(s)]
    disable = list(dict.fromkeys(recorded + on_now))
    cleaned = []
    if disable:
        cleaned = _set_proxy(disable, port, False)
        _write_text(PROXY_SERVICES_FILE, "")
    if running:
        if killed:
            print("stopped. engine killed (pid %s)" % pid)
        else:
            print("stopped — engine (pid %s) belongs to another user on this "
                  "Mac and could not be killed; proxy disabled, so no traffic "
                  "routes through it." % pid)
    else:
        print("stopped (no engine was running)")
    if cleaned:
        print("system socks proxy disabled: %s" % ", ".join(cleaned))
    _remove_host_pins()
    _telegram_proxy_stop()
    return 0


# ---------------------------------------------------------------- TUI menu
def cmd_menu():
    while True:
        print()
        print("  [1] Start")
        print("  [2] Stop")
        print("  [3] Status")
        print("  [4] Eval strategies")
        print("  [5] Change strategy")
        print("  [6] Telegram")
        print("  [0] Exit")
        print("  (Start/Stop run ALL bypass layers incl. IP pinning;")
        print("   manual override: ./nukera web on|off|status)")
        try:
            choice = input("\n> ").strip().lower()
        except (EOFError, KeyboardInterrupt):
            print()
            return 0
        if choice == "1":
            cmd_start()
        elif choice == "2":
            cmd_stop()
        elif choice == "3":
            cmd_status()
        elif choice == "4":
            cmd_eval()
        elif choice == "5":
            try:
                user = input("strategy args > ").strip()
            except (EOFError, KeyboardInterrupt):
                print()
                continue
            cmd_strategy(shlex.split(user) if user else [])
        elif choice in ("6", "telegram"):
            cmd_telegram()
        elif choice in ("0", "q", "quit", "exit"):
            return 0
        # anything else: just re-show the menu


# ---------------------------------------------------------------- eval
EVAL_TOOL = os.path.join(ROOT, "tools", "strategy_eval.py")
_EVAL_KEYS = (b"q", b"Q")  # user may stop a running eval at any point


def cmd_eval(argv=None):
    """Run the strategy auto-eval. Applies a winner ONLY if the full test
    completes; interrupt (q / Ctrl-C) or kill leaves the current strategy
    untouched."""
    argv = argv or []
    if "--list" in argv:
        return _run_eval(["--list"])
    if "--quick" in argv:
        print("quick mode — results are advisory, winner NOT applied")
        return _run_eval(["--quick"])
    if "--sites" in argv:
        i = argv.index("--sites")
        return _run_eval(["--sites"] + argv[i + 1:i + 2] + ["--apply"])
    return _run_eval(["--apply"])


# ---------------------------------------------------------------- web unblock
HOSTS_PIN_TOOL = os.path.join(ROOT, "tools", "hosts_pin.py")


def cmd_web(actions=None):
    """Manual override / diagnostic for the IP pinning layer. The pin is
    normally applied by `nukera start` and removed by `nukera stop`;
    use this only to toggle it without touching the engine."""
    actions = actions or []
    action = actions[0] if actions else "status"
    if action not in ("on", "off", "status"):
        print("usage: nukera web {on|off|status}")
        return 2
    return _run_hosts_pin(action)


def _run_hosts_pin(action):
    return subprocess.call([sys.executable, HOSTS_PIN_TOOL, action])


def _apply_host_pins():
    """start-time layer: makes sure the IP-pin block is present. No-op (no
    admin prompt) if already active; failure never fails the whole start."""
    if not os.path.exists(HOSTS_PIN_TOOL):
        print("warning: hosts_pin tool missing — IP pinning skipped")
        return 127
    rc = subprocess.call([sys.executable, HOSTS_PIN_TOOL, "on"])
    if rc != 0:
        print("warning: IP pinning skipped (rc=%d) — telegram may stay blocked direct" % rc)
    return rc


def _remove_host_pins():
    """stop-time layer: restores /etc/hosts. No-op if no block present.
    Quiet mode skips the post-state verification curls (engine already down)."""
    if not os.path.exists(HOSTS_PIN_TOOL):
        return 127
    env = dict(os.environ, NUKERA_QUIET="1")
    return subprocess.call([sys.executable, HOSTS_PIN_TOOL, "off"], env=env)

# ---------------------------------------------------------------- telegram app proxy
def _tgproxy_secret():
    value = _read_text(TGPROXY_SECRET)
    if len(value) == 32:
        try:
            bytes.fromhex(value)
            return value
        except ValueError:
            pass
    secret = os.urandom(16).hex()
    _write_text(TGPROXY_SECRET, secret + "\n")
    return secret


def _tgproxy_pid():
    try:
        return int(_read_text(TGPROXY_PID))
    except (ValueError, TypeError):
        return None


def _tgproxy_alive():
    pid = _tgproxy_pid()
    if not pid:
        return False
    try:
        os.kill(pid, 0)
    except (OSError, ProcessLookupError):
        return False
    try:
        cmd = subprocess.check_output(["ps", "-p", str(pid), "-o", "command="],
                                      text=True)
        return "cli/tgproxy.py" in cmd
    except (subprocess.CalledProcessError, OSError):
        return False


def _ensure_tgproxy_env():
    """One-time, no-prompt: create the state venv and install the proxy's two
    pip deps (cryptography/cffi) if not already there."""
    if _BUNDLED:
        return True
    if os.path.exists(TGPROXY_VENV_PY):
        return True
    print("Telegram Desktop proxy: installing dependencies (one-time)...")
    rc, _, err = _sh([sys.executable, "-m", "venv", TGPROXY_VENV])
    if rc != 0:
        print("warning: could not create venv: %s" % err.strip())
        return False
    rc, _, err = _sh([TGPROXY_VENV_PY, "-m", "pip", "install",
                      "cryptography", "cffi"], timeout=300)
    if rc != 0:
        print("warning: dependency install failed: %s" % err.strip())
        return False
    return True


def _telegram_proxy_start():
    """Bring the Telegram Desktop MTProto proxy up as its own process.
    Lifecycle-tied to the bypass: start spawns it, stop kills it."""
    if _tgproxy_alive():
        print("Telegram Desktop proxy: already running on %s:%s (MTProto)."
              % (TGPROXY_HOST, TGPROXY_PORT))
        return True, _tgproxy_pid()
    if not _ensure_tgproxy_env():
        return False, None
    if not os.path.exists(TGPROXY_SCRIPT):
        print("warning: tgproxy.py missing — Telegram Desktop proxy skipped")
        return False, None
    _write_text(TGPROXY_PID, "")
    try:
        os.remove(TGPROXY_READY)
    except OSError:
        pass
    secret = _tgproxy_secret()
    p = subprocess.Popen(
        [_tgproxy_python(), TGPROXY_SCRIPT, str(TGPROXY_PORT), secret, TGPROXY_READY],
        cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        start_new_session=True, close_fds=True)
    deadline = time.time() + 10.0
    while time.time() < deadline:
        if p.poll() is not None:
            print("warning: Telegram proxy process exited early (rc=%s)"
                  % p.returncode)
            return False, None
        if os.path.exists(TGPROXY_READY):
            break
        time.sleep(0.2)
    if not os.path.exists(TGPROXY_READY):
        try:
            os.kill(p.pid, signal.SIGKILL)
        except OSError:
            pass
        print("warning: Telegram proxy failed to start (no ready signal in 10s)")
        return False, None
    try:
        s = socket.create_connection((TGPROXY_HOST, TGPROXY_PORT), timeout=2.0)
        s.close()
    except OSError:
        try:
            os.kill(p.pid, signal.SIGKILL)
        except OSError:
            pass
        print("warning: Telegram proxy up but not listening on %s:%s"
              % (TGPROXY_HOST, TGPROXY_PORT))
        return False, None
    _write_text(TGPROXY_PID, "%s\n" % p.pid)
    print("Telegram Desktop proxy: running on %s:%s (MTProto)."
          % (TGPROXY_HOST, TGPROXY_PORT))
    return True, p.pid


def _telegram_proxy_stop():
    pid = _tgproxy_pid()
    if pid:
        try:
            os.kill(pid, signal.SIGTERM)
        except OSError:
            pass
    _write_text(TGPROXY_PID, "")
    try:
        os.remove(TGPROXY_READY)
    except OSError:
        pass


def _tgproxy_status_text():
    if _tgproxy_alive():
        return "enabled (%s:%s)" % (TGPROXY_HOST, TGPROXY_PORT)
    return "disabled"


def cmd_telegram():
    """Wire the installed Telegram app to our MTProto proxy once:
    opens the tg://proxy link → Telegram shows its connect prompt; the user
    just clicks OK. From then on start/stop controls the proxy."""
    if not _engine_running(_load_config()["port"])[0]:
        print("bypass is not running — start it first: `nukera start`")
        return 1
    try:
        ok, _ = _telegram_proxy_start()
    except Exception as e:
        print("warning: Telegram proxy failed: %s" % e)
        return 1
    if not ok:
        return 1
    secret = _tgproxy_secret()
    link = "tg://proxy?server=%s&port=%d&secret=dd%s" % (
        TGPROXY_HOST, TGPROXY_PORT, secret)
    print("TGPROXY_LINK=%s" % link)
    rc, _, _ = _sh(["open", link])
    if rc != 0:
        print("warning: could not open the link in Telegram (rc=%s)." % rc)
        print("  Telegram.app must be installed; paste the link manually if needed.")
        print("  %s" % link)
        return 1
    print("Opened Telegram — click OK on the 'Connect to proxy' prompt if it appears.")
    return 0

def _run_eval(extra_args):
    if not os.path.exists(EVAL_TOOL):
        print("error: eval tool not found: %s" % EVAL_TOOL)
        return 1
    print("strategy eval — press 'q' or Ctrl-C to stop (winner NOT applied on stop)")
    print("scratch engines run on %d+; the main proxy stays up" % (DEFAULT_PORT + 7))
    try:
        return _pump_eval(extra_args)
    except KeyboardInterrupt:
        print("\neval stopped by user — winner NOT applied, strategy unchanged")
        return 130


def _pump_eval(extra_args):
    """Run tools/strategy_eval.py, stream its stdout live, and let the user
    stop the test with 'q' at any moment. apply--only-on-complete lives inside
    the eval tool (winner file is written atomically at the very end)."""
    cmd = [sys.executable, EVAL_TOOL] + extra_args
    proc = None
    import select as _select

    def _forward(signum, _frame):
        """Deterministic stop: forward the signal to the eval tool (which
        sweeps engines + exits via os._exit in its own handler), then exit
        ourselves without any interpreter/thread teardown."""
        if proc is not None and proc.poll() is None:
            try:
                proc.send_signal(signum)
            except OSError:
                pass
        os._exit(128 + signum)

    _prev = {}
    for _sig in (signal.SIGINT, signal.SIGTERM):
        try:
            _prev[_sig] = signal.signal(_sig, _forward)
        except (OSError, ValueError):
            pass
    try:
        proc = subprocess.Popen(cmd, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT,
                                text=True, bufsize=1)
        stop = False
        while True:
            line = proc.stdout.readline()
            if not line and proc.poll() is not None:
                break
            if line:
                sys.stdout.write(line)
                sys.stdout.flush()
            if not stop and sys.stdin.isatty() and _select.select(
                    [sys.stdin], [], [], 0.0)[0]:
                k = sys.stdin.read(1)
                if k and k.strip().lower() == "q":
                    stop = True
                    print("\nuser stop requested — aborting eval...")
                    proc.terminate()
        rc = proc.poll()
        if stop:
            print("eval stopped — winner NOT applied, strategy unchanged")
            return 130
        if rc not in (0, None):
            print("eval failed (rc=%s) — no strategy applied" % rc)
            return 1
        print("eval complete — winner applied (if any)")
        print("running engine keeps its boot strategy; `nukera stop && nukera start` to activate")
        return 0
    finally:
        for _sig, _h in _prev.items():
            try:
                signal.signal(_sig, _h)
            except (OSError, ValueError):
                pass
        if proc and proc.poll() is None:
            _sh(["pkill", "-f", EVAL_TOOL])
            try:
                proc.wait(timeout=2)
            except (subprocess.TimeoutExpired, OSError):
                proc.kill()


def main(argv):
    if not argv:
        if sys.stdin.isatty():
            return cmd_menu()
        print("usage: nukera {start|stop|status|eval|strategy|telegram|web}", file=sys.stderr)
        return 2
    dispatch = {
        "start": cmd_start,
        "stop": cmd_stop,
        "status": cmd_status,
        "eval": lambda: cmd_eval(sys.argv[2:]),
        "strategy": lambda: cmd_strategy(sys.argv[2:]),
        "telegram": cmd_telegram,
        "web": lambda: cmd_web(sys.argv[2:]),
    }
    fn = dispatch.get(argv[0])
    if not fn:
        print("error: unknown command '%s' (expected start|stop|status|eval|strategy|telegram|web)" % argv[0])
        return 2
    try:
        return fn()
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))