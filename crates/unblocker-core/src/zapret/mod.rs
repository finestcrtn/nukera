//! zapret engine: vendored nfqws/tpws, nftables routing, strategy loader.
//!
// This module is the only place that touches nftables, iptables, or the
//! zapret binaries. The GUI never reaches into /etc, the daemon is the
//! only thing that talks to netfilter.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use tracing::info;

pub mod probe;
pub mod sites;

const NFQUEUE: u16 = 200;
const NFT_TABLE: &str = "unblocker";
const STRATEGY_FILE: &str = "/etc/unblocker/strategy.conf";
const HOSTLIST_FILE: &str = "/etc/unblocker/hostlists/reestr.txt";
const AUTO_HOSTLIST_FILE: &str = "/etc/unblocker/hostlists/auto.txt";

/// Find the vendored nfqws binary. Search order:
/// 1. Same directory as the unblocker binary (system install: /usr/lib/unblocker/nfqws)
/// 2. Same directory as the unblocker-gui binary
/// 3. exe-relative (cargo run from source tree)
/// 4. ./vendor/zapret/nfqws (dev tree)
/// 5. /usr/local/bin/nfqws, /usr/bin/nfqws
pub fn find_nfqws(workspace_root: &Path) -> Result<PathBuf> {
    let exe = std::env::current_exe().ok();
    let dirs: Vec<PathBuf> = exe
        .as_ref()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
        .into_iter()
        .collect();
    let mut candidates: Vec<PathBuf> = Vec::new();
    // Exe-relative first (bundled deployment next to the GUI binary).
    for d in &dirs {
        candidates.push(d.join("nfqws"));
        candidates.push(d.join("../lib/unblocker/nfqws"));
    }
    // System-installed binaries (these carry the file capabilities the
    // direct non-root spawn relies on).
    candidates.push(PathBuf::from("/usr/lib/unblocker/nfqws"));
    candidates.push(PathBuf::from("/usr/local/lib/unblocker/nfqws"));
    candidates.push(PathBuf::from("/usr/local/bin/nfqws"));
    candidates.push(PathBuf::from("/usr/bin/nfqws"));
    // Dev-tree copy is the last resort: it is NOT setcap'd, so direct
    // spawning it drops CAP_SETPCAP and nfqws aborts with
    // "setpcap: Operation not permitted". Prefer a caped system binary.
    candidates.push(workspace_root.join("vendor/zapret/nfqws"));
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    anyhow::bail!("nfqws not found. Reinstall the package or run scripts/unblocker-install.sh.")
}

/// Path to the active strategy file. /etc/unblocker/strategy.conf in
/// production. In dev (cargo run from source tree) we fall back to the
/// vendored strategies directory.
pub fn default_strategy_path() -> PathBuf {
    if Path::new(STRATEGY_FILE).exists() {
        return PathBuf::from(STRATEGY_FILE);
    }
    // Dev fallback: pick the first .strat file from config/zapret/strategies/
    let candidates = [
        "config/zapret/strategies/general-hostfakesplit.strat",
        "config/zapret/strategies/general-alt11.strat",
    ];
    for c in candidates {
        if Path::new(c).exists() {
            return PathBuf::from(c);
        }
    }
    PathBuf::from(STRATEGY_FILE)
}

/// Load a .strat file into argv tokens. Each line is a flag; comments
/// start with `#`. Blank lines are ignored.
pub fn load_strategy_file(path: &Path) -> Result<Vec<String>> {
    let s = std::fs::read_to_string(path)
        .with_context(|| format!("read strategy file: {}", path.display()))?;
    let mut out = Vec::new();
    for raw in s.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() { continue; }
        // Naive shell-split. Quoted args get their quotes stripped. Good
        // enough for the nfqws flag set we ship.
        for tok in shell_words(line) {
            out.push(tok);
        }
    }
    Ok(out)
}

fn shell_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q: Option<char> = None;
    for ch in s.chars() {
        match in_q {
            Some(q) if ch == q => { in_q = None; }
            Some(_) => { cur.push(ch); }
            None if ch == '"' || ch == '\'' => { in_q = Some(ch); }
            None if ch.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            None => { cur.push(ch); }
        }
    }
    if !cur.is_empty() { out.push(cur); }
    out
}

