use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::bypass::{BypassConfig, BypassHandle};
use crate::config::AppConfig;

const MARKER_BEGIN: &str = "# BEGIN unblocker";
const MARKER_END: &str = "# END unblocker";

// State file path is /tmp/unblocker.state on Linux desktops; the Flutter
// layer overrides it via FFI set_app_paths() on mobile (app data dir).
use once_cell::sync::Lazy;
use parking_lot::Mutex as ParkMutex;
static STATE_FILE: Lazy<ParkMutex<PathBuf>> =
    Lazy::new(|| ParkMutex::new(PathBuf::from("/tmp/unblocker.state")));

pub fn set_state_file(p: PathBuf) {
    *STATE_FILE.lock() = p;
}

pub fn state_file() -> PathBuf {
    STATE_FILE.lock().clone()
}

fn pkexec_available() -> bool {
    std::path::Path::new("/usr/bin/pkexec").exists()
}

fn run(cmd: &str, args: &[&str]) -> Result<std::process::Output> {
    let out = std::process::Command::new(cmd).args(args).output()?;
    Ok(out)
}

fn run_root(cmd: &str, args: &[&str]) -> Result<std::process::Output> {
    // Try unprivileged first.
    if let Ok(out) = run(cmd, args) {
        if out.status.success() {
            return Ok(out);
        }
    }
    // Try pkexec (polkit, passwordless if rule installed)
    if pkexec_available() {
        let mut pk_args = vec![cmd.to_string()];
        pk_args.extend(args.iter().map(|s| s.to_string()));
        let pk_refs: Vec<&str> = pk_args.iter().map(|s| s.as_str()).collect();
        if let Ok(out) = run("pkexec", &pk_refs) {
            if out.status.success() {
                return Ok(out);
            }
        }
    }
    // Try passwordless sudo.
    let mut nopasswd_args: Vec<String> = vec!["-n".to_string(), cmd.to_string()];
    nopasswd_args.extend(args.iter().map(|s| s.to_string()));
    let nopasswd_refs: Vec<&str> = nopasswd_args.iter().map(|s| s.as_str()).collect();
    if let Ok(out) = run("sudo", &nopasswd_refs) {
        if out.status.success() {
            return Ok(out);
        }
    }
    // Fallback direct (maybe setcap)
    run(cmd, args)
}

fn _atty_unused() { let _ = atty::is(atty::Stream::Stdin); }

pub fn system_proxy_enable(port: u16) -> Result<()> {
    // Running as root (systemd daemon): gsettings writes to root's
    // session and the environment.d file would land in /root. The real
    // bypass for the desktop user is nft + hosts + nfqws, which the
    // daemon applies regardless. Skip user-session proxy setup here.
    if running_as_root() {
        info!(%port, "system proxy: skipped (running as root — nft+hosts carry the bypass)");
        return Ok(());
    }
    for (schema, key, val) in [
        ("org.gnome.system.proxy", "mode", "'manual'"),
        ("org.gnome.system.proxy.http", "host", "'127.0.0.1'"),
        ("org.gnome.system.proxy.https", "host", "'127.0.0.1'"),
        ("org.gnome.system.proxy.socks", "host", "'127.0.0.1'"),
        ("org.gnome.system.proxy.ftp", "host", "'127.0.0.1'"),
    ] {
        let _ = run("gsettings", &["set", schema, key, val]);
    }
    for (schema, key) in [
        ("org.gnome.system.proxy.http", "port"),
        ("org.gnome.system.proxy.https", "port"),
        ("org.gnome.system.proxy.socks", "port"),
        ("org.gnome.system.proxy.ftp", "port"),
    ] {
        let _ = run("gsettings", &["set", schema, key, &port.to_string()]);
    }
    let _ = run("gsettings", &["set", "org.gnome.system.proxy", "ignore-hosts", "['localhost', '127.0.0.0/8', '::1']"]);
    for k in ["socks_proxy", "SOCKS_PROXY", "ALL_PROXY", "all_proxy"] {
        let sv = format!("socks5h://127.0.0.1:{port}");
        std::env::set_var(k, sv);
    }
    if let Some(home) = dirs::home_dir() {
        let envd = home.join(".config/environment.d/unblocker.conf");
        let _ = std::fs::create_dir_all(envd.parent().unwrap());
        let _ = std::fs::write(&envd, format!("socks_proxy=socks5h://127.0.0.1:{port}\nSOCKS_PROXY=socks5h://127.0.0.1:{port}\nall_proxy=socks5h://127.0.0.1:{port}\n"));
    }
    info!(%port, "system proxy enabled via gsettings (SOCKS) + env (socks_proxy)");
    Ok(())
}

