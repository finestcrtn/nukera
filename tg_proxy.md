# Telegram MTProto↔WebSocket Proxy

How the Telegram bypass works, layer by layer, with exact file paths for extraction.

---

## The Problem

Telegram is blocked at the ISP level. The DPI inspects TLS ClientHello packets looking for the SNI `web.telegram.org` or detects MTProto traffic patterns. Both TCP and UDP connections to Telegram data centers (DCs) are dropped or reset.

Telegram apps connect directly to DC IPs (149.154.x.x, 91.108.x.x) on port 443. The DPI sees this traffic and blocks it.

---

## The Solution

Route Telegram traffic through a WebSocket tunnel to Cloudflare. Cloudflare's IPs are never blocked (they host half the internet). The proxy:

1. Listens locally for MTProto connections
2. Wraps MTProto data inside WebSocket frames
3. Sends WebSocket frames to Cloudflare → Telegram DCs
4. DPI sees WebSocket traffic to Cloudflare, not MTProto to Telegram DCs

---

## Architecture

```
Telegram Desktop / Mobile App
    ↓ MTProto over TCP (encrypted)
    ↓ connects to 127.0.0.1:1444
    ↓
Local MTProto↔WebSocket Proxy (Rust, in-process)
    ↓ decrypts MTProto init
    ↓ determines target DC
    ↓ re-encrypts MTProto data
    ↓ wraps in WebSocket frames
    ↓ sends to kws{dc}.web.telegram.org via TLS to Cloudflare
    ↓
Cloudflare CDN
    ↓ forwards WebSocket to Telegram DC
    ↓
Telegram DC (149.154.x.x:443)
```

---

## File Map
main folder, all paths in it: `/home/finestcrtn/Documents/new zapret/`
| File | Purpose |
|------|---------|
| `crates/unblocker-core/src/telegram/mod.rs` | Entry point: `start()` creates listener, pool, runs proxy |
| `crates/unblocker-core/src/telegram/proxy.rs` | Core logic: client handler, connection pool, bridge, fallbacks |
| `crates/unblocker-core/src/telegram/ws.rs` | WebSocket transport: TLS, framing, send/recv |
| `crates/unblocker-core/src/telegram/crypto.rs` | AES-256-CTR encryption for MTProto data |
| `crates/unblocker-core/src/telegram/cfproxy.rs` | Cloudflare fallback: domain discovery, DoH resolution, 429 handling |
| `crates/unblocker-core/src/telegram/config.rs` | Config constants: DC IPs, pool size, timeouts |
| `crates/unblocker-core/src/telegram/balancer.rs` | Domain load balancing across CF endpoints |
| `crates/unblocker-core/src/api.rs` (lines 175-309) | `start_tg()`: generates secret, writes tg:// URL, starts proxy |

---

## How Telegram Connects (Step by Step)

### 1. App generates `tg://` URL

When the user taps "Setup Telegram", the app:

```rust
// api.rs — start_tg()
secret = env("TG_PROXY_SECRET") || read_file("tg-proxy.secret") || generate_16_random_bytes → 32hex
tg_url = "tg://proxy?server=127.0.0.1&port=1444&secret=dd{secret}"
write("/etc/unblocker/tg-proxy.url", tg_url)
```

**URL components:**
- `server=127.0.0.1` — local proxy
- `port=1444` — proxy port
- `secret=dd{32hex}` — encryption key (`dd` = padded intermediate protocol)
- The `dd` prefix tells Telegram to use Intermediate encryption (not Abridged)

### 2. App opens Telegram with the URL

**Android:**
```kotlin
// MainActivity.kt
val intent = Intent(Intent.ACTION_VIEW, Uri.parse(tgUrl))
startActivity(intent)
```


Telegram app recognizes `tg://proxy?...` and configures itself to connect through the proxy.

### 3. Telegram connects to local proxy

Telegram opens TCP to `127.0.0.1:1444` and sends a 64-byte handshake:

```
[8 bytes random] [32 bytes prekey] [16 bytes IV] [4 bytes protocol tag] [2 bytes DC ID] [2 bytes padding]
```