/// Save a strategy argv back to the strategy file (so the autotune can
/// persist its best pick).
pub fn save_strategy_file(path: &Path, args: &[String]) -> Result<()> {
    let body = args.join(" ");
    std::fs::write(path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Path to the hostlist. /etc/unblocker/hostlists/reestr.txt in prod.
pub fn default_hostlist_path() -> PathBuf {
    if Path::new(HOSTLIST_FILE).exists() {
        return PathBuf::from(HOSTLIST_FILE);
    }
    PathBuf::from("/tmp/hostlists/reestr.txt")
}

/// Path to the auto-grown hostlist (nfqws writes to this on its own).
pub fn auto_hostlist_path() -> PathBuf {
    PathBuf::from(AUTO_HOSTLIST_FILE)
}

/// Install the nftables rule that sends the user's traffic to NFQUEUE.
/// Safe to call multiple times: deletes the rule first, idempotent.
/// No UID filter: nfqws runs as root (via pkexec fallback) and needs to
/// see ALL queued packets. Filtering by skuid caused nfqws to miss packets
/// after it dropped privileges.
pub fn nft_install(_uid: u32, queue: u16) -> Result<()> {
    // Always delete first (idempotent).
    let _ = run_nft(&["delete", "table", "inet", NFT_TABLE]);
    let queue_s = queue.to_string();
    run_nft(&["add", "table", "inet", NFT_TABLE])?;
    run_nft(&["add", "chain", "inet", NFT_TABLE, "output",
        "{type", "filter", "hook", "output", "priority", "mangle;}"])?;
    run_nft(&["add", "rule", "inet", NFT_TABLE, "output",
        "tcp", "dport", "{80,443}",
        "queue", "num", &queue_s, "bypass"])?;
    run_nft(&["add", "rule", "inet", NFT_TABLE, "output",
        "udp", "dport", "443",
        "queue", "num", &queue_s, "bypass"])?;
    info!("nft: NFQUEUE rules installed (no UID filter, queue {})", queue);
    Ok(())
}

/// Install nft rules to redirect Telegram Desktop MTProto traffic to the
/// local TG proxy. Telegram Desktop connects via tg://proxy URL directly
/// to 127.0.0.1:1444, but some DC connections still go to the real IPs.
/// We redirect port 80 (HTTP) only — port 443 (HTTPS) is NOT redirected
/// because the browser also uses port 443 for web.telegram.org and the
/// TG proxy doesn't speak HTTPS. The browser is handled by nfqws DPI
/// desync + hosts entries instead.
pub fn nft_install_tg_redirect(tg_port: u16) -> Result<()> {
    let q_s = tg_port.to_string();
    let table = format!("{NFT_TABLE}_nat");
    let _ = run_nft(&["delete", "table", "inet", &table]);
    run_nft(&["add", "table", "inet", &table])?;
    run_nft(&["add", "chain", "inet", &table, "output",
        "{type", "nat", "hook", "output", "priority", "0;}"])?;

    let ranges = [
        "149.154.128.0/17",
        "149.154.175.0/24",
        "91.108.0.0/16",
        "185.76.151.0/24",
        "194.221.250.0/24",
    ];
    for range in &ranges {
        // Only redirect port 80 (HTTP) — port 443 (HTTPS) goes through
        // nfqws DPI desync + hosts for browser traffic.
        let _ = run_nft(&["add", "rule", "inet", &table, "output",
            "tcp", "dport", "80",
            "ip", "daddr", range, "redirect", "to", &q_s]);
    }

    tracing::info!(port=%tg_port, "nft TG redirect installed (HTTP only, {} ranges)", ranges.len());
    Ok(())
}

/// Remove all nftables unblocker tables (both the main table and the nat redirect).
pub fn nft_flush_safely() {
    let _ = run_nft(&["delete", "table", "inet", NFT_TABLE]);
    let _ = run_nft(&["delete", "table", "inet", &format!("{NFT_TABLE}_nat")]);
}

/// Remove the nftables rule. Tries targeted delete first; on any error,
/// falls back to `nft delete table` which atomically removes everything.
pub fn nft_remove() -> Result<()> {
    // Try targeted first; if it fails (e.g. table already gone) ignore.
    let _ = run_nft(&["delete", "table", "inet", NFT_TABLE]);
    Ok(())
}

/// Atomic safety net: delete ALL unblocker tables. Called on every shutdown
/// path so we NEVER leave a stale rule that could black-hole the user.
/// Cleans both the main nft table and the nat redirect table.

fn run_nft(args: &[&str]) -> Result<()> {
    // Try direct
    let direct = Command::new("nft").args(args).output();
    if let Ok(out) = direct {
        if out.status.success() {
            return Ok(());
        }
        // If failed due to permission, try pkexec / sudo
        let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
        if !stderr.contains("permission") && !stderr.contains("operation not permitted") && !stderr.contains("could not") {
            anyhow::bail!("nft {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr).trim());
        }
    }
    // Try pkexec (polkit, will prompt if needed, or use passwordless rule)
    if std::path::Path::new("/usr/bin/pkexec").exists() {
        let mut pk_args = vec!["nft".to_string()];
        pk_args.extend(args.iter().map(|s| s.to_string()));
        let pk_refs: Vec<&str> = pk_args.iter().map(|s| s.as_str()).collect();
        if let Ok(out) = Command::new("pkexec").args(&pk_refs).output() {
            if out.status.success() {
                return Ok(());
            }
        }
    }
    // Try sudo -n (passwordless sudo)
    {
        let mut sudo_args = vec!["-n".to_string(), "nft".to_string()];
        sudo_args.extend(args.iter().map(|s| s.to_string()));
        let sudo_refs: Vec<&str> = sudo_args.iter().map(|s| s.as_str()).collect();
        if let Ok(out) = Command::new("sudo").args(&sudo_refs).output() {
            if out.status.success() {
                return Ok(());
            }
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("nft {:?} failed (even via pkexec/sudo): {}", args, stderr.trim());
        }
    }
    // Final fallback: try direct again for error message
    let out = Command::new("nft")
        .args(args)
        .output()
        .with_context(|| format!("running nft {:?}", args))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("nft {:?} failed: {}", args, stderr.trim());
    }
    Ok(())
}

fn open_log() -> std::fs::File {
    let log_path = std::path::PathBuf::from(format!("/tmp/unblocker-runtime-nfqws-{}.log",
                                                     std::process::id()));
    let _ = std::fs::remove_file(&log_path);
    let _ = std::fs::set_permissions(&log_path, std::os::unix::fs::PermissionsExt::from_mode(0o666));
    if let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = std::fs::set_permissions(&log_path, std::os::unix::fs::PermissionsExt::from_mode(0o666));
        f
    } else {
        drop(std::fs::File::create(&log_path));
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .unwrap_or_else(|_| panic!("cannot open nfqws log {}", log_path.display()))
    }
}

fn is_nfqws_alive() -> bool {
    std::process::Command::new("pgrep")
        .args(["-f", "nfqws.*--qnum"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Spawn the nfqws engine with the given strategy. Returns the Child.
/// Captures stdout/stderr for the per-pid log; the engine is killed when
/// the child is dropped.
///
/// Tries direct spawn first (works when file capabilities are sufficient).
/// If direct fails or nfqws is not alive after a short delay, falls back
/// to `pkexec` which bypasses `NoNewPrivs` restrictions present in
/// the Dart FFI process and grants full root capabilities to nfqws.
pub fn spawn_engine(bin: &Path, args: &[String], _uid: u32) -> Result<Child> {
    // Try direct foreground spawn first.
    // Without --daemon, the Child IS the nfqws process (no defunct parent).
    // File caps on the binary give it CAP_NET_ADMIN etc. without needing
    // to call setpcap or drop privileges via USER env.
    match spawn_engine_foreground(bin, args) {
        Ok(mut child) => {
            std::thread::sleep(std::time::Duration::from_millis(400));
            if is_nfqws_alive() {
                tracing::info!("nfqws: direct foreground spawn, queue {} bound", NFQUEUE);
                return Ok(child);
            }
            // nfqws died — clean up and retry via pkexec
            tracing::warn!("nfqws: direct spawn dead after 400ms, retrying via pkexec");
            let _ = child.kill();
            let _ = child.wait();
            nft_flush_safely();
            spawn_engine_pkexec(bin, args)
        }
        Err(e) => {
            tracing::warn!("nfqws direct spawn failed ({}), retrying via pkexec", e);
            nft_flush_safely();
            spawn_engine_pkexec(bin, args)
        }
    }
}

/// Spawn nfqws in the foreground (no --daemon, no USER env).
/// The returned Child is the actual nfqws process.
fn spawn_engine_foreground(bin: &Path, args: &[String]) -> Result<Child> {
    let mut cmd = Command::new(bin);
    cmd.arg("--qnum").arg(NFQUEUE.to_string())
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::from(open_log()))
        .stdout(Stdio::null());
    let child = cmd.spawn()
        .with_context(|| format!("spawn nfqws: {}", bin.display()))?;
    Ok(child)
}

/// Spawn nfqws via pkexec (runs as root, bypasses NoNewPrivs).
/// Does NOT set USER env — pkexec already runs as root.
fn spawn_engine_pkexec(bin: &Path, args: &[String]) -> Result<Child> {
    let mut cmd = Command::new("pkexec");
    cmd.arg(bin)
        .arg("--qnum").arg(NFQUEUE.to_string())
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::from(open_log()))
        .stdout(Stdio::null());
    let child = cmd.spawn()
        .with_context(|| format!("spawn nfqws via pkexec: {}", bin.display()))?;
    tracing::info!("nfqws: spawned via pkexec as root: {} --qnum {}", bin.display(), NFQUEUE);
    Ok(child)
}

/// Probe a URL with a plain HTTPS request, no proxy. Returns true if
/// the response is 2xx/3xx within the timeout.
///
/// IMPORTANT: the runtime runs this as root, but the nft rule only
/// matches `meta skuid 1000` traffic, so a root reqwest probe bypasses
/// nfqws entirely. The probe is therefore useless for judging the DPI
/// bypass state. Real site health is judged by the user's browser
/// traffic, not by the daemon probing as root. The autotune should
/// rely on this function only as a "did the world go down" check
/// (e.g. network is up) and not on the per-site DPI state.
pub async fn probe_direct(url: &str, timeout_secs: u64) -> bool {
    use std::time::Duration;
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    match client.get(url).send().await {
        Ok(r) => r.status().is_success() || r.status().is_redirection(),
        Err(_) => false,
    }
}