pub fn system_proxy_disable() -> Result<()> {
    if running_as_root() {
        return Ok(());
    }
    let _ = run("gsettings", &["set", "org.gnome.system.proxy", "mode", "'none'"]);
    for k in ["http_proxy", "https_proxy", "HTTP_PROXY", "HTTPS_PROXY", "SOCKS_PROXY", "ALL_PROXY"] {
        std::env::remove_var(k);
    }
    if let Some(home) = dirs::home_dir() {
        let _ = std::fs::remove_file(home.join(".config/environment.d/unblocker.conf"));
    }
    info!("system proxy disabled");
    Ok(())
}

pub fn hosts_apply(overrides: &[crate::config::HostOverride]) -> Result<()> {
    // Always start by stripping any prior block. Even if `overrides` is
    // non-empty, the on-disk block may be from a previous config that
    // had different (or stale) entries. Stale entries silently break
    // Instagram/Facebook/etc. because the kernel resolves the domain
    // to the bad IP BEFORE nfqws gets a chance to desync the SNI.
    let _ = hosts_remove();
    if overrides.is_empty() {
        info!("hosts overrides: none (stale block stripped if present)");
        return Ok(());
    }
    // Fix nsswitch.conf: ensure /etc/hosts ("files") is checked BEFORE
    // systemd-resolved ("resolve"). Without this, resolved answers first
    // via upstream DNS and the [!UNAVAIL=return] action prevents
    // /etc/hosts from ever being consulted.
    fix_nsswitch_for_hosts();
    let mut block = String::from(format!("\n{MARKER_BEGIN}\n").as_str());
    for h in overrides {
        for ip in &h.ips {
            block.push_str(&format!("{ip} {}\n", h.domain));
        }
    }
    block.push_str(&format!("{MARKER_END}\n"));
    let hosts = PathBuf::from("/etc/hosts");
    let content = std::fs::read_to_string(&hosts).unwrap_or_default();
    if content.contains(MARKER_BEGIN) && content.contains(&block.trim().to_string()) {
        info!("hosts already up to date");
        return Ok(());
    }
    let cleaned = strip_marker(&content);
    let new_content = format!("{cleaned}{block}");
    write_hosts(&new_content)?;
    info!("hosts overrides applied ({} entries)", overrides.len());
    Ok(())
}

/// Load static host files (telegram-web.txt) from a
/// config directory and return them as HostOverride entries.
/// These are always-on: Telegram Web + community AI/service unblock.
pub fn load_static_hosts(config_hosts_dir: &Path) -> Vec<crate::config::HostOverride> {
    let mut overrides = Vec::new();
    let mut seen_domains: HashSet<String> = HashSet::new();

    let files = ["telegram-web.txt"];
    for fname in &files {
        let path = config_hosts_dir.join(fname);
        if !path.exists() {
            warn!("static hosts file not found: {}", path.display());
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                warn!("failed to read {}: {e}", path.display());
                continue;
            }
        };
        let mut count = 0usize;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let ip = match parts.next() {
                Some(p) => p.to_string(),
                None => continue,
            };
            let domain = match parts.next() {
                Some(d) => d.to_string(),
                None => continue,
            };
            if seen_domains.insert(domain.clone()) {
                overrides.push(crate::config::HostOverride {
                    domain,
                    ips: vec![ip],
                });
                count += 1;
            }
        }
        info!("loaded {} entries from {}", count, path.display());
    }
    info!("total static host entries: {}", overrides.len());
    overrides
}

