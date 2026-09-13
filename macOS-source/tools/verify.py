#!/usr/bin/env python3
"""Traffic verification for the nukera bypass (Phase 1).

Layered methodology -- each layer fails fast with the exact reason:

  L1 engine control plane : `./nukera status` -> is the engine running, which
                            port, which strategy. No engine -> nothing to route.
  L2 engine speaks SOCKS  : raw SOCKS5 no-auth greeting `\x05\x01\x00` on
                            127.0.0.1:<port> must answer `\x05\x00`. Proves the
                            listener is a working SOCKS server, not a dead socket.
  L3 what the OS tells apps: `scutil --proxy`. This is the proxy config the
                            SYSTEM actually hands to every app (Chrome included).
                            If the primary network service is a VPN tunnel, the
                            proxy set on Wi-Fi may NOT surface here -> apps go
                            direct. This layer catches that VPN-primary failure.
  L4 live app traffic      : drive an isolated Chrome (fresh profile, CDP port)
                            loading a page for a target host, while sampling
                            `lsof -nP -i` every ~300ms. Classify each observed
                            connection by pid:
                              chrome -> 127.0.0.1:<port>     ROUTED via app
                              ciadpi -> <target-ip>:443      HOP  (app reaches out)
                              chrome -> <target-ip>:443      LEAK (direct, bypasses)
                            chrome -> <target-ip>:443 (UDP)  QUIC leak indicator
    PASS requires ROUTED seen + HOP seen + no LEAK for the target host.

Detector sanity (--control): run the same L4 with the bypass OFF and expect a
LEAK (direct connection). If the OFF run also shows nothing, the detector cannot
see the difference -> the method is broken, trust nothing.

Traffic direction proof: id of the conn is what the app sends through the kernel;
lsof reports the actual kernel socket, so this is ground truth, not inference.

Usage:
  tools/verify.py [--live|--control|--all] [--target URL] [--sample SEC]

  --live    bypass ON  (auto `./nukera start` if stopped) -> the real proof.
  --control bypass OFF (auto `./nukera stop` if running)  -> detector sanity.
  --all     run both, control last (leaves bypass OFF). default.
  --target  default https://www.cloudflare.com/cdn-cgi/trace
  --sample  sampling window seconds per phase (default 15)
"""

import argparse
import ipaddress
import json
import os
import re
import shutil
import socket
import ssl
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NUKERA = os.path.join(ROOT, "nukera")
STATE = os.path.join(ROOT, "state")
PID_FILE = os.path.join(STATE, "nukera.pid")
OUT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "verify-out")
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

GOOD, BAD, WARN = "PASS", "FAIL", "WARN"

# ---------------------------------------------------------------- plumbing ----

def run(cmd, timeout=30, check=False):
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    if check and p.returncode != 0:
        raise RuntimeError("cmd failed rc=%s: %s\n%s" % (p.returncode, cmd, p.stderr))
    return p


def status():
    p = run([NUKERA, "status"])
    d = {}
    for line in p.stdout.splitlines():
        if ":" in line:
            k, _, v = line.partition(":")
            d[k.strip()] = v.strip()
    return d


def read_pid():
    try:
        with open(PID_FILE) as f:
            return int(f.read().strip())
    except (OSError, ValueError):
        return None


# ---------------------------------------------------------------- L1..L3 ----

def l1_ensure_engine(need_running):
    st = status()
    if need_running:
        if st.get("running") == "true":
            return "engine already up", True
        run([NUKERA, "start"], check=True)
        st = status()
        ok = st.get("running") == "true"
        return "started: %s" % json.dumps({k: st[k] for k in ("pid", "port", "strategy") if k in st}), ok
    if st.get("running") == "true":
        run([NUKERA, "stop"], check=True)
        return "stopped (control)", False
    return "engine down (as required)", False


def l2_socks_speaks(port, timeout=5):
    try:
        s = socket.create_connection(("127.0.0.1", port), timeout=timeout)
        try:
            s.sendall(b"\x05\x01\x00")
            r = s.recv(2)
            return r == b"\x05\x00", "greeting %r" % r
        finally:
            s.close()
    except OSError as e:
        return False, str(e)


