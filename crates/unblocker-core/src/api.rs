//! FFI API for Flutter (Linux).
//!
//! The GUI calls these directly via `dart:ffi` — no systemd, no Python.
//! All functions are synchronous (block on the tokio runtime) and return C
//! strings. The actual bypass work is delegated to the Linux platform
//! driver: nfqws NFQUEUE + nftables + /etc/hosts + in-process TG proxy.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;
use std::sync::Mutex;

use parking_lot::Mutex as ParkMutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;
use tracing::info;

static STATE_MUTEX: Mutex<()> = Mutex::new(());

/// Create a fresh tokio runtime for FFI calls. Each call gets its own
/// runtime to avoid conflicts with the CLI's #[tokio::main] runtime
/// or stale state from previous FFI calls.
fn ffi_runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime for FFI")
}

// ---------------------------------------------------------------------------
// App paths — Linux uses hardcoded defaults.
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct AppPaths {
    pub etc_dir: PathBuf,
    pub state_file: PathBuf,
    pub cache_dir: String,
    pub config_dir: Option<PathBuf>,
}

static APP_PATHS: ParkMutex<Option<AppPaths>> = ParkMutex::new(None);

fn default_cache_dir() -> String {
    std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!("{}/unblocker", s.trim_end_matches('/')))
        .or_else(|| {
            dirs::home_dir()
                .map(|p| p.join(".cache/unblocker").to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "/var/cache/unblocker".to_string())
}

fn default_paths() -> AppPaths {
    AppPaths {
        etc_dir: PathBuf::from("/etc/unblocker"),
        state_file: PathBuf::from("/tmp/unblocker.state"),
        cache_dir: default_cache_dir(),
        config_dir: None,
    }
}

pub(crate) fn paths() -> AppPaths {
    APP_PATHS.lock().clone().unwrap_or_else(default_paths)
}

/// FFI: point the engine at app-owned directories. NULL keeps the defaults.
/// Returns 0 on success.
#[no_mangle]
pub extern "C" fn set_app_paths(
    apps_etc: *const c_char,
    apps_state: *const c_char,
    apps_cache: *const c_char,
    apps_config: *const c_char,
) -> c_int {
    let read = |p: *const c_char| -> Option<String> {
        if p.is_null() {
            return None;
        }
        unsafe { CStr::from_ptr(p) }
            .to_str()
            .ok()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    };

    let mut cur = default_paths();
    if let Some(prev) = APP_PATHS.lock().as_ref() {
        cur = prev.clone();
    }
    if let Some(e) = read(apps_etc) {
        cur.etc_dir = PathBuf::from(&e);
    }
    if let Some(s) = read(apps_state) {
        cur.state_file = PathBuf::from(&s);
    }
    if let Some(c) = read(apps_cache) {
        cur.cache_dir = c;
    }
    if let Some(d) = read(apps_config) {
        cur.config_dir = Some(PathBuf::from(&d));
    }

    let mut locked = APP_PATHS.lock();
    if locked.as_ref() != Some(&cur) {
        info!("app paths set: etc={} state={} cache={}", cur.etc_dir.display(), cur.state_file.display(), cur.cache_dir);
        crate::system::set_state_file(cur.state_file.clone());
        if let Some(d) = &cur.config_dir {
            crate::config::set_config_dir(d.clone());
        }
        *locked = Some(cur);
    }
    0
}

// ---------------------------------------------------------------------------
// C ABI
// ---------------------------------------------------------------------------

fn alloc_cstr(s: &str) -> *mut c_char {
    CString::new(s).unwrap().into_raw()
}

/// Free a string returned by Rust (call from Dart via `free_string`).
#[no_mangle]
pub extern "C" fn free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

/// Returns 1 if enabled, 0 otherwise. Checks both the state file and
/// the systemd fallback.
#[no_mangle]
pub extern "C" fn is_enabled() -> c_int {
    if crate::system::is_enabled() {
        return 1;
    }
    if let Ok(out) = std::process::Command::new("systemctl")
        .args(["is-active", "unblocker.service"])
        .output()
    {
        if String::from_utf8_lossy(&out.stdout).trim() == "active" {
            return 1;
        }
    }
    0
}

/// Returns tg:// URL (allocated, must free) or empty string if not exists.
#[no_mangle]
pub extern "C" fn get_tg_url() -> *mut c_char {
    let path = paths().etc_dir.join("tg-proxy.url");
    let s = std::fs::read_to_string(path).unwrap_or_default();
    alloc_cstr(s.trim())
}

/// Enable bypass (DPI + hosts + TG). Returns empty string on success, error
/// message otherwise. Blocks until setup done.
#[no_mangle]
pub extern "C" fn enable() -> *mut c_char {
    let _guard = STATE_MUTEX.lock().unwrap();
    // Stale state detection: if state file exists but nfqws is not running,
    // the previous session died or was killed. Clean up the stale state so
    // we don't skip enabling.
    if crate::system::is_enabled() {
        let nfqws_alive = std::process::Command::new("pgrep")
            .args(["-x", "nfqws"])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);
        if !nfqws_alive {
            info!("stale state detected (nfqws dead) — cleaning up");
            crate::system::clear_state();
        } else {
            return alloc_cstr("");
        }
    }
    let cfg = match crate::config::load_or_create() {
        Ok(c) => c,
        Err(e) => return alloc_cstr(&format!("config: {e}")),
    };
    let mut drv = crate::platform::driver().lock();
    let rt = ffi_runtime();
    match rt.block_on(drv.enable_inner(&cfg)) {
        Ok(()) => alloc_cstr(""),
        Err(e) => alloc_cstr(&e.to_string()),
    }
}