pub fn hosts_remove() -> Result<()> {
    let hosts = PathBuf::from("/etc/hosts");
    let content = std::fs::read_to_string(&hosts).unwrap_or_default();
    if !content.contains(MARKER_BEGIN) {
        return Ok(());
    }
    let cleaned = strip_marker(&content);
    // write_hosts uses pkexec cp which prompts for password — correct
    // behavior: user must approve modifying /etc/hosts.
    write_hosts(&cleaned)?;
    // Restore nsswitch.conf if we modified it
    restore_nsswitch();
    info!("hosts overrides removed");
    Ok(())
}

fn strip_marker(s: &str) -> String {
    if let Some(start) = s.find(MARKER_BEGIN) {
        if let Some(end) = s.find(MARKER_END) {
            let before = &s[..start];
            let after = &s[end + MARKER_END.len()..];
            return format!("{}{}", before.trim_end(), after);
        }
    }
    s.to_string()
}

fn write_hosts(content: &str) -> Result<()> {
    let tmp = "/tmp/unblocker.hosts.new";
    std::fs::write(tmp, content)?;
    let out = run_root("cp", &[tmp, "/etc/hosts"])?;
    if !out.status.success() {
        warn!("hosts write via pkexec failed: {:?}", String::from_utf8_lossy(&out.stderr));
        // fallback: try direct if we are root
        std::fs::write("/etc/hosts", content)?;
    }
    let _ = std::fs::remove_file(tmp);
    Ok(())
}

/// Fix /etc/nsswitch.conf so /etc/hosts ("files") is checked BEFORE
/// systemd-resolved ("resolve"). The default Arch order:
///   hosts: mymachines resolve [!UNAVAIL=return] files myhostname dns
/// causes resolved to answer first via upstream DNS, and the
/// [!UNAVAIL=return] action prevents /etc/hosts from ever being
/// consulted — our hosts entries are silently ignored.
const NSSWITCH_PATH: &str = "/etc/nsswitch.conf";
const NSSWITCH_BACKUP: &str = "/etc/nsswitch.conf.unblocker-backup";

fn fix_nsswitch_for_hosts() {
    let content = match std::fs::read_to_string(NSSWITCH_PATH) {
        Ok(c) => c,
        Err(e) => {
            warn!("cannot read {}: {e}", NSSWITCH_PATH);
            return;
        }
    };
    // Only fix if resolve comes before files
    let hosts_line = content.lines().find(|l| l.starts_with("hosts:"));
    let Some(line) = hosts_line else { return };
    let has_resolve_before_files = line.find("resolve").map(|r| {
        line.find("files").map(|f| r < f).unwrap_or(false)
    }).unwrap_or(false);
    if !has_resolve_before_files {
        return; // Already correct or no resolve module
    }
    // Backup original via pkexec (nsswitch.conf is root-owned)
    if !std::path::Path::new(NSSWITCH_BACKUP).exists() {
        let tmp = "/tmp/unblocker.nsswitch.bak";
        let out = run_root("cp", &[NSSWITCH_PATH, tmp]);
        if out.map(|o| o.status.success()).unwrap_or(false) {
            let _ = std::fs::copy(tmp, NSSWITCH_BACKUP);
            let _ = std::fs::remove_file(tmp);
        }
    }
    // Swap: put files before resolve
    let fixed = content.replacen(
        "resolve [!UNAVAIL=return] files",
        "files resolve [!UNAVAIL=return]",
        1,
    );
    if fixed != content {
        // Write via pkexec
        let tmp = "/tmp/unblocker.nsswitch.new";
        let _ = std::fs::write(tmp, &fixed);
        let out = run_root("cp", &[tmp, NSSWITCH_PATH]);
        let _ = std::fs::remove_file(tmp);
        if out.map(|o| o.status.success()).unwrap_or(false) {
            info!("nsswitch.conf: fixed hosts order (files before resolve)");
            // Flush systemd-resolved cache so it picks up the change
            let _ = std::process::Command::new("resolvectl").args(["flush-caches"]).output();
        } else {
            warn!("nsswitch.conf fix failed (pkexec denied?) — hosts entries may be ignored by browser");
        }
    }
}

