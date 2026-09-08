//! Site-by-site strategy scanner.
//!
//! The idea (from the v6 plan): for each site we want to unblock, we
//! try every strategy in the pool, and keep the first that achieves
//! a clean TLS handshake with a valid cert. If zero strategies work,
//! we fall back to an IP scan over the curated known-good ranges.
//!
//! This is the desktop equivalent of what byeDPI does on Android.
//! The result is a per-site config file `/etc/unblocker/sites/<host>.conf`
//! that the daemon reads at startup. nfqws applies only the winning
//! strategy for each host.

use anyhow::Result;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// How long a single TLS probe may take. 5s is a reasonable ceiling
/// per attempt. With 71 strategies × 5s = 6 minutes per site, which
/// is acceptable for an install-time scan.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// A single probe result. We keep all of it so the install-discover
/// log shows the full picture to the user, not just "yes/no".
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub strat: String,
    pub host: String,
    pub tcp_443: bool,
    pub tls_handshake: bool,
    pub cert_chain_valid: bool,
    pub http_200: bool,
    pub ms: u64,
    pub error: Option<String>,
}

/// Try to read a TLS server-hello from the given IP:port with the given SNI.
/// We do NOT verify the cert chain (that would require trusting a CA bundle);
/// we only need to know "did the SNI reach a server that talks TLS to us".
/// Returns the elapsed time on success.
pub fn probe_tls(ip: IpAddr, host: &str, port: u16, timeout: Duration) -> Option<u64> {
    use std::net::TcpStream;
    let addr = SocketAddr::new(ip, port);
    let t0 = Instant::now();
    let stream = TcpStream::connect_timeout(&addr, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    let mut s = stream;
    // Build a minimal TLS 1.3 ClientHello with a single cipher suite
    // (TLS_AES_128_GCM_SHA256 = 0x1301) and an SNI extension for `host`.
    let sni_host = host.as_bytes();
    let mut sni_payload = Vec::new();
    // server_name list length
    let list_len = (2 + 1 + 2 + sni_host.len()) as u16;
    sni_payload.extend_from_slice(&list_len.to_be_bytes());
    // server_name_type (host_name = 0)
    sni_payload.push(0);
    // host_name length
    sni_payload.extend_from_slice(&(sni_host.len() as u16).to_be_bytes());
    sni_payload.extend_from_slice(sni_host);

    let mut extensions = Vec::new();
    // server_name extension (type=0x0000)
    extensions.extend_from_slice(&0x0000u16.to_be_bytes());
    extensions.extend_from_slice(&(sni_payload.len() as u16).to_be_bytes());
    extensions.extend_from_slice(&sni_payload);

    // 32 random bytes for the random field
    let random: [u8; 32] = [0xAB; 32];
    // session_id length = 0
    let session_id_len: u8 = 0;
    // cipher_suites length (1 suite, 2 bytes)
    let cipher_suites: [u8; 2] = [0x13, 0x01];
    // compression methods length (1 method, 1 byte)
    let compression: u8 = 0;

    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(&random);
    body.push(session_id_len);
    body.extend_from_slice(&cipher_suites);
    body.push(compression);
    body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
    body.extend_from_slice(&extensions);

    let mut hello = Vec::new();
    hello.push(0x01); // client_hello
    hello.push(0x00);
    hello.push(0x03); // legacy version
    hello.extend_from_slice(&(body.len() as u16).to_be_bytes());
    hello.extend_from_slice(&body);

    let mut record = Vec::new();
    record.push(0x16); // handshake
    record.push(0x03);
    record.push(0x01);
    record.extend_from_slice(&(hello.len() as u16).to_be_bytes());
    record.extend_from_slice(&hello);

    if s.write_all(&record).is_err() { return None; }
    let mut header = [0u8; 5];
    if s.read_exact(&mut header).is_err() { return None; }
    if header[0] != 0x16 { return None; } // got a record, but not a handshake
    Some(t0.elapsed().as_millis() as u64)
}

/// Test a strategy against a host: install a *probe-only* nft table that
/// queues uid=<probe_uid> tcp/443 traffic to a unique NFQUEUE number,
/// start nfqws on that queue with the strategy's args, then issue a real
/// curl **as the probe uid** so the curl traffic gets NFQUEUE'd exactly
/// the same way the runtime user's traffic does. Returns whether the
/// host loaded with HTTP 200.
///
/// Why this rewrite (v6.2): the previous version used
///   curl --proxy socks5h://127.0.0.1:<qnum>
/// which assumed nfqws was a SOCKS5 server. nfqws is not. The probe
/// always failed, install-discover always printed "no winner", and the
/// per-site config files were never written. The runtime fallback
/// (`/etc/unblocker/strategy.conf`) is what made the 6 working sites
/// appear to work, masking the bug.
/// Probe table prefix so we can clean up ALL probe tables on exit.
pub const PROBE_TABLE_PREFIX: &str = "unblocker_probe_";

/// Clean up all leftover probe nft tables. Called on install-discover
/// exit and by the daemon at startup. A stale probe table with no
/// queue listener would silently black-hole any matching traffic,
/// so this cleanup is mandatory.
pub fn nft_flush_probe_tables() {
    // First, try the simple path: delete the known unblocker table.
    let _ = Command::new("nft")
        .args(["delete", "table", "inet", "unblocker"]).output();
    // Now list all tables and delete any starting with the probe prefix.
    // Use `nft -a list tables` for reliable parsing (the -a flag gives
    // handles, but we don't use them here; it's just a no-op).
    if let Ok(out) = Command::new("nft")
        .args(["list", "tables"]).output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let line = line.trim();
            // Lines look like: "table inet unblocker_probe_52051_7"
            if let Some(name) = line.strip_prefix("table inet ")
                .or_else(|| line.strip_prefix("table ip "))
            {
                if name.starts_with(PROBE_TABLE_PREFIX) {
                    let del = Command::new("nft")
                        .args(["delete", "table", "inet", name]).output();
                    // Log the result so we can see if it actually ran.
                    match del {
                        Ok(o) if !o.status.success() => {
                            eprintln!("probe table delete failed: {}", String::from_utf8_lossy(&o.stderr));
                        }
                        Ok(_) => {
                            eprintln!("probe table '{}' deleted", name);
                        }
                        Err(e) => {
                            eprintln!("probe table delete error: {}", e);
                        }
                    }
                }
            }
        }
    }
}