### 4. Proxy decrypts the handshake

```rust
// proxy.rs — handle_client()
// Derive AES key from prekey + secret
key = SHA256(prekey || secret_bytes)
// Decrypt with AES-256-CTR
decryptor = new_aes_ctr(key, iv)
decryptor.xor(&mut handshake)

// Extract protocol tag and DC ID
proto_tag = handshake[56..60]  // 0xEFEFEFEF (Intermediate) or 0xEEEEEEEE (Abridged)
dc_raw = handshake[60..62]     // DC number (negative = media)
dc = abs(dc_raw)
is_media = dc_raw < 0
```

### 5. Proxy connects to Telegram DC

Connection priority:
1. **Pool hit** — reuse pre-established WebSocket from `WsPool`
2. **Direct WebSocket** — connect to `kws{dc}.web.telegram.org` via TLS to Cloudflare
3. **Cloudflare fallback** — try encoded CF domains (`cfproxy.rs`)
4. **TCP fallback** — raw TCP to hardcoded DC IPs (149.154.x.x:443)

### 6. Bridge with encryption

The proxy maintains two encryption states:
- **Client→Proxy**: AES-256-CTR with `SHA256(client_prekey || secret)`
- **Proxy→Telegram**: AES-256-CTR with random relay key

```
Client sends MTProto → decrypt with client key → encrypt with relay key → send to Telegram
Telegram sends MTProto → decrypt with relay key → encrypt with client key → send to client
```

This double-encryption ensures the proxy can't read the actual Telegram data (end-to-end encrypted).

### 7. WebSocket transport

```rust
// ws.rs — ws_connect()
// TLS connection to Cloudflare
tls_config = ClientConfig::builder()
    .dangerous()  // NoVerify — skip cert validation
    .with_no_client_auth()

// WebSocket handshake
GET /apiws HTTP/1.1
Host: kws{dc}.web.telegram.org
Sec-WebSocket-Protocol: binary
Upgrade: websocket
Connection: Upgrade

// Binary frames only, client-side masking
// Keepalive: ping every 30s, read timeout 120s
```

---

## Connection Pool (`WsPool`)

Pre-establishes WebSocket connections to Telegram DCs for instant reuse.

```
Per DC slot (DC × media flag):
  ├── Pool size: 4 connections
  ├── Max age: 120 seconds
  ├── Background refill on miss
  └── Stats: pool_hits / pool_misses
```

**Warmup:** On startup, the pool pre-connects to all configured DCs. When a client connects, it gets a pooled connection instantly.

---

## Cloudflare Fallback

When direct WebSocket to Telegram DCs fails (DPI blocking):

1. **Domain discovery** — fetches encoded domains from `Flowseal/tg-ws-proxy`
2. **DoH resolution** — resolves `kws{dc}.{cf_domain}` via Cloudflare/Google/Quad9 DNS
3. **TLS connect** — connects to Cloudflare IP with `kws{dc}.{cf_domain}` as SNI
4. **WebSocket handshake** — `/apiws` endpoint

**429 handling:** If Cloudflare rate-limits (HTTP 429), the domain goes into cooldown (45s-300s exponential backoff).

---

## DC Resolution

Telegram uses numbered data centers. The proxy maps DC IDs to connection targets:

| DC | Default IP | WebSocket Domain |
|----|------------|-----------------|
| 2 | 149.154.167.220 | kws2.web.telegram.org |
| 4 | 149.154.167.220 | kws4.web.telegram.org |
| 1 | 149.154.175.50 | kws1.web.telegram.org |
| 3 | 149.154.175.50 | kws3.web.telegram.org |
| 5 | 91.108.56.130 | kws5.web.telegram.org |

Negative DC IDs indicate media connections (photos, videos, files).

---

## Ports & Protocols

| Component | Port | Protocol | Direction |
|-----------|------|----------|-----------|
| Local proxy | **1444** | MTProto obfuscated | Telegram → Proxy |
| WebSocket to DC | **443** | TLS + WebSocket (`/apiws`) | Proxy → Cloudflare → DC |
| TCP fallback | **443** | Raw TCP | Proxy → DC directly |

---