fn restore_nsswitch() {
    if !std::path::Path::new(NSSWITCH_BACKUP).exists() {
        return;
    }
    let tmp = "/tmp/unblocker.nsswitch.restore";
    let out = run_root("cp", &[NSSWITCH_BACKUP, tmp]);
    if out.map(|o| o.status.success()).unwrap_or(false) {
        let out2 = run_root("cp", &[tmp, NSSWITCH_PATH]);
        let _ = std::fs::remove_file(tmp);
        if out2.map(|o| o.status.success()).unwrap_or(false) {
            let _ = std::fs::remove_file(NSSWITCH_BACKUP);
            info!("nsswitch.conf: restored original");
            let _ = std::process::Command::new("resolvectl").args(["flush-caches"]).output();
        }
    }
}

pub fn iptables_enable(port: u16) -> Result<()> {
    // Guard: only add REDIRECT if byedpi is actually listening on the port.
    let probe = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(500),
    );
    if probe.is_err() {
        warn!("byedpi not listening on 127.0.0.1:{} — skipping REDIRECT to avoid brick", port);
        return Ok(());
    }

    let uid = nix_uid();
    let rule = format!(
        "-A OUTPUT -p tcp -m multiport --dports 80,443 -m owner ! --uid-owner {uid} -j REDIRECT --to-ports {port}"
    );
    let check = run("iptables", &["-t", "nat", "-C", "OUTPUT",
        "-p", "tcp", "-m", "multiport", "--dports", "80,443",
        "-m", "owner", "!", "--uid-owner", &uid.to_string(),
        "-j", "REDIRECT", "--to-ports", &port.to_string()]);
    if !check.as_ref().map(|o| o.status.success()).unwrap_or(false) {
        let res = run_root("iptables", &["-t", "nat", &rule]);
        if !res.as_ref().map(|o| o.status.success()).unwrap_or(false) {
            warn!("iptables REDIRECT failed (no root) — falling back to SOCKS only");
            return Ok(());
        }
    }
    info!(uid, %port, "iptables REDIRECT active (system-wide)");
    Ok(())
}

pub fn iptables_disable(port: u16) -> Result<()> {
    let uid = nix_uid();
    let _ = run("iptables", &["-t", "nat", "-D", "OUTPUT",
        "-p", "tcp", "-m", "multiport", "--dports", "80,443",
        "-m", "owner", "!", "--uid-owner", &uid.to_string(),
        "-j", "REDIRECT", "--to-ports", &port.to_string()]);
    let _ = run_root("iptables", &["-t", "nat", "-D", "OUTPUT",
        "-p", "tcp", "-m", "multiport", "--dports", "80,443",
        "-m", "owner", "!", "--uid-owner", &uid.to_string(),
        "-j", "REDIRECT", "--to-ports", &port.to_string()]);
    info!("iptables REDIRECT removed");
    Ok(())
}

/// Like iptables_enable but for a specific target UID (used when running
/// as root under systemd: the bypass targets the original desktop user).
pub fn iptables_enable_for_uid(port: u16, uid: u32) -> Result<()> {
    // No probe guard here: this is called by the systemd daemon which
    // spawned byedpi itself — byedpi is guaranteed alive. The previous
    // TCP-connect probe was a leftover from the unprivileged path and
    // fails against byedpi's --transparent mode (which doesn't accept
    // raw connections without SO_ORIGINAL_DST).
    let _ = run("iptables", &["-t", "nat", "-D", "OUTPUT",
        "-p", "tcp", "-m", "multiport", "--dports", "80,443",
        "-m", "owner", "--uid-owner", &uid.to_string(),
        "-j", "REDIRECT", "--to-ports", &port.to_string()]);
    let res = run("iptables", &["-t", "nat", "-A", "OUTPUT",
        "-p", "tcp", "-m", "multiport", "--dports", "80,443",
        "-m", "owner", "--uid-owner", &uid.to_string(),
        "-j", "REDIRECT", "--to-ports", &port.to_string()]);
    if res.as_ref().map(|o| o.status.success()).unwrap_or(false) {
        info!(uid, %port, "iptables REDIRECT active for uid (FROM uid)");
    } else {
        warn!("iptables REDIRECT add failed: {:?}", String::from_utf8_lossy(&res.as_ref().map(|o| &o.stderr).unwrap_or(&Vec::new())));
    }
    Ok(())
}