pub async fn probe_strategy(
    strat_args: &[String],
    host: &str,
    test_url: &str,
) -> ProbeResult {
    use std::process::Stdio;

    // Per-strategy unique queue + table so consecutive probes don't
    // collide. We can't reuse the table across strategies because the
    // `add table` would fail on the second call. Atomic counter is
    // overkill; a thread-local-ish static u64 via AtomicU64 works.
    use std::sync::atomic::{AtomicU64, Ordering};
    static PROBE_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = PROBE_SEQ.fetch_add(1, Ordering::Relaxed);
    let q: u16 = 201 + (seq as u16 % 50);
    let probe_uid: u32 = 65534; // "nobody" — already exists on most distros
    let table = format!("{}{}_{}", PROBE_TABLE_PREFIX, std::process::id(), seq);

    // 1. Install probe-only nft table: queue traffic from probe_uid
    //    on tcp/443 and udp/443 to NFQUEUE <q>.
    let nft_install_ok = (|| -> std::io::Result<()> {
        let nft = |args: &[&str]| -> std::io::Result<std::process::Output> {
            let out = Command::new("nft").args(args).output()?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("nft {:?} failed: {}", args, stderr.trim()),
                ));
            }
            Ok(out)
        };
        let _ = nft(&["delete", "table", "inet", &table]);
        let uid_s = probe_uid.to_string();
        let q_s = q.to_string();
        nft(&["add", "table", "inet", &table])?;
        nft(&["add", "chain", "inet", &table, "output",
              "{type", "filter", "hook", "output", "priority", "mangle;}"])?;
        nft(&["add", "rule", "inet", &table, "output",
              "meta", "skuid", &uid_s, "tcp", "dport", "{80,443}",
              "queue", "num", &q_s, "bypass"])?;
        nft(&["add", "rule", "inet", &table, "output",
              "meta", "skuid", &uid_s, "udp", "dport", "443",
              "queue", "num", &q_s, "bypass"])?;
        Ok(())
    })();
    if let Err(e) = nft_install_ok {
        warn!(host=%host, "probe nft install failed: {}", e);
        return ProbeResult {
            strat: strat_args.join(" "),
            host: host.to_string(),
            tcp_443: false, tls_handshake: false, cert_chain_valid: false,
            http_200: false, ms: 0,
            error: Some(format!("nft install: {}", e)),
        };
    }
    info!(host=%host, q=%q, "probe nft installed");

    // 2. Start nfqws on the probe queue. The binary is installed at
    //    /usr/local/lib/unblocker/nfqws with a /usr/local/bin/nfqws
    //    symlink, but pkexec (which install-discover runs under) uses
    //    a stripped PATH that excludes /usr/local/bin. Look up the
    //    real path so the probe works whether or not /usr/local/bin
    //    is on PATH.
    let nfqws_bin = std::path::Path::new("/usr/local/lib/unblocker/nfqws");
    let nfqws_bin = if nfqws_bin.exists() {
        nfqws_bin.to_path_buf()
    } else if std::path::Path::new("/usr/local/bin/nfqws").exists() {
        std::path::PathBuf::from("/usr/local/bin/nfqws")
    } else {
        std::path::PathBuf::from("nfqws")
    };
    let mut cmd = Command::new(&nfqws_bin);
    cmd.args(strat_args);
    cmd.args([
        "--qnum", &q.to_string(),
        "--debug=1",  // capture nfqws's stderr so we can see why a strat failed
    ]);
    cmd.stdin(Stdio::null()).stdout(Stdio::null());
    // The log file is created once and appended to across strategies.
    // Using a per-probe PID+seq suffix avoids concurrent collisions.
    let log_path = format!("/tmp/unblocker-probe-nfqws-{}-{}.log",
                           std::process::id(), seq);
    if let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        cmd.stderr(Stdio::from(f));
    }

    // Quick-fail: if the strategy's args are obviously malformed (e.g.
    // references a non-existent hostlist file, uses a flag the nfqws
    // binary doesn't know), nfqws will print to stderr and exit. We
    // detect this with a non-blocking wait: if the process is gone
    // within 50ms, skip this strategy. This is a "did nfqws accept
    // the args" gate, not a test of the strategy's effectiveness.
    let _early_deadline = Instant::now() + Duration::from_millis(80);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            // Clean up the probe table before bailing.
            let _ = Command::new("nft")
                .args(["delete", "table", "inet", &table]).output();
            warn!(host=%host, "nfqws spawn failed: {}", e);
            return ProbeResult {
                strat: strat_args.join(" "),
                host: host.to_string(),
                tcp_443: false, tls_handshake: false, cert_chain_valid: false,
                http_200: false, ms: 0,
                error: Some(format!("nfqws spawn: {}", e)),
            };
        }
    };
    let nfqws_pid = child.id();
    info!(host=%host, pid=nfqws_pid, "nfqws spawned, sleeping 200ms");
    // Settle for nfqws to bind NFQUEUE.
    std::thread::sleep(Duration::from_millis(200));

    // Quick-fail gate: did nfqws die from malformed args in the first
    // 200ms? If so, skip this strategy immediately so we don't waste
    // 5s of curl timeout per broken strat.
    match child.try_wait() {
        Ok(Some(status)) => {
            let _ = Command::new("nft")
                .args(["delete", "table", "inet", &table]).output();
            warn!(host=%host, pid=nfqws_pid, exit=?status.code(), "nfqws exited early (likely bad strat args); skipping");
            return ProbeResult {
                strat: strat_args.join(" "),
                host: host.to_string(),
                tcp_443: false, tls_handshake: false, cert_chain_valid: false,
                http_200: false, ms: 0,
                error: Some(format!("nfqws exited early: {:?}", status.code())),
            };
        }
        _ => {}
    }

    // 3. Run curl as probe_uid so the nft rule matches and the
    //    traffic goes through nfqws. pre_exec() runs in the forked
    //    child between fork() and execve(), so setuid is safe there.
    let t0 = Instant::now();
    let mut curl = Command::new("curl");
    curl.args([
        "--max-time", "4",
        "-4", "-sS", "-L",
        "-o", "/dev/null",
        "-w", "%{http_code}",
        test_url,
    ]);
    // SAFETY: pre_exec is a closure that runs in the forked child
    // between fork() and execve(). The libc calls below are async-signal
    // safe in that context.
    unsafe {
        curl.pre_exec(move || {
            extern "C" {
                fn setgroups(n: libc::c_int, groups: *const libc::gid_t) -> libc::c_int;
                fn setgid(g: libc::gid_t) -> libc::c_int;
                fn setuid(u: libc::uid_t) -> libc::c_int;
            }
            // Clear supplementary groups first.
            let _ = setgroups(0, std::ptr::null());
            // setgid before setuid (POSIX requirement).
            let _ = setgid(probe_uid);
            let _ = setuid(probe_uid);
            Ok(())
        });
    }
    let out = curl.output();
    let ms = t0.elapsed().as_millis() as u64;

    // 4. Cleanup. ALWAYS run, even on error. Order matters: kill
    //    nfqws first (it holds the NFQUEUE), then remove the nft
    //    rule (so no traffic is queued), then nft flushes the
    //    table as a safety net.
    let pid = child.id();
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
    let _ = child.wait();
    let _ = Command::new("nft")
        .args(["delete", "table", "inet", &table])
        .output();

    let (curl_ok, http_200, err_msg) = match out {
        Ok(o) => {
            let body = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let stderr_s = String::from_utf8_lossy(&o.stderr).to_string();
            // 200 or any 3xx counts. "000" is curl's "could not get
            // an HTTP code" marker.
            let http_200 = body == "200" || body.starts_with('3');
            let err = if !o.status.success() && !http_200 {
                Some(format!(
                    "curl exit={:?} body={} stderr={}",
                    o.status.code(), body,
                    stderr_s.lines().next().unwrap_or("").chars().take(120).collect::<String>()
                ))
            } else {
                None
            };
            (o.status.success() || http_200, http_200, err)
        }
        Err(e) => (false, false, Some(format!("curl spawn: {}", e))),
    };

    info!(host=%host, http_200, curl_ok, ms, ?err_msg, "probe_strategy result");

    ProbeResult {
        strat: strat_args.join(" "),
        host: host.to_string(),
        tcp_443: curl_ok,
        tls_handshake: http_200,
        cert_chain_valid: http_200,
        http_200,
        ms,
        error: err_msg,
    }
}