/// Disable bypass. Returns empty string on success.
#[no_mangle]
pub extern "C" fn disable() -> *mut c_char {
    let _guard = STATE_MUTEX.lock().unwrap();
    let mut drv = crate::platform::driver().lock();
    let rt = ffi_runtime();
    match rt.block_on(drv.disable_inner()) {
        Ok(()) => alloc_cstr(""),
        Err(e) => alloc_cstr(&e.to_string()),
    }
}

/// Dart-friendly wrapper that also overrides TG_PROXY_SECRET before enabling.
#[no_mangle]
pub extern "C" fn enable_with_secret(secret_ptr: *const c_char) -> *mut c_char {
    if !secret_ptr.is_null() {
        let cstr = unsafe { CStr::from_ptr(secret_ptr) };
        if let Ok(s) = cstr.to_str() {
            if !s.is_empty() {
                std::env::set_var("TG_PROXY_SECRET", s);
            }
        }
    }
    enable()
}

// ---------------------------------------------------------------------------
// Shared TG proxy startup
// ---------------------------------------------------------------------------

/// Start the in-process Rust MTProto↔WebSocket proxy on 127.0.0.1:1444 and
/// materialise the `tg://proxy?…` URL next to the secret. Portable: no
/// `nft`, no `pkexec`, just tokio + tokio-rustls.
pub(crate) async fn start_tg() -> anyhow::Result<(
    CancellationToken,
    tokio::task::JoinHandle<std::io::Result<()>>,
)> {
    let tg_port: u16 = std::env::var("TG_PROXY_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1444);

    // Kill stale tg listener (previously daemon/standalone).
    let _ = std::process::Command::new("fuser")
        .args(["-k", &format!("{tg_port}/tcp")])
        .output();
    let _ = std::process::Command::new("pkill")
        .args(["-f", "tg-ws-proxy"])
        .output();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let etc = paths().etc_dir;
    let secret = std::env::var("TG_PROXY_SECRET").unwrap_or_else(|_| {
        let secret_path = etc.join("tg-proxy.secret");
        if let Ok(existing) = std::fs::read_to_string(&secret_path) {
            let s = existing.trim().to_string();
            if !s.is_empty() && s.len() == 32 {
                return s;
            }
        }
        use std::io::Read;
        let mut bytes = [0u8; 16];
        let _ = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
        let s: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
        let _ = std::fs::create_dir_all(&etc);
        let _ = std::fs::write(secret_path, &s);
        s
    });
    let tg_url = format!("tg://proxy?server=127.0.0.1&port={tg_port}&secret=dd{secret}");
    let _ = std::fs::create_dir_all(&etc);
    let _ = std::fs::write(etc.join("tg-proxy.url"), &tg_url);

    let mut dc_map = std::collections::HashMap::new();
    dc_map.insert(2, "149.154.167.220".to_string());
    dc_map.insert(4, "149.154.167.220".to_string());

    let cache_dir = paths().cache_dir;
    crate::telegram::start("127.0.0.1", tg_port, &secret, dc_map, Some(cache_dir)).await
}

// ---------------------------------------------------------------------------
// Diagnostics — test each module in isolation, show exactly what breaks
// ---------------------------------------------------------------------------

/// Build a raw DNS A-record query for `domain`.
fn build_dns_query(domain: &str) -> Vec<u8> {
    let mut q = Vec::with_capacity(512);
    q.extend_from_slice(&[0xAB, 0xCD]); // ID
    q.extend_from_slice(&[0x01, 0x00]); // flags: RD=1
    q.extend_from_slice(&[0x00, 0x01]); // QDCOUNT=1
    q.extend_from_slice(&[0x00, 0x00]); // ANCOUNT
    q.extend_from_slice(&[0x00, 0x00]); // NSCOUNT
    q.extend_from_slice(&[0x00, 0x00]); // ARCOUNT
    for label in domain.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&[0x00, 0x01]); // QTYPE=A
    q.extend_from_slice(&[0x00, 0x01]); // QCLASS=IN
    q
}

