# Site Unblocking Approach — How to Unblock Any DPI-Blocked Site

## Overview

This document describes the proven approach we used to unblock WhatsApp (and Instagram, Facebook, etc.) on a Russian ISP that uses DPI with full TLS reassembly. The approach works in 4 phases and has been validated on the user's network.

## How It Works

### The Problem
The ISP's DPI (Deep Packet Inspection) blocks sites by:
1. **SNI fingerprinting**: inspects the TLS ClientHello's Server Name Indication field
2. **Full TLS reassembly**: reconstructs the complete ClientHello from TCP segments
3. **IP blocking**: blocks specific CDN IP ranges (e.g., 157.240.x.x for Meta)

nfqws's `hostfakesplit` DPI desync CAN bypass the DPI — but only if the IP is in a range the DPI doesn't fingerprint. Changing the IP alone is not enough; you need both the right IP AND the DPI desync.

### The Solution (3 Components)

**Component 1: nfqws DPI Desync** (already in the runtime)
- `hostfakesplit+ts` with `autottl=2` splits the TLS ClientHello and desyncs the DPI
- Works for: YouTube, BBC, Discord, VK, Meduza, Rutracker
- Does NOT work alone for: Instagram images, WhatsApp, Facebook

**Component 2: /etc/hosts CDN IP overrides** (the key discovery)
- The ISP blocks specific Meta CDN IP ranges (157.240.x.x)
- But some CDN edges in 57.144.x.x ranges are NOT fingerprinted by the DPI
- By mapping domains to these unblocked IPs via /etc/hosts, the DPI never sees the blocked IP
- Combined with nfqws desync, the connection succeeds

**Component 3: nfqws hostlist update**
- Domains like `static.whatsapp.net` need to be in the nfqws hostlist (`social-meta.sites`)
- This makes nfqws apply DPI desync to those specific domains

## The Discovery Process (How We Found Working IPs)

### Step 1: Collect Resources via VPN

With VPN active (e.g., a SOCKS proxy), access the blocked site and collect:
- **All domain names** referenced in the HTML
- **All resource URLs** (CSS, JS, images, fonts)
- **The DNS resolution** for each domain

```bash
# Fetch the page
curl -sSL -o page.html -x http://127.0.0.1:12334 https://example.com/
# Extract all domains
grep -oP 'https?://[^"'"'"'<>\s]+' page.html | sed 's|https\?://||' | sed 's|/.*||' | sort -u
# Resolve each domain via DoH (gives you the actual CDN IPs)
curl -sS 'https://dns.google/resolve?name=faq.whatsapp.com&type=A' -x http://127.0.0.1:12334
```

### Step 2: Find Working IPs (VPN + DoH)

Use the VPN's DoH (DNS-over-HTTPS) to resolve each domain. The VPN server resolves from outside Russia, so DoH gives you the actual CDN IPs:

```bash
# Via VPN proxy
curl -sS 'https://dns.google/resolve?name=DOMAIN&type=A' -x http://127.0.0.1:12334
```

### Step 3: Test IPs Locally (No VPN)

Test each resolved IP with curl through the nfqws bypass:

```bash
# For each candidate IP:
CODE=$(curl -sSL -o /dev/null -w "%{http_code}" --max-time 4 \
  --resolve DOMAIN:443:$CANDIDATE_IP \
  --resolve DOMAIN:80:$CANDIDATE_IP \
  https://DOMAIN/)

# A successful test is: 200, 301, 302 (not 000/timeout)
```

### Step 4: Add to Config

Once working IPs are found:
1. Add to `/etc/hosts` (via config/hosts/magila-cdn.txt)
2. Add domain to nfqws hostlist (`social-meta.sites`) if DPI desync is needed
3. Test the full page (not just the main URL)

## Critical Lessons Learned

### Lesson 1: VPN is a research tool, not a permanent solution
The VPN (SOCKS proxy) allowed us to:
- Access the blocked page and extract all URLs/domains
- Use DoH through the VPN to find CDN IPs
- Map out exactly which resources the page needs

The VPN is NOT the fix — it's how we found the IPs for the fix.

### Lesson 2: Different domains on the same CDN need different IPs
`faq.whatsapp.com` → 57.144.245.32 (works)
`web.whatsapp.com` → 57.144.244.1 (works)
`157.240.0.60` → blocked for ALL WhatsApp domains

The CDN edge IPs are per-domain. You must test each domain separately.