def l3_system_proxy_present(port):
    p = run(["scutil", "--proxy"])
    txt = p.stdout
    m_en = re.search(r"SOCKSEnable\s*:\s*(\d)", txt)
    m_p = re.search(r"SOCKSProxy\s*:\s*([0-9.]+)", txt)
    m_pof = re.search(r"SOCKSPort\s*:\s*(\d+)", txt)
    enabled = m_en and m_en.group(1) == "1"
    host_ok = bool(m_p and m_p.group(1) == "127.0.0.1")
    port_ok = bool(m_pof and int(m_pof.group(1)) == port)
    why = []
    if not enabled:
        why.append("SOCKSEnable != 1")
    if not host_ok:
        why.append("SOCKSProxy != 127.0.0.1")
    if not port_ok:
        why.append("SOCKSPort != %d" % port)
    return enabled and host_ok and port_ok, "; ".join(why) or txt.splitlines()[0]


# ---------------------------------------------------------------- L4 --------

def resolve_ips(host):
    try:
        return {i[4][0] for i in socket.getaddrinfo(host, 443, proto=socket.IPPROTO_TCP)}
    except OSError as e:
        return set(), str(e)


LSOF_RE = re.compile(
    r"^(\S+)\s+(\d+)\s+\S+\s+\S+\s+\S+\s+\S+\s+\S+\s+(TCP|UDP)\s+(\S+?)(?:->(\S+?))?\s+(?:\(([^)]*)\))?\s*$")

def lsof_snapshot(cmds):
    """Ret per-connection rows: (cmd, pid, proto, local, remote, state)."""
    rows = []
    p = subprocess.run(["lsof", "-nP", "-i"] + cmds, capture_output=True, text=True, timeout=10)
    for line in p.stdout.splitlines():
        m = LSOF_RE.match(line)
        if not m:
            continue
        cmd, pid, proto, local, remote, state = m.groups()
        rows.append((cmd, int(pid), proto, local, remote, state))
    return rows


def host_of(spec):
    spec = spec.strip("[]")
    if ":" in spec:
        spec, _ = spec.rsplit(":", 1)
    return spec.strip("[]")


def is_public(spec):
    """True for a remote address pointing into the public internet."""
    spec = spec.strip("[]")
    host, _, port = spec.rpartition(":")
    if not host or not port.isdigit():
        return False
    try:
        ip = ipaddress.ip_address(host.strip("[]"))
    except ValueError:
        return False
    return not (ip.is_loopback or ip.is_private or ip.is_link_local
                or ip.is_reserved or ip.is_multicast)


COUNTERS = ["routed", "hop", "leak", "quic"]


def sample_lsof(chrome_pids, engine_pid, engine_port, target_ips, seconds):
    """Sample, returning counters + raw rows for artifacts.

    routed = chrome -> 127.0.0.1:<engine_port>  (the app covers the browser)
    hop    = ciadpi -> public:<443>             (the app reaches the internet)
    leak   = chrome -> target-ip:<443> direct   (bypasses the app)
    quic   = chrome -> target-ip:UDP/443        (UDP not carried by SOCKS)
    """
    routed = hop = leak = quic = 0
    raw = []
    deadline = time.time() + seconds
    while time.time() < deadline:
        for row in lsof_snapshot([]):
            raw.append(row)
            cmd, pid, proto, local, remote, state = row
            rhost = host_of(remote) if remote else None
            if pid in chrome_pids:
                if proto == "TCP" and remote == "127.0.0.1:%d" % engine_port:
                    routed += 1
                elif remote is not None and rhost in target_ips:
                    if proto == "TCP":
                        leak += 1
                    elif proto == "UDP":
                        quic += 1
            elif pid == engine_pid and remote is not None and proto == "TCP" \
                    and remote.endswith(":443") and is_public(remote):
                hop += 1
        time.sleep(0.3)
    return routed, hop, leak, quic, raw


def gather_target_ips(target_url):
    host = urllib.parse.urlparse(target_url).hostname
    ips, err = resolve_ips(host)
    if not ips:
        raise RuntimeError("cannot resolve %s: %s" % (host, err))
    return host, ips


