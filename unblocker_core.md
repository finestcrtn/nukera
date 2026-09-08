# Unblocker Core Architecture

How the Linux DPI bypass works, layer by layer.

---

## The Problem

ISP DPI (Deep Packet Inspection) equipment inspects TLS ClientHello packets to read the SNI (Server Name Indication) field. If the SNI matches a blocked domain (e.g., `instagram.com`, `rutracker.org`), the DPI drops or resets the connection.

The DPI sits between you and the internet. It sees raw TCP packets on the wire. If it sees a clean `ClientHello` with `SNI=instagram.com`, it blocks you.

---

## The Solution

Modify the TLS ClientHello **before** it reaches the DPI. The DPI sees garbled/unrecognizable data instead of the domain name, so it can't block you.

This is called "DPI desync" — desynchronizing the DPI's packet inspection.

---

## Architecture Overview

```
Browser/Chrome
    ↓ TCP connect to instagram.com:443
    ↓
nftables rule (kernel)
    → redirect tcp/80,443 to nfqws on NFQUEUE 200
    ↓
nfqws (zapret engine)
    → reads raw packets from NFQUEUE
    → applies desync strategy (disorder/split/fake/tlsrec)
    → modifies ClientHello (garbles SNI)
    → sends modified packet onward
    ↓
ISP DPI
    → sees garbled SNI → can't identify domain → can't block
    ↓
instagram.com server
    → receives valid TLS ClientHello (desync only affects DPI path)
    → normal HTTPS connection established
```

---

## Layer 1: nftables — Traffic Capture

nftables is the Linux kernel's packet filtering framework. We create a rule that intercepts the user's outbound HTTPS traffic and sends it to our desync engine.

**What the rule does:**
```
table inet unblocker {
    chain output {
        type filter hook output priority mangle;
    }
    rule: meta skuid <uid> tcp dport {80,443} queue num 200 bypass
    rule: meta skuid <uid> udp dport 443 queue num 200 bypass
}
```

- `meta skuid <uid>` — only intercept traffic from the desktop user (not root, not other users)
- `tcp dport {80,443}` — HTTP and HTTPS ports
- `queue num 200` — send packets to NFQUEUE number 200
- `bypass` — if nfqws is dead, let traffic through (no black hole)

**Why nftables and not iptables?** nftables is the modern replacement. Same functionality, better performance, atomic rule changes.

**Root requirement:** nftables needs root. The app uses `pkexec` (Polkit) to get root for nft commands. On first run, Polkit prompts for password. After that, it's passwordless if a Polkit rule is installed.

---

## Layer 2: nfqws — Packet Desync Engine

nfqws is the zapret project's packet processing engine. It reads raw IP packets from NFQUEUE, applies desync strategies, and sends modified packets onward.

**How it works:**
1. nfqws binds to NFQUEUE 200 (kernel queue)
2. Kernel delivers copies of matching packets to nfqws
3. nfqws inspects the packet:
   - If it's a TCP SYN with TLS ClientHello → apply desync strategy
   - Otherwise → pass through unchanged
4. Desync strategy modifies the packet (see below)
5. nfqws sends the modified packet back to the kernel
6. Kernel delivers the modified packet to the destination

**nfqws runs in transparent mode** (`--transparent`). It accepts raw TCP packets from iptables REDIRECT, not SOCKS5 connections. The kernel does the routing; nfqws just modifies packets.

**nfqws is a separate process** (not in-process). It runs as a child process, spawned by the Rust app. If it crashes, the watchdog detects it and removes the nft rule to prevent black-holing traffic.

---

## Layer 3: Desync Strategies — How SNI Gets Garbled

nfqws applies one of several strategies to TLS ClientHello packets. Each strategy modifies the packet differently:

### Disorder (`--disorder 1`)
Sends the first TCP segment out of order. The DPI inspects packets in order, so it sees the SNI in the wrong position and can't parse it. The server's TCP stack handles reassembly normally.

### TLS Record Split (`--tlsrec 1+s`)
Splits the TLS ClientHello across multiple TLS records. The DPI sees incomplete records and can't extract the SNI.

### Fake Packets (`--fake -1 --ttl 8`)
Injects fake TCP segments before the real ClientHello. The DPI processes the fake first (which has no SNI), then the real one arrives but the DPI has already decided to let it through.

### Split (`--split 1+s`)
Splits the TCP payload at a specific offset. The DPI sees two incomplete segments.

### OOB (`--oob 3+s`)
Sends data out-of-band via IP options or other mechanisms.

### Combined strategies
Multiple techniques can be combined: `--disorder 1 --tlsrec 1+s` applies both disorder AND TLS record splitting.