## How to Extract for Android

### Required files

```
/home/finestcrtn/Documents/new zapret/crates/unblocker-core/src/telegram/
├── mod.rs          (entry point)
├── proxy.rs        (core logic)
├── ws.rs           (WebSocket transport)
├── crypto.rs       (AES-256-CTR encryption)
├── cfproxy.rs      (Cloudflare fallback)
├── config.rs       (DC IPs, constants)
└── balancer.rs     (domain load balancing)
```

### Dependencies (from Cargo.toml)

```toml
tokio = { version = "1", features = ["full"] }
tokio-util = "0.7"
tokio-rustls = "0.24"
rustls = "0.21"
rustls-pki-types = "0.2"
sha2 = "0.10"
aes = "0.8"
ctr = "0.8"
rand = "0.8"
hex = "0.4"
byteorder = "1"
base64 = "0.21"
once_cell = "1"
parking_lot = "0.12"
```

### Integration steps

1. Copy `telegram/` module into your project
2. Call `telegram::start(host, port, secret, dc_map, cache_dir)` at app startup
3. Generate `tg://` URL with your secret
4. Open Telegram with the URL via `Intent.ACTION_VIEW` (Android) or `xdg-open` (Linux)

### Minimal start call

```rust
let secret = generate_secret(); // 32 hex chars
let dc_map = HashMap::from([
    (2, "149.154.167.220".to_string()),
    (4, "149.154.167.220".to_string()),
]);
let tg_url = format!("tg://proxy?server=127.0.0.1&port=1444&secret=dd{secret}");

// Start proxy
let (cancel, handle) = telegram::start("127.0.0.1", 1444, &secret, dc_map, None).await?;

// Open Telegram
open_tg_app(&tg_url);
```

### Android-specific: open Telegram

```kotlin
// In your Activity
val url = "tg://proxy?server=127.0.0.1&port=1444&secret=dd$secret"
val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url))
startActivity(intent)
```

---

## Key Constants

```rust
DEFAULT_PORT     = 1444    // Local proxy listen port
POOL_SIZE        = 4       // WebSocket connections per DC slot
WS_POOL_REUSE_MAX_AGE = 120  // Seconds before pooled WS is discarded
DC_FAIL_COOLDOWN = 30      // Seconds to wait after DC connection failure
WS_FAIL_TIMEOUT  = 2.0     // Seconds for direct WS connection attempt
BRIDGE_PING_INTERVAL = 30  // Seconds between keepalive pings
BRIDGE_READ_TIMEOUT  = 120 // Seconds before idle connection is closed
WS_WRITE_TIMEOUT     = 5   // Seconds for WS frame write
CFPROXY_429_BASE     = 45  // Seconds base cooldown for 429 rate limit
```

---

## Failure Modes & Recovery

| Failure | Recovery |
|---------|----------|
| Direct WS to DC fails | Try Cloudflare fallback domains |
| Cloudflare 429 rate limit | Domain cooldown (45s-300s exponential) |
| All CF domains fail | TCP fallback to hardcoded DC IPs |
| DC connection drops | Pool refills automatically, client gets new connection |
| Proxy crashes | Telegram app reconnects automatically |

---

## Data Flow Diagram

```
┌─────────────────────┐
│  Telegram Desktop    │
│  (connects to        │
│   127.0.0.1:1444)   │
└──────────┬──────────┘
           │ MTProto encrypted
           ▼
┌─────────────────────┐
│  MTProto↔WS Proxy   │
│  (Rust, in-process) │
│                     │
│  1. Decrypt init    │
│  2. Determine DC    │
│  3. Get/pool WS     │
│  4. Re-encrypt data │
│  5. Wrap in WS      │
└──────────┬──────────┘
           │ WebSocket binary frames
           │ TLS to Cloudflare
           ▼
┌─────────────────────┐
│  Cloudflare CDN     │
│  (kws{dc}.web.      │
│   telegram.org)     │
└──────────┬──────────┘
           │ Forwarded to DC
           ▼
┌─────────────────────┐
│  Telegram DC        │
│  (149.154.x.x:443)  │
└─────────────────────┘
```