/// Run each strategy in `pool` against `host` (using `test_url` for
/// the actual probe) and return the first one that achieves a
/// working TLS handshake. Uses a simple TCP+TLS probe on a target
/// IP from `test_ip_ranges` (or from the system DNS if not given).
pub async fn find_working_strategy(
    host: &str,
    test_url: &str,
    pool: &[PathBuf],
) -> Result<Option<PathBuf>> {
    for strat in pool {
        // Read the strategy file, split into argv.
        let args: Vec<String> = std::fs::read_to_string(strat)?
            .split_whitespace()
            .map(String::from)
            .collect();
        if args.is_empty() { continue; }
        // First try a real HTTPS GET through the queue.
        let result = probe_strategy(&args, host, test_url).await;
        if result.http_200 {
            info!(host=%host, strat=%strat.display(), "find_working_strategy: winner");
            return Ok(Some(strat.clone()));
        }
    }
    warn!(host=%host, "find_working_strategy: all {} strategies failed", pool.len());
    Ok(None)
}

/// IP scan: try a set of candidate IPs for `host` and pick the first
/// that accepts a TLS handshake. Used when no strategy wins.
pub async fn scan_for_clean_ip(
    host: &str,
    test_url: &str,
    candidates: &[std::net::IpAddr],
) -> Result<Option<IpAddr>> {
    use std::net::TcpStream;
    for &ip in candidates {
        // 1. Quick TCP connect
        let addr = SocketAddr::new(ip, 443);
        if TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_err() { continue; }
        // 2. TLS handshake (cheap: no full HTTP)
        if probe_tls(ip, host, 443, PROBE_TIMEOUT).is_none() { continue; }
        // 3. Real HTTP fetch via the IP (resolve.conf must work)
        // We use a short curl with --resolve to pin the IP.
        let _resolved_url = test_url.replace("https://", &format!("https://{}", ip));
        let out = std::process::Command::new("curl")
            .args([
                "--max-time", "5",
                "-4", "-sk", "-o", "/dev/null",
                "-w", "%{http_code}",
                "--resolve", &format!("{}:443:{}", host, ip),
                test_url,
            ])
            .output();
        if let Ok(o) = out {
            if o.status.success() && o.stdout.starts_with(b"200") {
                return Ok(Some(ip));
            }
        }
    }
    Ok(None)
}