pub fn iptables_disable_for_uid(port: u16, uid: u32) -> Result<()> {
    // Must match the exact predicate used in iptables_enable_for_uid.
    let _ = run("iptables", &["-t", "nat", "-D", "OUTPUT",
        "-p", "tcp", "-m", "multiport", "--dports", "80,443",
        "-m", "owner", "--uid-owner", &uid.to_string(),
        "-j", "REDIRECT", "--to-ports", &port.to_string()]);
    Ok(())
}

pub fn clear_state_inline() -> Result<()> {
    clear_state();
    Ok(())
}

/// SAFETY NET: flush the entire nat OUTPUT chain. Used on shutdown to
/// guarantee that no REDIRECT rule survives even if the targeted -D
/// failed. Running as root under systemd, this always succeeds.
pub fn iptables_flush_output() -> Result<()> {
    let _ = run("iptables", &["-t", "nat", "-F", "OUTPUT"]);
    Ok(())
}

fn nix_uid() -> u32 {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1000)
}

fn running_as_root() -> bool {
    // libc is a core dependency; geteuid == 0 means the systemd daemon.
    unsafe { libc::geteuid() == 0 }
}

pub fn doh_enable(_primary: &str, _ru: &str) -> Result<()> {
    warn!("DoH drop-in disabled by default (previous version wrote DNS=URL which breaks resolved). System DoH should be configured via browser/OS settings or sing-box dns; skipping /etc/systemd write.");
    Ok(())
}

pub fn doh_disable() -> Result<()> {
    Ok(())
}

