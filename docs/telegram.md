# How Telegram Web was unblocked

## The problem

Telegram Desktop app works via MTProto proxy, but **Telegram Web** (browser) was completely
broken. Opening `https://web.telegram.org/` or `https://desktop.telegram.org/` resulted in
connection reset or timeout. The ISP (Roskomnadzor) blocks Telegram's IP ranges at the
network level — `149.154.160.0/20`, `91.108.0.0/16`, etc.

## Why it was broken — two independent problems

### Problem 1: DNS resolved to blocked IPs

When the browser asked "what IP is web.telegram.org?", DNS returned an IP in Telegram's
blocked CIDR range (e.g. `149.154.167.99`). The ISP's firewall drops all traffic to these IPs.

**Our hosts entries** mapped Telegram domains to `149.154.167.220` (DC2 edge, not blocked).
But the browser wasn't reading `/etc/hosts`.

### Problem 2: systemd-resolved ignores /etc/hosts

The system uses `systemd-resolved` for DNS. The nsswitch.conf order was:

```
hosts: mymachines resolve [!UNAVAIL=return] files myhostname dns
```

The `resolve` module (systemd-resolved) is tried FIRST. The `[!UNAVAIL=return]` action
means: if resolved returns ANY answer (even from upstream DNS), return it immediately
and NEVER consult `/etc/hosts`. So our hosts entries were silently ignored.

## The fix — three changes

### Fix 1: Telegram Web hosts entries

Added `config/hosts/telegram-web.txt` with 34 domains all pointing to `149.154.167.220`:

```
149.154.167.220 web.telegram.org
149.154.167.220 api.telegram.org
149.154.167.220 t.me
149.154.167.220 telegram.org
149.154.167.220 desktop.telegram.org
... (34 total)
```

`149.154.167.220` is Telegram's DC2 edge server. It's NOT on Roskomnadzor's blacklist
because it's used for internal Telegram traffic. Our hosts entries redirect the browser
to this working IP instead of the blocked IPs that DNS would return.

### Fix 2: nsswitch.conf swap

Changed `/etc/nsswitch.conf` from:
```
hosts: mymachines resolve [!UNAVAIL=return] files myhostname dns
```
to:
```
hosts: mymachines files resolve [!UNAVAIL=return] myhostname dns
```

Now `/etc/hosts` is consulted FIRST. If our hosts file has an entry for a domain,
it's used immediately — systemd-resolved is never queried.

The original nsswitch.conf is backed up to `/etc/nsswitch.conf.unblocker-backup` and
restored on disable.

### Fix 3: nfqws DPI desync

Even with the correct IP from hosts, the ISP inspects TLS ClientHello packets and
drops connections where the SNI (Server Name Indication) contains "telegram.org".

nfqws intercepts outgoing packets via NFQUEUE, applies `hostfakesplit` strategy which
splits the SNI field so the DPI parser can't read it. The ISP sees garbled data,
passes the connection through, and Telegram's server on the other end reassembles it.

## How it all works together

```
1. Browser resolves "web.telegram.org"
   → /etc/hosts says 149.154.167.220 (our entry, not blocked IP)

2. Browser connects to 149.154.167.220:443
   → TLS ClientHello contains SNI "web.telegram.org"

3. nfqws intercepts the packet via NFQUEUE
   → applies hostfakesplit: splits SNI so DPI can't read it

4. ISP's DPI sees garbled SNI, can't identify Telegram
   → passes the connection through

5. Telegram's server at 149.154.167.220 receives the connection
   → reassembles the SNI, serves the page

6. Browser receives real Telegram Web content
```

## What didn't work before

- **Just hosts entries without nsswitch fix**: browser used systemd-resolved, ignored /etc/hosts
- **Just nfqws without hosts entries**: nfqws desynced SNI, but browser connected to blocked IP
- **Just nsswitch fix without hosts**: browser read /etc/hosts but it was empty (no entries added)

All three fixes were needed simultaneously.

## Files involved

| File | Role |
|------|------|
| `config/hosts/telegram-web.txt` | 34 Telegram domains → 149.154.167.220 |
| `crates/unblocker-core/src/system.rs` | `fix_nsswitch_for_hosts()` swaps nsswitch order |
| `crates/unblocker-core/src/system.rs` | `hosts_apply()` writes entries to /etc/hosts |
| `crates/unblocker-core/src/zapret/mod.rs` | nfqws NFQUEUE rules + DPI desync strategies |
| `/etc/polkit-1/rules.d/50-unblocker.rules` | passwordless systemctl |
| `/etc/systemd/system/unblocker.service` | runs nfqws as root |

## Verification

After enabling unblocker:
```bash
# Check hosts entries exist
grep "desktop.telegram.org" /etc/hosts
# → 149.154.167.220 desktop.telegram.org

# Check nsswitch order
grep "^hosts:" /etc/nsswitch.conf
# → hosts: mymachines files resolve [!UNAVAIL=return] myhostname dns

# Check nfqws is running
pgrep -a nfqws

# Test actual page load
curl -sL https://desktop.telegram.org/ | grep "<title>"
# → <title>Telegram Desktop</title>
```