/// Parse first A-record IP from DNS response.
fn parse_dns_response(resp: &[u8]) -> Option<String> {
    if resp.len() < 12 { return None; }
    let ancount = u16::from_be_bytes([resp[6], resp[7]]) as usize;
    if ancount == 0 { return None; }
    let mut off = 12;
    while off < resp.len() {
        let len = resp[off] as usize;
        if len == 0 { off += 1; break; }
        if len & 0xC0 == 0xC0 { off += 2; break; }
        off += 1 + len;
    }
    off += 4;
    for _ in 0..ancount {
        if off >= resp.len() { return None; }
        if resp[off] & 0xC0 == 0xC0 { off += 2; }
        else { while off < resp.len() && resp[off] != 0 { off += 1 + resp[off] as usize; } off += 1; }
        if off + 10 > resp.len() { return None; }
        let rtype = u16::from_be_bytes([resp[off], resp[off+1]]);
        let rdlen = u16::from_be_bytes([resp[off+8], resp[off+9]]) as usize;
        off += 10;
        if rtype == 1 && rdlen == 4 && off + 4 <= resp.len() {
            return Some(format!("{}.{}.{}.{}", resp[off], resp[off+1], resp[off+2], resp[off+3]));
        }
        off += rdlen;
    }
    None
}

/// Raw UDP DNS query to specific server. Returns (ip, ms) or error.
async fn test_dns_udp(server: &str, domain: &str, timeout_ms: u64) -> Result<(String, u64), String> {
    use std::time::Instant;
    let start = Instant::now();
    let query = build_dns_query(domain);
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await.map_err(|e| format!("bind: {e}"))?;
    sock.send_to(&query, format!("{server}:53")).await.map_err(|e| format!("send: {e}"))?;
    let mut buf = [0u8; 512];
    let n = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), sock.recv_from(&mut buf))
        .await.map_err(|_| format!("timeout {timeout_ms}ms"))?
        .map_err(|e| format!("recv: {e}"))?.0;
    let ms = start.elapsed().as_millis() as u64;
    let ip = parse_dns_response(&buf[..n]).ok_or("no A record")?;
    Ok((ip, ms))
}

/// DNS via system resolver (goes through VPN→TUN→hev→byedpi→internet).
async fn test_dns_system(domain: &str) -> (bool, String) {
    match tokio::net::lookup_host(format!("{domain}:443")).await {
        Ok(addrs) => {
            let ips: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();
            if ips.is_empty() { (false, "no addresses".to_string()) }
            else { (true, ips.join(", ")) }
        }
        Err(e) => (false, format!("{e}")),
    }
}

/// HTTPS GET through byedpi SOCKS5 chain. Returns (ok, ms, status_or_error).
async fn test_https_byedpi(host: &str, timeout_secs: u64) -> (bool, u64, String) {
    use std::time::Instant;
    let start = Instant::now();
    let r: Result<String, String> = async {
        let mut s = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs),
            tokio::net::TcpStream::connect("127.0.0.1:1081"))
            .await.map_err(|_| "timeout byedpi".to_string())?.map_err(|e| format!("connect: {e}"))?;
        s.write_all(&[0x05, 0x01, 0x00]).await.map_err(|e| format!("{e}"))?;
        let mut r = [0u8; 2]; s.read_exact(&mut r).await.map_err(|e| format!("{e}"))?;
        if r[1] != 0x00 { return Err(format!("auth {:#x}", r[1])); }
        let hb = host.as_bytes();
        let mut req = Vec::with_capacity(7 + hb.len());
        req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03]);
        req.push(hb.len() as u8); req.extend_from_slice(hb);
        req.extend_from_slice(&443u16.to_be_bytes());
        s.write_all(&req).await.map_err(|e| format!("{e}"))?;
        let mut h = [0u8; 4]; s.read_exact(&mut h).await.map_err(|e| format!("{e}"))?;
        if h[1] != 0x00 { return Err(format!("reject {:#x}", h[1])); }
        match h[3] {
            0x01 => { let mut a=[0u8;6]; s.read_exact(&mut a).await.ok(); }
            0x03 => { let mut l=[0u8;1]; s.read_exact(&mut l).await.ok();
                      let mut a=vec![0u8;l[0]as usize+2]; s.read_exact(&mut a).await.ok(); }
            0x04 => { let mut a=[0u8;18]; s.read_exact(&mut a).await.ok(); }
            _ => return Err(format!("bad atyp {:#x}", h[3])),
        }
        s.write_all(format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes())
            .await.map_err(|e| format!("{e}"))?;
        let mut buf = [0u8; 1024];
        let n = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), s.read(&mut buf))
            .await.map_err(|_| "timeout".to_string())?.map_err(|e| format!("{e}"))?;
        if n == 0 { return Err("empty".to_string()); }
        Ok(String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("?").to_string())
    }.await;
    let ms = start.elapsed().as_millis() as u64;
    match r { Ok(s) => (true, ms, s), Err(e) => (false, ms, e) }
}