pub fn write_state(port: u16, profile: &str) -> Result<()> {
    let s = serde_json::json!({"port": port, "profile": profile, "pid": std::process::id()});
    let path = state_file();
    std::fs::write(&path, serde_json::to_string_pretty(&s)?)?;
    // chmod 0666 so the user-owned GUI disable path can delete it.
    // write_state may run as root (CLI via pkexec) leaving a root-owned
    // file that the unprivileged disable() cannot remove.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666));
    }
    Ok(())
}
pub fn read_state() -> Option<serde_json::Value> {
    std::fs::read_to_string(state_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}
pub fn clear_state() {
    let p = state_file();
    if std::fs::remove_file(&p).is_ok() {
        return;
    }
    // Direct delete failed (e.g. root-owned file from CLI pkexec run).
    // Try pkexec rm to clean up.
    let _ = run_root("rm", &["-f", p.to_string_lossy().as_ref()]);
}

pub fn cleanup_stale() {
    let _ = run_root("rm", &["-f", "/etc/systemd/resolved.conf.d/unblocker.conf"]);
}

pub fn hosts_preview(cfg: &AppConfig) -> String {
    let dns = crate::dns::DnsConfig { primary: cfg.doh_primary.clone(), fallback: cfg.doh_fallback.clone(), ru: cfg.doh_ru.clone(), hosts: cfg.hosts_overrides.clone() };
    dns.hosts_file_content()
}

async fn find_byedpi_bin(cfg: &AppConfig) -> String {
    // cargo run exe is at target/debug/unblocker -> needs ../../vendor
    if let Ok(exe) = std::env::current_exe() {
        let exe_dir = exe.parent().unwrap_or(std::path::Path::new("."));
        for rel in ["vendor/byedpi/ciadpi", "../vendor/byedpi/ciadpi", "../../vendor/byedpi/ciadpi", "../../../vendor/byedpi/ciadpi"] {
            let p = exe_dir.join(rel);
            if p.exists() { return p.to_string_lossy().into_owned(); }
        }
    }
    for cand in [
        cfg.byedpi_bin.clone(),
        "vendor/byedpi/ciadpi".into(),
        "/usr/local/bin/ciadpi".into(),
        "/usr/bin/ciadpi".into(),
        "/usr/bin/byedpi".into(),
    ] {
        if std::path::Path::new(&cand).exists() { return cand; }
    }
    cfg.byedpi_bin.clone()
}

pub async fn enable_all(cfg: &AppConfig) -> Result<Option<BypassHandle>> {
    let _ = run_root("rm", &["-f", "/etc/systemd/resolved.conf.d/unblocker.conf"]);
    let port = cfg.socks_port;
    let bin = find_byedpi_bin(cfg).await;
    let has_bin = std::path::Path::new(&bin).exists();
    let (profile, should_save) = if cfg.profile.trim().is_empty() || cfg.profile == "auto" {
        if has_bin {
            let det = "--disorder 1 --tlsrec 1+s --conn-ip 0.0.0.0".to_string();
            (det, true)
        } else { (cfg.profile.clone(), false) }
    } else { (cfg.profile.clone(), false) };
    let save_profile = profile.clone();
    let handle = if has_bin {
        let bc = BypassConfig { bin: bin.clone(), socks_addr: cfg.socks_addr.clone(), socks_port: port, args: profile.clone(), transparent: true };
        match BypassHandle::spawn(bc).await {
            Ok(h) => {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let probe = std::net::TcpStream::connect_timeout(
                    &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                    std::time::Duration::from_secs(2),
                );
                if probe.is_ok() { info!(%profile, "byedpi listening on 127.0.0.1:{}", port); }
                else { warn!("byedpi not reachable on port {} — REDIRECT will be skipped", port); }
                Some(h)
            },
            Err(e) => { warn!(%e, "byedpi start failed"); None }
        }
    } else {
        warn!(%bin, "byedpi not found");
        None
    };
    let _ = system_proxy_enable(port);
    // Merge config overrides + static hosts (Telegram Web + AI unblock).
    let mut all_overrides = cfg.hosts_overrides.clone();
    let static_dirs = [
        std::path::PathBuf::from("config/hosts"),
        std::path::PathBuf::from("/etc/unblocker/hostlists"),
    ];
    for dir in &static_dirs {
        if dir.exists() {
            let static_entries = load_static_hosts(dir);
            for entry in static_entries {
                if !all_overrides.iter().any(|o| o.domain == entry.domain) {
                    all_overrides.push(entry);
                }
            }
            break;
        }
    }
    let _ = hosts_apply(&all_overrides);
    let _ = iptables_enable(port);  // adds REDIRECT only if byedpi is alive
    let _ = doh_enable(&cfg.doh_primary, &cfg.doh_ru);
    let _ = write_state(port, &profile);
    if should_save {
        let mut new_cfg = cfg.clone();
        new_cfg.profile = save_profile.clone();
        let _ = crate::config::save(&new_cfg);
        info!(%save_profile, "auto-tuned profile saved");
    }
    // Spawn watchdog: re-check byedpi liveness every 5s; if dead, remove REDIRECT immediately.
    spawn_watchdog(port, bin.clone());
    Ok(handle)
}

fn spawn_watchdog(port: u16, _bin: String) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        if !state_file().exists() { break; }
        let alive = std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            std::time::Duration::from_millis(500),
        ).is_ok();
        if !alive {
            // byedpi died — remove REDIRECT to avoid brick
            warn!("watchdog: byedpi dead on port {} — removing REDIRECT to prevent brick", port);
            let _ = iptables_disable(port);
        }
    });
}

pub fn disable_all(port: u16) -> Result<()> {
    // Order matters: remove iptables REDIRECT FIRST so even if byedpi is dead,
    // traffic can still flow direct (no black hole).
    let _ = iptables_disable(port);
    let _ = system_proxy_disable();
    let _ = hosts_remove();
    let _ = doh_disable();
    clear_state();
    let _ = run("pkill", &["-f", "ciadpi"]);
    info!("all bypass disabled");
    Ok(())
}

pub fn is_enabled() -> bool {
    state_file().exists()
}
