# How Meta Sites Get Unblocked

Instagram, WhatsApp, Facebook, Messenger — all blocked by Russian DPI. Here's exactly how the Linux app defeats the block.

---

## What Kind of Block Is It?

Meta sites are blocked at **two levels**:

1. **SNI block** — DPI reads `"www.instagram.com"` in the TLS ClientHello and drops the connection. This is the primary block.
2. **IP block** — Some Meta CDN IP ranges are blocked at the IP level. DNS returns a blocked IP, connection fails before TLS even starts.

The Linux app handles both.

---

## Level 1: SNI Desync (Primary Block)

### How it works

When Chrome connects to `instagram.com:443`:

```
Chrome → TCP SYN to instagram.com IP
  ↓
nftables captures tcp/443 → NFQUEUE 200
  ↓
nfqws reads the packet
  ↓ finds TLS ClientHello with SNI="www.instagram.com"
  ↓ applies desync strategy
  ↓ modifies the packet
  ↓
ISP DPI → sees garbled SNI → can't match "instagram.com" → lets it through
  ↓
instagram.com server → receives valid TLS → connection works
```

### The desync strategy

For Meta sites, the strategy is `hostfakesplit` or `disorder`:

- **`--disorder 1`** — sends the first TCP segment out of order. DPI inspects in order, sees SNI in wrong position, can't parse it. Server handles reassembly normally.
- **`--tlsrec 1+s`** — splits TLS ClientHello across multiple TLS records. DPI sees incomplete records, can't extract SNI.
- **`--fake -1 --ttl 8`** — injects fake TCP segments before real ClientHello. DPI processes fake first, real one arrives but DPI has already decided to let it through.

### Why this works for Meta specifically

Meta uses standard TLS 1.3. The SNI field is in the first ~200 bytes of the ClientHello. The desync strategies target exactly this region — splitting, disordering, or faking around the SNI so the DPI can't match the literal string `"www.instagram.com"`.

The server doesn't care about the desync — it receives a valid TLS ClientHello after TCP reassembly.

---

## Level 2: IP Pinning (Fallback)

Some Meta CDN IPs are blocked at the IP level. DNS returns a blocked IP, Chrome can't even start TLS.

### How IP discovery works

The `install-discover` subcommand probes for working IPs:

1. **Precompiled IPs** — hardcoded in `pool.rs`:
   ```
   instagram.com → 157.240.9.174, 157.240.0.35, 157.240.5.35, 157.240.3.35
   facebook.com → 157.240.0.35, 157.240.9.174, 157.240.3.35
   web.whatsapp.com → 157.240.0.174, 157.240.9.174, 157.240.0.35
   ```

2. **DoH probing** — query Google DoH (`dns.google`) and Cloudflare DoH (`cloudflare-dns.com`) for fresh A records:
   ```
   GET https://dns.google/dns-query?name=instagram.com&type=A
   → returns A records from Google's perspective
   ```

3. **GeoHide community IPs** — fetch community-vetted hosts from `Internet-Helper/GeoHideDNS`:
   ```
   GET cdn.jsdelivr.net/gh/Internet-Helper/GeoHideDNS@main/hosts/hosts
   → 5000+ entries, filter to Meta parent domains (cdninstagram.com, fbcdn.net)
   ```

4. **TCP+TLS probe** — for each candidate IP:
   ```
   TCP connect to IP:443 → succeeds?
     ↓ yes
   Send TLS 1.3 ClientHello with SNI=instagram.com → server responds?
     ↓ yes
   IP is alive and serves TLS for instagram.com → pin it
   ```

5. **Write to /etc/hosts**:
   ```
   # BEGIN unblocker
   157.240.9.174 instagram.com
   157.240.0.35 cdninstagram.com
   157.240.0.174 web.whatsapp.com
   # END unblocker
   ```

### Why /etc/hosts is dangerous if wrong

If the pinned IP is dead or blocked, Chrome connects to a dead IP, TLS handshake fails, site appears broken. The app **always strips old entries first**, then writes only verified-working IPs.

### Self-heal

If 3 consecutive probes fail for a host with a `/etc/hosts` entry, the app re-runs discovery and swaps the entry.

---

## The Combined Flow

For `instagram.com` on a Russian ISP:

```
1. Chrome resolves instagram.com
   → /etc/hosts has pinned IP (e.g., 157.240.9.174)
   → Chrome connects to that IP

2. nftables captures tcp/443 → NFQUEUE 200

3. nfqws receives the packet
   → TLS ClientHello has SNI="www.instagram.com"
   → applies --disorder 1 (or hostfakesplit)
   → modifies the packet (garbles SNI region)
   → sends modified packet onward

4. ISP DPI
   → sees garbled data where SNI should be
   → can't match "instagram.com"
   → lets packet through

5. instagram.com server
   → receives valid TLS ClientHello (after TCP reassembly)
   → TLS handshake succeeds
   → HTTPS connection established
   → Instagram loads
```

---

## Config: What Domains Are Handled

From `config/mod.rs` defaults:

```rust
bypass_domains: vec![
    "youtube.com", "googlevideo.com", "ytimg.com",
    "discord.com", "discord.gg",
    "instagram.com", "cdninstagram.com", "fbcdn.net",
    "facebook.com", "fb.com", "fbsbx.com",
    "scontent.cdninstagram.com",
    "x.com", "twitter.com", "t.co", "twimg.com",
    "bbc.com", "bbc.co.uk",
    "t.me", "telegram.org",
    "whatsapp.com",
    "grok.com", "x.ai", "imo.im",
]
```

These are the domains that get DPI desync applied.

From `config/mod.rs` exclude list:
```rust
exclude_domains: vec![
    "yandex.ru", "ya.ru", "mail.ru",
    "gosuslugi.ru", "sberbank.ru",
]
```

Russian domestic sites — no bypass needed, traffic goes direct.

---