### Lesson 3: 404 is NOT "working"
A 404 from the server means the CDN responded (connection alive), but the content is wrong. A 000/timeout means the DPI blocked it. Only HTTP 200 (or 301/302 redirect) with real content means the page works.

### Lesson 4: nfqws DPI desync IS the key
Without nfqws: ALL IPs for WhatsApp time out (DPI blocks)
With nfqws hostfakesplit+ts: some IPs work (57.144.244.1)
The DPI desync is essential — IP override alone doesn't work for all domains.

### Lesson 5: The GeoHide DNS community list helps
The GeoHide DNS hosts file (https://github.com/Internet-Helper/GeoHideDNS) contains community-vetted IPs for many blocked services. It's a starting point, but you still need to verify locally.

## Step-by-Step: Unblock a New Site

When you encounter a new blocked site:

1. **Turn on the VPN** (any SOCKS proxy)
2. **Run the resource collector**:
   ```bash
   curl -sSL -o /tmp/newsite.html -x http://127.0.0.1:12334 https://NEWSITE.com/
   grep -oP 'https?://[^"'"'"'<>\s]+' /tmp/newsite.html | sed 's|https\?://||' | sed 's|/.*||' | sort -u
   ```
3. **Resolve each domain via VPN DoH**:
   ```bash
   curl -sS "https://dns.google/resolve?name=DOMAIN&type=A" -x http://127.0.0.1:12334
   ```
4. **Test each resolved IP locally** (no VPN):
   ```bash
   for ip in $IPS; do
     CODE=$(curl -sSL -o /dev/null -w "%{http_code}" --max-time 4 --resolve DOMAIN:443:$ip https://DOMAIN/)
     echo "$ip: $CODE"
   done
   ```
5. **Add working IPs to config/hosts/magila-cdn.txt**
6. **Add domains to nfqws hostlist** if DPI desync is needed
7. **Test the full page** (not just the main URL — test CSS, JS, images too)
8. **Commit and restart the runtime**

## Files Modified

- `config/hosts/magila-cdn.txt` — CDN edge IPs (MagilaWEB + WhatsApp + Telegram)
- `crates/unblocker-core/src/ip_discovery/hosts.rs` — hosts file management
- `crates/unblocker-core/src/ip_discovery/mod.rs` — discovery functions
- `crates/unblocker-core/src/ip_discovery/sites.rs` — site list + asset_probe_url
- `crates/unblocker-core/src/ip_discovery/geohide.rs` — GeoHide DNS fetcher
- `crates/unblocker-core/src/zapret/probe.rs` — DPI probe + nft_flush
- `crates/unblocker-core/src/zapret/mod.rs` — nfqws spawn + nft management
- `crates/unblocker-core/src/system.rs` — hosts_apply with stale cleanup
- `crates/unblocker/src/main.rs` — install-discover + refresh + enable/disable
- `unblocker_gui/lib/main.dart` — GUI (Flutter, one-button toggle)
- `scripts/verify.sh` — 10-check verification (6 HTML + 4 asset body checks)

## Verified Results

| Site | HTTP | Evidence |
|------|------|----------|
| YouTube | 200 | DPI bypass via nfqws |
| Instagram (HTML) | 200 | DPI bypass |
| Instagram (image) | 200 | Real JPEG (ffd8ff, 4653B) |
| Instagram (WebP) | 200 | Real WebP (RIFF header, 900B) |
| Facebook | 200 | DPI bypass + /etc/hosts |
| WhatsApp FAQ | 200 | 57.144.244.1 CDN edge |
| WhatsApp web | 200 | 57.144.244.1 CDN edge |
| WhatsApp main | 200 | 57.144.244.1 CDN edge |
| WhatsApp icons | 200 | static.whatsapp.net via 57.144.245.32 |
| Discord | 200 | DPI bypass |
| BBC | 200 | DPI bypass |
| VK | 200 | DPI bypass |
| Meduza | 200 | DPI bypass |
| Rutracker | 200 | DPI bypass |

## What Didn't Work

- **Changing IPs alone**: doesn't help when the DPI reassembles the full TLS ClientHello
- **nfqws DPI bypass alone for WhatsApp**: without the right IP, all IPs time out
- **All nfqws strategy combinations for WhatsApp**: hostfakesplit, fakedsplit, fakeddisorder, multisplit, etc. × fooling modes × fake-tls-mod — ALL fail without the right IP
- **Cloudflare WARP**: registration endpoints are DPI-blocked on this network
- **IP scanning across Meta ranges**: 157.240.x.x, 31.13.x.x, 57.144.x.x — most blocked