/// SOCKS5 CONNECT test through byedpi. Returns (ok, target, ms, detail).
async fn socks5_test(target: &str, timeout_secs: u64) -> (bool, String, u64, String) {
    use std::time::Instant;
    let start = Instant::now();
    let r: Result<String, String> = async {
        let mut s = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs),
            tokio::net::TcpStream::connect("127.0.0.1:1081"))
            .await.map_err(|_| "timeout".to_string())?.map_err(|e| format!("{e}"))?;
        s.write_all(&[0x05, 0x01, 0x00]).await.map_err(|e| format!("{e}"))?;
        let mut r = [0u8; 2]; s.read_exact(&mut r).await.map_err(|e| format!("{e}"))?;
        if r[1] != 0x00 { return Err(format!("auth {:#x}", r[1])); }
        let (host, port_s) = target.rsplit_once(':').unwrap_or((target, "443"));
        let port: u16 = port_s.parse().unwrap_or(443);
        let hb = host.as_bytes();
        if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
            let mut req = Vec::with_capacity(10);
            req.extend_from_slice(&[0x05, 0x01, 0x00, 0x01]);
            req.extend_from_slice(&ip.octets()); req.extend_from_slice(&port.to_be_bytes());
            s.write_all(&req).await.map_err(|e| format!("{e}"))?;
        } else {
            let mut req = Vec::with_capacity(7 + hb.len());
            req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03]);
            req.push(hb.len() as u8); req.extend_from_slice(hb);
            req.extend_from_slice(&port.to_be_bytes());
            s.write_all(&req).await.map_err(|e| format!("{e}"))?;
        }
        let mut h = [0u8; 4]; s.read_exact(&mut h).await.map_err(|e| format!("{e}"))?;
        if h[1] != 0x00 { return Err(format!("rejected {:#x}", h[1])); }
        Ok(format!("connected {:#x}", h[1]))
    }.await;
    let ms = start.elapsed().as_millis() as u64;
    match r { Ok(d) => (true, target.into(), ms, d), Err(e) => (false, target.into(), ms, e) }
}

