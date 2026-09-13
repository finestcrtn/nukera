#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Strategy evaluation harness — port of the Android ByeByeDPI TestActivity
(auto-test strategy pool against domain lists, pick the one that unblocks
the most) + the original mac project's macos_bruteforce.sh ciadpi pool.

Approach (mirrors ByeByeDPI-master/app/.../activities/TestActivity.kt +
utility/SiteCheckUtils.kt):
  1. Build a strategy pool:
       - the ciadpi pool from the original mac macos_bruteforce.sh (Phases 1-6)
       - the 51-line Android pool  (tools/data/proxytest_strategies.list)
       - our shipped presets        (engine/strategies/*.args)
     Filtered to the flag set the LOCAL ciadpi binary actually parses
     (mac 17.3 lacks -f/-n/-S/-F/-Y that Android's byedpi offers).
  2. For each strategy: start a private ciadpi on a scratch port, probe a
     union of domain lists over SOCKS5 (DNS resolved in proxy, like Android),
     score success = HTTP 2xx AND non-empty body (Android also requires full
     Content-Length; chunked sites mean >0 is the practical equivalent).
  3. Rank strategies by pass rate; per-site coverage matrix printed.
  4. --apply: write the winner <name>.args into engine/strategies/ and set
     state/strategy.txt so `./nukera start` uses it.

Usage:
  python3 tools/strategy_eval.py [--sites merged,cloudflare]   domain lists
                                 [--port 1087]                 scratch port
                                 [--parallel 8]                probe workers
                                 [--timeout 5]                 curl max-time
                                 [--quick]                     small pool+subset
                                 [--apply]                     persist winner
                                 [--list]                      print pool only
                                 [--srcdir tools/data]

The engine is left untouched except for the strategy file when --apply.
Run from the repo root.
"""

import argparse
import os
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CIADPI = os.path.join(ROOT, "engine", "ciadpi")
STRATEGY_DIR = os.path.join(ROOT, "engine", "strategies")
STATE_DIR = os.path.join(ROOT, "state")
STRATEGY_FILE = os.path.join(STATE_DIR, "strategy.txt")
CANCEL_FILE = os.path.join(STATE_DIR, "adapt.cancel")


class UserCancel(Exception):
    pass
SRCDIR = os.path.join(ROOT, "tools", "data")

# any engine spawned by this run, killed on SIGINT/SIGTERM/exit
ACTIVE_ENGINES = []


def _kill_all_engines(base=1087, count=80):
    for p in list(ACTIVE_ENGINES):
        try:
            p.terminate()
        except OSError:
            pass
    deadline = time.time() + 2.0
    for p in list(ACTIVE_ENGINES):
        try:
            p.wait(timeout=deadline - time.time())
        except (OSError, subprocess.TimeoutExpired):
            try:
                p.kill()
            except OSError:
                pass
    ACTIVE_ENGINES.clear()
    _kill_port_sweep(base, count)


def _port_from_cmd(cmd):
    toks = shlex.split(cmd)
    for i, t in enumerate(toks):
        if t == "-p":
            if i + 1 < len(toks):
                try:
                    return int(toks[i + 1])
                except (TypeError, ValueError):
                    pass
        elif t.startswith("-p") and len(t) > 2:
            try:
                return int(t[2:])
            except ValueError:
                pass
    return None


def _kill_port_sweep(base, count):
    """Kill every ciadpi listening on a scratch port (base..base+count).
    Deterministic: no thread-race on the registry. Never touches the main
    engine (port 1080), which is outside the scratch range."""
    scratch = set(range(base, base + count))
    try:
        r = subprocess.run(["pgrep", "-f", "ciadpi"],
                           capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return
    if r.returncode != 0:
        return
    for line in r.stdout.splitlines():
        line = line.strip()
        if not line.isdigit():
            continue
        pid = int(line)
        try:
            p = subprocess.run(["ps", "-p", str(pid), "-o", "command="],
                               capture_output=True, text=True, timeout=5)
        except (OSError, subprocess.TimeoutExpired):
            continue
        cmd = (p.stdout or "").strip()
        port = _port_from_cmd(cmd)
        if port is not None and port in scratch:
            try:
                os.kill(pid, signal.SIGKILL)
            except OSError:
                pass


def _on_signal(signum, frame):
    try:
        _kill_all_engines()
    except Exception:  # noqa: BLE001
        pass
    try:
        sys.stderr.write("\ninterrupted — winner NOT applied (incomplete test)\n")
    except Exception:  # noqa: BLE001
        pass
    # os._exit: bypasses interpreter/threadpool teardown (which stalls on
    # non-daemon executor workers and can hang forever); engines were already
    # swept by _kill_all_engines above.
    os._exit(130 if signum == signal.SIGINT else 143)

# ciadpi pool from the original mac project's macos_bruteforce.sh.
# Phases 1-3 single; 4-5 stacked; 6 UDP-fake (Discord voice).
BRUTEFORCE_POOL = [
    ("-s 1", 1), ("-s 1+s", 1), ("-d 1", 1), ("-d 1+s", 1), ("-d 3", 1), ("-d 3+s", 1),
    ("-o 1", 2), ("-o 1+s", 2), ("-o 3+s", 2), ("-q 1", 2), ("-q 1+s", 2),
    ("-r 1+s", 3), ("-r 3+s", 3),
    ("-d 1 -o 1+s", 4), ("-d 1 -r 1+s", 4), ("-o 1+s -r 1+s", 4),
    ("-s 1 -d 1+s", 4), ("-s 1 -o 1+s", 4), ("-q 1 -d 1+s", 4),
    ("-d 1 -o 1+s -r 1+s", 5), ("-s 1 -d 1+s -o 1+s", 5), ("-q 1 -d 1+s -r 1+s", 5),
    ("-d 1 -a 3", 6), ("-d 1 -a 5", 6), ("-o 1+s -a 5", 6), ("-d 1 -o 1+s -r 1+s -a 5", 6),
]

# Phases 1-6 for the -aN UDP-fake strategies were tested with -a 1 baseline in
# Android; on TCP-only probes -aN is inert, scores inherit the TCP behavior.


@dataclass
class Strategy:
    label: str
    args: str
    pool: str = "?"

    @property
    def argv(self):
        return shlex.split(self.args)

    @property
    def token_flags(self):
        return [t[1] for t in self.argv
                if t.startswith("-") and not t.startswith("--") and len(t) > 1]


@dataclass
class Probe:
    domain: str
    ok: bool
    code: int = -1
    size: int = 0
    err: str = ""


@dataclass
class Result:
    strategy: Strategy
    total: int = 0
    passed: int = 0
    probes: list = field(default_factory=list)
    engine_failed: bool = False


# ---------------------------------------------------------------- helpers
def sh(args, timeout=None):
    try:
        p = subprocess.run(args, capture_output=True, timeout=timeout)
        return (p.returncode,
                (p.stdout or b"").decode("utf-8", "replace"),
                (p.stderr or b"").decode("utf-8", "replace"))
    except subprocess.TimeoutExpired:
        return 124, "", "timeout"
    except Exception:  # noqa: BLE001
        return 127, "", "error"


def supported_flags():
    """Set of single-letter short options the local ciadpi accepts."""
    rc, out, _ = sh([CIADPI, "--help"])
    flags = set()
    if rc == 0:
        for line in out.splitlines():
            line = line.strip()
            if line.startswith("-") and "," in line:
                short = line.split(",")[0].lstrip("-")
                if len(short) == 1 and short.isalpha():
                    flags.add(short)
    return flags


def socks_ready(port, timeout=3.0):
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


def read_sites(path):
    try:
        with open(path) as f:
            return [ln.strip() for ln in f
                    if ln.strip() and not ln.strip().startswith("#")]
    except OSError:
        return []


def probe_domain(domain, port, timeout, json_out_path):
    cmd = ["curl", "-sS", "--max-time", str(timeout), "-L"]
    if shutil.which("curl"):
        cmd += ["-x", "socks5h://127.0.0.1:%d" % port,
                "-o", "/dev/null", "-w", "%{http_code} %{size_download}",
                "https://%s/" % domain.rstrip("/")]
        rc, out, err = sh(cmd, timeout=timeout + 5)
        http_code = -1
        size = 0
        if rc == 0 and out.strip():
            try:
                http_code, size = [int(x) for x in out.strip().split()]
            except ValueError:
                pass
        return http_code, size, rc
    return -1, 0, 127


# ---------------------------------------------------------------- pools
def load_android_pool():
    lines = []
    try:
        with open(os.path.join(SRCDIR, "proxytest_strategies.list")) as f:
            lines = [ln.strip() for ln in f if ln.strip() and not ln.startswith("#")]
    except OSError:
        pass
    return [Strategy("android-%d" % i, ln, "android") for i, ln in enumerate(lines, 1)]


def load_presets():
    pool = []
    if os.path.isdir(STRATEGY_DIR):
        for f in sorted(os.listdir(STRATEGY_DIR)):
            if f.endswith(".args"):
                args = ""
                try:
                    with open(os.path.join(STRATEGY_DIR, f)) as fh:
                        args = fh.read().strip()
                except OSError:
                    continue
                if args:
                    pool.append(Strategy(f[:-5], args, "preset"))
    return pool


def build_pool(quick):
    supported = supported_flags()
    pool = []
    dropped = []

    # Android lines (newer flags) filtered against the local binary.
    for s in load_android_pool():
        bad = [f for f in s.token_flags if f not in supported]
        if bad:
            dropped.append("android-%d: flags %s unsupported (mac ciadpi %s)"
                           % (int(s.label.split("-")[1]), sorted(set(bad)), ""))
            continue
        pool.append(s)

    bf_pool = {}
    for args, phase in BRUTEFORCE_POOL:
        bf_pool.setdefault(phase, []).append(args)
    for phase in sorted(bf_pool):
        for i, args in enumerate(bf_pool[phase], 1):
            s = Strategy("brute-p%d-%02d" % (phase, i), args, "bruteforce")
            bad = [f for f in s.token_flags if f not in supported]
            if bad:
                dropped.append("%s: flags %s unsupported" % (s.label, sorted(set(bad))))
                continue
            pool.append(s)

    for s in load_presets():
        bad = [f for f in s.token_flags if f not in supported]
        if bad:
            dropped.append("%s: flags %s unsupported" % (s.label, sorted(set(bad))))
            continue
        pool.append(s)

    # De-dupe identical argv, keep first.
    seen = {}
    uniq = []
    for s in pool:
        key = tuple(s.argv)
        if key not in seen:
            seen[key] = s
            uniq.append(s)
    pool = uniq

    if quick:
        return pool, dropped
    return pool, dropped


# ---------------------------------------------------------------- engine
def run_strategy(s, port):
    try:
        proc = subprocess.Popen([CIADPI, "-p", str(port)] + s.argv,
                                stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL,
                                start_new_session=True, close_fds=True)
    except OSError:
        return None, True
    ACTIVE_ENGINES.append(proc)
    if not socks_ready(port):
        try:
            proc.terminate()
        except OSError:
            pass
        return proc, True
    return proc, False


def kill_strategy(proc):
    if not proc:
        return
    try:
        if proc in ACTIVE_ENGINES:
            ACTIVE_ENGINES.remove(proc)
    except ValueError:
        pass
    for sig in (15, 9):
        try:
            proc.send_signal(sig)
        except OSError:
            break
        deadline = time.time() + 2.0
        while proc.poll() is None and time.time() < deadline:
            time.sleep(0.05)
        if proc.poll() is not None:
            break


# ---------------------------------------------------------------- domains
def load_domains(sitelist_arg, quick):
    names = ["merged", "cloudflare"]
    if sitelist_arg:
        names = [n.strip() for n in sitelist_arg.split(",") if n.strip()]
    elif quick:
        names = ["general", "cloudflare"]
    # always include control domains to validate the probe harness
    domains = read_sites(os.path.join(SRCDIR, "control.sites"))
    for n in names:
        path = os.path.join(SRCDIR, "proxytest_%s.sites" % n)
        got = read_sites(path)
        if not got:
            sys.stderr.write("warning: no domains from %s\n" % n)
        for d in got:
            if d not in domains:
                domains.append(d)
    return domains


# ---------------------------------------------------------------- evaluate
def evaluate(pool, domains, base_port, parallel, timeout, quick):
    results = []
    done = {"n": 0}
    lock = threading.Lock()
    total_strats = 0

    def run(s, port):
        if os.path.exists(CANCEL_FILE):
            raise UserCancel()
        res = Result(strategy=s, total=len(domains))
        proc, engine_failed = run_strategy(s, port)
        if proc is None:
            res.engine_failed = True
        else:
            res.engine_failed = engine_failed
            time.sleep(0.3)  # settle

            with ThreadPoolExecutor(max_workers=parallel) as ex:
                futs = {ex.submit(probe_domain, d, port, timeout, None): d
                        for d in domains}
                import concurrent.futures as _cf
                for fut in futs:
                    while True:
                        if os.path.exists(CANCEL_FILE):
                            raise UserCancel()
                        try:
                            code, size, rc = fut.result(timeout=0.5)
                            break
                        except _cf.TimeoutError:
                            continue
                    d = futs[fut]
                    ok = rc == 0 and 200 <= code <= 299 and size > 0
                    res.probes.append(Probe(domain=d, ok=ok, code=code,
                                            size=size, err=str(rc)))
            res.passed = sum(1 for p in res.probes if p.ok)
            kill_strategy(proc)

        with lock:
            done["n"] += 1
            k = done["n"]
        if os.path.exists(CANCEL_FILE):
            raise UserCancel()
        flags = " ".join(s.argv)
        state = "OK " if not res.engine_failed else "ENGINE"
        sys.stdout.write("[%*d/%d done] [%s] %s (passed %d/%d)\n"
                         % (len(str(total_strats)), k, total_strats, state,
                            flags, res.passed, res.total))
        sys.stdout.flush()
        return res

    if quick and len(pool) > 12:
        android = [s for s in pool if s.pool == "android"]
        brute = [s for s in pool if s.pool == "bruteforce"]
        preset = [s for s in pool if s.pool == "preset"]
        chosen = (android[:5] + brute[:4] + preset[:3]
                  + android[5:8] + brute[4:6])[:12]
    else:
        chosen = pool
    total_strats = len(chosen)
    print("eval: 0/%d strategies over %d domains, %d workers...\n"
          % (total_strats, len(domains), 4))
    from concurrent.futures import as_completed
    with ThreadPoolExecutor(max_workers=4) as ex:
        futs = [ex.submit(run, s, base_port + i) for i, s in enumerate(chosen)]
        try:
            for f in as_completed(futs):
                results.append(f.result())
        except KeyboardInterrupt:
            for f in futs:
                f.cancel()
            raise

        except UserCancel:
            for f in futs:
                f.cancel()
            raise
    return results


# ---------------------------------------------------------------- output
def print_report(results, domains, dropped):
    results.sort(key=lambda r: (r.passed, r.total - r.engine_failed), reverse=True)
    print("\n=== strategy eval report ===")
    if dropped:
        print("\n-- skipped (flags unsupported by local ciadpi) --")
        for d in dropped:
            print("   " + d)

    print("\n-- ranking (pass rate) --")
    for r in results:
        pct = 100 * r.passed / r.total if r.total else 0
        mark = "  <-- apply" if r is results[0] else ""
        print("  %4d/%-3d  %5.1f%%  %-14s %s%s"
              % (r.passed, r.total, pct, r.strategy.label, " ".join(r.strategy.argv), mark))

    alive = [r for r in results if not r.engine_failed]
    if not alive:
        print("\nERROR: no strategy produced a working engine. Are you in the repo root?")
        return None

    best = alive[0]

    # per-domain: how many alive strategies unblocked it
    cov = {d: sum(1 for r in alive if any(p.domain == d and p.ok for p in r.probes))
           for d in domains}
    print("\n-- coverage --")
    print("  any-strategy success: %d/%d domains" %
          (sum(1 for n in cov.values() if n), len(domains)))
    for d in domains:
        if cov[d] == 0:
            print("  NOT unblocked by any strategy: %s" % d)
    return best


def apply_winner(best):
    if not best:
        return False
    n = 1
    label = None
    while n < 1000:
        cand = "eval-win-%d" % n
        if not os.path.exists(os.path.join(STRATEGY_DIR, cand + ".args")):
            label = cand
            break
        n += 1
    if not label:
        sys.stderr.write("error: could not allocate winner label\n")
        return False
    # atomic: write tmp then rename; a crash mid-apply leaves old files intact
    path = os.path.join(STRATEGY_DIR, label + ".args")
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        f.write(best.strategy.args + "\n")
    os.replace(tmp, path)
    os.makedirs(STATE_DIR, exist_ok=True)
    tmp2 = STRATEGY_FILE + ".tmp"
    with open(tmp2, "w") as f:
        f.write(label + "\n")
    os.replace(tmp2, STRATEGY_FILE)
    print("\napplied winner: %s args=(%s)" % (label, best.strategy.args))
    print("next `./nukera start` will use strategy: %s" % label)
    return True


def main():
    ap = argparse.ArgumentParser(description="Nukera strategy eval (Android-style)")
    ap.add_argument("--sites", default=None, help="comma list: merged,cloudflare,...")
    ap.add_argument("--port", type=int, default=1087, help="scratch ciadpi port")
    ap.add_argument("--parallel", type=int, default=8, help="concurrent probes")
    ap.add_argument("--timeout", type=int, default=6, help="curl max-time per probe")
    ap.add_argument("--quick", action="store_true", help="small pool + small site set")
    ap.add_argument("--apply", action="store_true", help="persist winner to strategy list")
    ap.add_argument("--list", action="store_true", help="print pool, then exit")
    args = ap.parse_args()

    if not os.path.exists(CIADPI):
        sys.stderr.write("error: engine not found at %s (run from repo root)\n" % CIADPI)
        return 1

    signal.signal(signal.SIGINT, _on_signal)
    signal.signal(signal.SIGTERM, _on_signal)

    try:
        return _main(args)
    finally:
        _kill_port_sweep(args.port,
                         max(len(build_pool(args.quick)[0]), 4) + 2)


def _main(args):
    try:
        if os.path.exists(CANCEL_FILE):
            os.remove(CANCEL_FILE)
    except OSError:
        pass
    pool, dropped = build_pool(args.quick)
    if args.list:
        for s in pool:
            print("%-16s %s" % (s.label, s.args))
        if dropped:
            print("-- dropped --")
            for d in dropped:
                print(d)
        return 0

    domains = load_domains(args.sites, args.quick)
    print("pool=%d strategies, %d domains" % (len(pool), len(domains)))
    if dropped:
        print("note: %d strategy lines skipped (unsupported mac flags):" % len(dropped))
        for d in dropped:
            print("   " + d)

    try:
        results = evaluate(pool, domains, args.port, args.parallel, args.timeout, args.quick)
    except UserCancel:
        sys.stderr.write("\ncancelled by user (adapt.cancel) — winner NOT applied\n")
        return 130
    best = print_report(results, domains, dropped)
    if args.apply and best:
        apply_winner(best)
    return 0


if __name__ == "__main__":
    sys.exit(main())