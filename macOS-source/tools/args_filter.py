#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Strategy-arg sanitizer for the nukera ciadpi engine (macOS build, v17.3).

Keeps every valid ciadpi short flag in input order, drops everything that the
mac engine would reject or that the CLI manages itself. Verified against the
binary: ./engine/ciadpi -h + live probes (FAKE_SUPPORT and __linux__ short
flags -S -f -n -E -F -T -Y -P -# -/ are rejected on this build).
"""

import shlex

# mac ciadpi v17.3 supported short flags (exact getopt set)
NO_VALUE_FLAGS = set("DNUhvZ")          # D daemon, N no-domain, U no-udp, h help, v version, Z wait-send
VALUE_FLAGS = set("pcIb xgALuyKHjVRsdoqtOlQerM mawWBC".replace(" ", ""))
# side-effect / CLI-managed flags we never forward from a user strategy:
# p port, i listen ip, D daemonize, w pidfile, h help (exits), v version (exits)
CLI_MANAGED = set("p i D w h v".replace(" ", ""))


def filter_strategy_args(raw):
    """Split `raw` (space/newline separated ciadpi args) and return
    (kept_tokens, dropped_tokens). Valid flags are preserved in order;
    duplicates preserved; attached values (e.g. `-d3+s`, `-As`) preserved;
    unknown flags and standalone junk words dropped; a bare value-flag that
    getopt would starve (next token is another flag) is dropped too."""
    tokens = shlex.split(raw or "")
    kept, dropped = [], []
    pending = False  # last kept value-flag still awaits a separate value token
    i = 0
    n = len(tokens)
    while i < n:
        t = tokens[i]
        flaglike = (len(t) > 2 and t.startswith("-")
                    and not t.startswith("--") and not t[1:].isdigit() is False
                    and t not in ("-", "--"))
        if pending and (t.startswith("-") and len(t) > 1):
            f = kept.pop()
            dropped.append("%s (missing value)" % f)
            pending = False
            continue
        if (not t.startswith("-")) or t in ("-", "--") or len(t) == 1:
            if pending:
                kept.append(t)      # separate value for the pending flag
                pending = False
            else:
                dropped.append(t)   # standalone junk word (e.g. "asfajfa")
            i += 1
            continue
        ch = t[1]
        if ch in CLI_MANAGED:
            dropped.append(t)
            pending = False
            i += 1
            continue
        if len(t) == 2:
            if ch in NO_VALUE_FLAGS or ch in VALUE_FLAGS:
                kept.append(t)
                if ch in VALUE_FLAGS:
                    pending = True
            else:
                dropped.append(t)   # unknown flag (e.g. -S on this build)
            i += 1
            continue
        # len(t) > 2
        if ch in VALUE_FLAGS:
            kept.append(t)          # attached value, e.g. -d1, -As, -r1+s
            i += 1
            continue
        if ch in NO_VALUE_FLAGS:
            rest = t[2:]
            ok = all((c in NO_VALUE_FLAGS) or (c in VALUE_FLAGS) for c in rest)
            if ok:
                kept.append(t)      # cluster like -NU; value flag in cluster consumes rest
            else:
                dropped.append(t)
            i += 1
            continue
        dropped.append(t)           # unknown flag char
        i += 1
    return kept, dropped