/// Run all diagnostic tests. Each module tested independently.
/// Returns a C string that the caller must free with free_string().
#[no_mangle]
pub extern "C" fn run_diagnostics() -> *mut c_char {
    let rt = ffi_runtime();
    let results = rt.block_on(async {
        let mut r = serde_json::Map::new();

        // ═══ MODULE 1: Engine health ═══
        r.insert("vpn_active".into(), serde_json::Value::Bool(crate::system::is_enabled()));
        let byedpi_ok = tokio::net::TcpStream::connect("127.0.0.1:1081").await.is_ok();
        r.insert("byedpi_listening".into(), serde_json::Value::Bool(byedpi_ok));

        // ═══ MODULE 2: DNS — each server independently ═══
        let mut dns = serde_json::Map::new();
        let (sys_ok, sys_detail) = test_dns_system("vk.com").await;
        dns.insert("system_resolver".into(), serde_json::json!({"ok": sys_ok, "detail": sys_detail}));
        for server in ["1.1.1.1", "8.8.8.8", "9.9.9.9"] {
            let (ok, detail) = match test_dns_udp(server, "vk.com", 3000).await {
                Ok((ip, ms)) => (true, format!("{ip} ({ms}ms)")),
                Err(e) => (false, e),
            };
            dns.insert(server.into(), serde_json::json!({"ok": ok, "detail": detail}));
        }
        r.insert("dns".into(), serde_json::Value::Object(dns));

        // ═══ MODULE 3: TCP — direct vs through byedpi ═══
        let mut tcp = serde_json::Map::new();
        for (label, addr) in [("cf_1.1.1.1", "1.1.1.1:443"), ("g_8.8.8.8", "8.8.8.8:53"),
                               ("vk_ip", "157.240.0.174:443")] {
            let start = std::time::Instant::now();
            let ok = tokio::time::timeout(std::time::Duration::from_secs(3),
                tokio::net::TcpStream::connect(addr)).await.is_ok();
            let ms = start.elapsed().as_millis() as u64;
            tcp.insert(format!("direct_{label}"), serde_json::json!({"ok": ok, "ms": ms}));
        }
        for target in ["1.1.1.1:443", "vk.com:443", "example.com:443"] {
            let (ok, _, ms, detail) = socks5_test(target, 5).await;
            tcp.insert(format!("byedpi_{target}"), serde_json::json!({"ok": ok, "ms": ms, "detail": detail}));
        }
        r.insert("tcp".into(), serde_json::Value::Object(tcp));

        // ═══ MODULE 4: HTTP — real content through byedpi ═══
        let mut http = serde_json::Map::new();
        // Port 80 plain HTTP (no TLS, no desync corruption possible)
        for host in ["vk.com", "example.com"] {
            let start = std::time::Instant::now();
            let r: Result<(u16, String), String> = async {
                let mut s = tokio::time::timeout(std::time::Duration::from_secs(8),
                    tokio::net::TcpStream::connect("127.0.0.1:1081"))
                    .await.map_err(|_| "timeout".to_string())?.map_err(|e| format!("{e}"))?;
                s.write_all(&[0x05, 0x01, 0x00]).await.map_err(|e| format!("{e}"))?;
                let mut r = [0u8; 2]; s.read_exact(&mut r).await.map_err(|e| format!("{e}"))?;
                if r[1] != 0x00 { return Err(format!("auth {:#x}", r[1])); }
                let hb = host.as_bytes();
                let mut req = Vec::with_capacity(7 + hb.len());
                req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03]);
                req.push(hb.len() as u8); req.extend_from_slice(hb);
                req.extend_from_slice(&80u16.to_be_bytes());
                s.write_all(&req).await.map_err(|e| format!("{e}"))?;
                let mut h = [0u8; 4]; s.read_exact(&mut h).await.map_err(|e| format!("{e}"))?;
                if h[1] != 0x00 { return Err(format!("reject {:#x}", h[1])); }
                match h[3] { 0x01=>{let mut a=[0u8;6];s.read_exact(&mut a).await.ok();}
                    0x03=>{let mut l=[0u8;1];s.read_exact(&mut l).await.ok();let mut a=vec![0u8;l[0]as usize+2];s.read_exact(&mut a).await.ok();}
                    0x04=>{let mut a=[0u8;18];s.read_exact(&mut a).await.ok();}
                    _=>return Err(format!("bad atyp {:#x}",h[3])), }
                s.write_all(format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes())
                    .await.map_err(|e| format!("{e}"))?;
                let mut buf = vec![0u8; 8192];
                let n = tokio::time::timeout(std::time::Duration::from_secs(8), s.read(&mut buf))
                    .await.map_err(|_| "timeout".to_string())?.map_err(|e| format!("{e}"))?;
                if n == 0 { return Err("empty".to_string()); }
                let resp = String::from_utf8_lossy(&buf[..n]);
                let status = resp.lines().next().unwrap_or("?").to_string();
                let has_html = resp.contains("<html") || resp.contains("<HTML") || resp.contains("<!DOCTYPE");
                let body_len = resp.len();
                Ok((200u16, format!("{status} | html={has_html} | {body_len}B")))
            }.await;
            let ms = start.elapsed().as_millis() as u64;
            match r {
                Ok((_, detail)) => { http.insert(format!("port80_{host}"), serde_json::json!({"ok": true, "ms": ms, "detail": detail})); }
                Err(e) => { http.insert(format!("port80_{host}"), serde_json::json!({"ok": false, "ms": ms, "detail": e})); }
            }
        }
        // Port 443 HTTPS (TLS — tests if desync corrupts handshake)
        for host in ["vk.com", "example.com"] {
            let (ok, ms, detail) = test_https_byedpi(host, 8).await;
            http.insert(format!("port443_{host}"), serde_json::json!({"ok": ok, "ms": ms, "detail": detail}));
        }
        // Blocked site test
        {
            let (ok, ms, detail) = test_https_byedpi("rutracker.org", 8).await;
            http.insert("blocked_rutracker".into(), serde_json::json!({"ok": ok, "ms": ms, "detail": detail}));
        }
        r.insert("http".into(), serde_json::Value::Object(http));

        serde_json::Value::Object(r)
    });
    let json = serde_json::to_string(&results).unwrap_or_else(|_| "{}".to_string());
    alloc_cstr(&json)
}