**Auto-detection:** The `tune` subcommand tries all 9 candidate strategies against known blocked sites (youtube, discord, bbc, x.com) and picks the one that works.

---

## Layer 4: /etc/hosts — IP-Blocked Domains

Some sites are blocked not by SNI but by IP address (e.g., Meta/Instagram/Facebook CDN IPs are blocked at the IP level).

For these sites, DNS resolution returns a blocked IP. The fix: pin the domain to a working CDN IP in `/etc/hosts`.

**How it works:**
1. App writes entries like `157.240.0.174 instagram.com` to `/etc/hosts`
2. When Chrome resolves `instagram.com`, the OS checks `/etc/hosts` first
3. Chrome connects to the pinned IP (a working CDN edge)
4. nfqws desysts the SNI → connection works

**IP discovery:** The `install-discover` subcommand probes DoH servers (DNS over HTTPS) and TCP+TLS connections to find working IPs for each blocked domain. It writes these to `/etc/hosts` between marker comments.

**Stale entries:** If an IP stops working, the `refresh` subcommand re-discovers a working IP for that specific domain.

---

## Layer 5: State Management

The app writes state to `/tmp/unblocker.state`:
```json
{"port": 1080, "profile": "--disorder 1 --conn-ip 0.0.0.0", "pid": 12345}
```

- `is_enabled()` checks if this file exists
- On disable, the file is removed
- A watchdog thread monitors nfqws liveness every 5s. If nfqws dies, it removes the nft rule to prevent black-holing traffic

---

## Enable Flow (Step by Step)

When the user taps "Enable":

1. **Clean stale state** — flush any leftover nft rules from previous runs
2. **Find nfqws binary** — search PATH, vendor dirs, system install locations
3. **Load strategy** — read `/etc/unblocker/strategy.conf` (or default)
4. **Spawn nfqws** — start the desync engine as a child process
5. **Install nft rule** — `table inet unblocker`, queue tcp/80,443 to NFQUEUE 200
6. **Apply hosts** — write IP overrides to `/etc/hosts` between markers
7. **Write state** — create `/tmp/unblocker.state`
8. **Start watchdog** — monitor nfqws liveness, auto-cleanup if dead

## Disable Flow (Step by Step)

Order matters to prevent black-holing:

1. **Remove iptables REDIRECT FIRST** — so traffic can flow direct even if nfqws is dead
2. **Stop system proxy** — unset gsettings SOCKS proxy
3. **Remove hosts** — strip marker block from `/etc/hosts`
4. **Remove state** — delete `/tmp/unblocker.state`
5. **Kill nfqws** — `pkill -9 -x nfqws`
6. **Flush nft** — `nft delete table inet unblocker`

---

## Key Safety Mechanisms

1. **Bypass flag** — nft rules use `bypass` so if nfqws dies, traffic passes through (no black hole)
2. **Watchdog** — monitors nfqws every 5s, removes REDIRECT if dead
3. **Disable-first order** — always remove iptables/nft before killing processes
4. **Idempotent nft** — `nft_install` deletes the table first, then recreates. Safe to call multiple times.
5. **Marker-based hosts** — `/etc/hosts` modifications are between `# BEGIN unblocker` / `# END unblocker` markers. Safe to apply/remove without touching other entries.

---

## File Locations

| File | Purpose |
|------|---------|
| `/etc/unblocker/strategy.conf` | Active desync strategy |
| `/etc/unblocker/hostlists/reestr.txt` | Blocked domain list |
| `/etc/unblocker/sites/<host>.conf` | Per-site strategy config |
| `/tmp/unblocker.state` | Runtime state (port, profile, pid) |
| `/etc/hosts` | IP overrides (between markers) |
| `/tmp/unblocker-runtime-nfqws-*.log` | nfqws stdout/stderr |

---

## Strategy Auto-Tuning

The `tune` subcommand:

1. Takes 9 candidate strategies from `candidate_pool()`
2. For each strategy, spawns an ephemeral byedpi instance on a unique port
3. Tests against youtube, discord, bbc, x.com via SOCKS5
4. Counts how many sites pass
5. Picks the strategy with the most passes
6. Saves to `/etc/unblocker/strategy.conf`

---

## What Makes This Work

1. **Kernel-level interception** — nftables captures packets before they leave the machine. No app-level proxy needed.
2. **Packet modification, not replay** — nfqws modifies the actual packet in flight, not creating a new connection. The server sees valid TLS.
3. **Transparent to apps** — Chrome doesn't know about nfqws. It connects normally; the kernel redirects transparently.
4. **Bypass flag** — if nfqws dies, traffic passes through. No black hole.
5. **IP pinning for IP-blocked sites** — `/etc/hosts` overrides DNS for domains blocked at the IP level.