def chrome_pids_tree(root_pid):
    """All live descendants of root_pid (Chrome helpers own the sockets)."""
    p = subprocess.run(["ps", "-axo", "pid=,ppid="], capture_output=True, text=True)
    children = {}
    for ln in p.stdout.splitlines():
        pid, ppid = ln.split()
        children.setdefault(int(ppid), []).append(int(pid))
    seen, stack = set(), [root_pid]
    while stack:
        cur = stack.pop()
        for kid in children.get(cur, []):
            if kid not in seen:
                seen.add(kid)
                stack.append(kid)
    return {root_pid} | seen


def cdp_nav(cdp_port, url):
    q = urllib.parse.quote(url, safe="")
    try:
        req = urllib.request.Request("http://127.0.0.1:%d/json/new?%s" % (cdp_port, q),
                                     method="PUT")
        with urllib.request.urlopen(req, timeout=4) as r:
            return r.status == 200
    except OSError:
        return False


def socks5_get(target_url, srv_host, srv_port, timeout=15):
    """HTTPS round-trip THROUGH the app's SOCKS server, via curl (its TLS
    negotiates through ciadpi's desync fragmentation; a raw `ssl` wrapper does
    not). Returns (http_status_line, body_bytes)."""
    if shutil.which("curl"):
        p = subprocess.run(
            ["curl", "--socks5-hostname", "%s:%d" % (srv_host, srv_port),
             "-sS", "--max-time", str(timeout), "-o", "/dev/null",
             "-w", "%{http_code}\n%{num_connects}\n", target_url],
            capture_output=True, text=True)
        out = p.stdout.strip()
        code = out.split("\n")[0] if out else "(no-response)"
        n = out.split("\n")[1] if out and "\n" in out else "-"
        return code, int(n or 0)
    raise RuntimeError("curl required for app-level round-trip")


def launch_chrome_profile(target_url, cdp_port, engine_port, engine_pid,
                          target_ips, sample_seconds, artifacts):
    prof = tempfile.mkdtemp(prefix="nukera-verify-chrome-")
    proc = subprocess.Popen(
        [CHROME,
         "--user-data-dir=%s" % prof,
         "--remote-debugging-port=%d" % cdp_port,
         "--no-first-run", "--no-default-browser-check",
         "--disable-background-networking", "--disable-sync",
         "--no-service-autorun", "--disable-default-apps",
         "about:blank"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    deadline = time.time() + 15
    while time.time() < deadline and proc.poll() is None:
        pids = chrome_pids_tree(proc.pid)
        if any(pid in pids for _, pid, *_ in lsof_snapshot([])):
            break
        time.sleep(0.2)
    time.sleep(1.0)
    chrome_pids = chrome_pids_tree(proc.pid)

    deadline = time.time() + 10
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(
                    "http://127.0.0.1:%d/json/version" % cdp_port, timeout=2):
                break
        except OSError:
            time.sleep(0.3)
    navs = 1 if cdp_nav(cdp_port, target_url) else 0

    routed = hop = leak = quic = 0
    raws = []
    remaining = sample_seconds
    while remaining > 0:
        chunk = min(4.0, remaining)
        r, h, l, q, raw = sample_lsof(chrome_pids, engine_pid, engine_port,
                                      target_ips, chunk)
        routed += r; hop += h; leak += l; quic += q
        raws.extend(raw)
        remaining -= chunk
        if remaining > 0 and cdp_nav(cdp_port, target_url):
            navs += 1

    artifacts["%s_routed.txt" % _tag(target_url)] = "\n".join(repr(x) for x in raws) \
        + "\n# pids=%s navs=%d\n" % (sorted(chrome_pids), navs)
    try:
        proc.kill()
    except OSError:
        pass
    return chrome_pids, prof, (routed, hop, leak, quic, navs)


def _tag(target_url):
    host = urllib.parse.urlparse(target_url).hostname.replace(".", "_")
    return "%d_%s" % (int(time.time()), host)


# ---------------------------------------------------------------- main ------

def phase(name, need_running, chrome, cdp_port, engine_state, target_url,
          sample_seconds, artifacts):
    print("\n=== %s (bypass %s) ===" % (name, "ON" if need_running else "OFF"))

    mess, ok = l1_ensure_engine(need_running)
    print("  L1 engine   :", ("%-40s " % mess), GOOD if ok else BAD)
    engine_state = status()

    if chrome:
        host, target_ips = gather_target_ips(target_url)
        engine_pid = read_pid()
        port = int(engine_state["port"]) if "port" in engine_state else 1080
        chrome_pids, prof, (routed, hop, leak, quic, navs) = launch_chrome_profile(
            target_url, cdp_port, port, engine_pid, target_ips,
            sample_seconds, artifacts)
        job = ("cdp=%s profile=%s pids=%s navs=%d"
               % (cdp_port, prof, sorted(chrome_pids), navs))
    else:
        chrome_pids, prof, job = set(), None, "browserless"
        routed = hop = leak = quic = 0

    hdr = "  phase %-30s run took 0s"
    port = int(engine_state["port"]) if "port" in engine_state else 1080
    ok2, why2 = l2_socks_speaks(port)
    print("  L2 socks    :", ("%-40s " % (why2 if ok2 else "FAIL: " + why2)).strip(),
          GOOD if ok2 else BAD)

    ok3, why3 = l3_system_proxy_present(port)
    print("  L3 os->apps :", ("%-40s " % why3[:38]), GOOD if ok3 else BAD)

    if need_running:
        code = n = None
        for _ in range(3):
            time.sleep(1.0)
            code, n = socks5_get(target_url, "127.0.0.1", port)
            if code == "200":
                break
        ok5 = code == "200"
        print("  L3.5 app dev:", ("%-40s " % ("HTTP %s via app (curls=%s)" % (code, n))),
              GOOD if ok5 else BAD)
    else:
        ok5, code = True, "engine off (skipped)"

    host, target_ips = gather_target_ips(target_url)
    engine_pid = read_pid()
    print("  L4 traffic  :")
    print("      chrome -> 127.0.0.1:%d (routed) : %d" % (port, routed))
    print("      ciadpi -> public:<443>  (hop)   : %d" % (hop, ))
    print("      chrome -> %s:<443>   (leak)  : %d" % (host, leak))
    print("      chrome -> %s:UDP443  (quic)  : %d" % (host, quic))

    verdict = BAD
    reasons = []
    if need_running:
        if not ok:
            reasons.append("engine down")
        if not ok2:
            reasons.append("SOCKS not speaking")
        if not ok3:
            reasons.append("OS not presenting proxy to apps")
        if not ok5:
            reasons.append("target did not respond through the app (%s)" % code[:30])
        if routed == 0:
            reasons.append("no chrome->app connection observed")
        if hop == 0:
            reasons.append("no app->internet connection observed")
        if leak > 0:
            reasons.append("%d DIRECT connection(s) bypass the app" % leak)
        if quic > 0:
            reasons.append("%d UDP/QUIC (becomes the app's job? not through it)" % quic)
        if not reasons:
            verdict = GOOD
    else:
        # detector sanity: OFF run must show a direct connection
        if leak > 0:
            verdict = GOOD
            reasons.append("direct connection seen -> detector can differentiate")
        else:
            reasons.append("NO direct connection seen -> detector blind?")
    print("  verdict     : %s  %s" % (verdict, "; ".join(reasons)))
    if chrome and prof:
        shutil.rmtree(prof, ignore_errors=True)
    return verdict, reasons


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--mode", choices=["live", "control", "all"], default="all")
    ap.add_argument("--target", default="https://example.com/")
    ap.add_argument("--sample", type=float, default=15.0)
    args = ap.parse_args()

    if not os.path.exists(NUKERA) or not shutil.which("lsof"):
        print("need ./nukera and lsof", file=sys.stderr)
        return 2
    if not os.path.exists(CHROME):
        print("Google Chrome not found at %s" % CHROME, file=sys.stderr)
        return 2

    os.makedirs(OUT_DIR, exist_ok=True)
    artifacts = {}
    phases = [("LIVE", True), ("control", False)] if args.mode == "all" \
        else [("LIVE", True)] if args.mode == "live" \
        else [("control", False)]

    overall = GOOD
    for name, need_running in phases:
        st = status()
        verdict, reasons = phase(name, need_running, True, 9222, st, args.target,
                                 args.sample, artifacts)
        if verdict != GOOD:
            overall = BAD
        for fn, content in artifacts.items():
            with open(os.path.join(OUT_DIR, fn), "w") as f:
                f.write(content)
        artifacts.clear()

    print("\n=== summary: %s ===" % overall)
    print("artifacts in %s" % OUT_DIR)
    return 0 if overall == GOOD else 1


if __name__ == "__main__":
    sys.exit(main())