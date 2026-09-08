use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::process::Command;
use tracing_subscriber::EnvFilter;
use unblocker_core::{
    bypass::ensure_byedpi,
    config::{load_or_create, save},
    dns::DnsConfig,
    system,
    telegram,
    tuner::{check_site, default_targets, quick_tune},
    zapret,
};

#[derive(Parser)]
#[command(name="unblocker", version, about="One-toggle DPI + IP-block bypass (Linux Arch MVP, Flutter-ready)")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start daemon: nfqws + nftables + health (runs as root under systemd)
    Daemon {
        #[arg(long, default_value = "127.0.0.1:1080")]
        socks: String,
    },
    /// Enable system-wide (gsettings proxy + hosts + nft + nfqws) — one tap, all apps
    Enable,
    /// Disable system-wide bypass
    Disable,
    /// Check if a URL is reachable directly vs via SOCKS
    Probe {
        url: String,
        #[arg(long)]
        socks: Option<String>,
    },
    /// Auto-tune: score DPI candidates and print best profile
    Tune {
        #[arg(long)]
        quick: bool,
    },
    /// Print current config and hosts map
    Status,
    /// Print hosts file that would be applied
    Hosts,
    /// One-shot install-time IP discovery: probe DoH + TCP+TLS for every
    /// IP-blocked service, write the working IPs to /etc/hosts. Run as
    /// root once during install. Idempotent: re-running keeps the
    /// currently-working IPs.
    InstallDiscover,
    /// Refresh one host's IP via on-demand DoH + TLS probing, then write
    /// the working IP to /etc/hosts. Use when a site breaks (e.g. ISP
    /// started blocking the IP we had cached). No background polling —
    /// the user runs this command explicitly.
    ///
    /// Examples:
    ///   unblocker refresh web.whatsapp.com
    ///   unblocker refresh scontent-hel3-1.cdninstagram.com
    ///   unblocker refresh --all
    Refresh {
        /// Host to refresh. With `--all`, every tracked host is refreshed.
        host: Option<String>,
        /// Refresh every tracked host (skips WSS-only sites like Telegram).
        #[arg(long)]
        all: bool,
    },
    /// Run diagnostics: test DNS, TCP, HTTP through the VPN chain.
    /// Outputs JSON to stdout. Use with emulator port forwarding.
    Diag,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let cli = Cli::parse();
    let cfg = load_or_create()?;
    match cli.cmd {
        Some(Cmd::Daemon { socks }) => run_daemon(cfg, socks).await,
        Some(Cmd::Enable) => run_enable(cfg).await,
        Some(Cmd::Disable) => run_disable(cfg).await,
        None => run_enable(cfg).await,
        Some(Cmd::Probe { url, socks }) => run_probe(url, socks, cfg).await,
        Some(Cmd::Tune { .. }) => run_tune(cfg).await,
        Some(Cmd::Status) => run_status(cfg).await,
        Some(Cmd::Hosts) => run_hosts(cfg).await,
        Some(Cmd::InstallDiscover) => run_install_discover(cfg).await,
        Some(Cmd::Refresh { host, all }) => run_refresh(cfg, host, all).await,
        Some(Cmd::Diag) => run_diag(),
    }
}

async fn project_root() -> std::path::PathBuf {
    // crates/unblocker -> workspace root
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

async fn run_daemon(cfg: unblocker_core::config::AppConfig, _socks_arg: String) -> Result<()> {
    // STEP 0: hard safety net. We are root. Always start from a clean
    // nftables state so a previous crashed daemon can't have left a stale
    // rule that would black-hole the user. This is the same invariant
    // the user demanded: the disable path must be a true no-op on the
    // network, so we never get into a "stale rule + no engine" state.
    zapret::nft_flush_safely();
    // Also clean any leftover probe tables from install-discover.
    zapret::probe::nft_flush_probe_tables();

    let port = cfg.socks_port;
    let root = project_root().await;
    let nfqws = zapret::find_nfqws(&root)?;
    let strategy_path = zapret::default_strategy_path();
    let strategy = zapret::load_strategy_file(&strategy_path)
        .unwrap_or_else(|e| { tracing::warn!("strategy load: {e}"); vec![] });

    // Target UID = the desktop user we're intercepting traffic from.
    // systemd's PKEXEC_UID/SUDO_UID may not be set (we run as root
    // directly under systemd). Default to 1000 (most common single-user
    // desktop on Arch).
    let target_uid = std::env::var("PKEXEC_UID")
        .ok()
        .or_else(|| std::env::var("SUDO_UID").ok())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1000);

    println!("Unblocker daemon — system-wide enable");
    println!("  nfqws:      {}", nfqws.display());
    println!("  strategy:   {}", strategy_path.display());
    println!("  target uid: {target_uid}");

    // STEP 1: spawn nfqws FIRST. nfqws binds NFQUEUE 200. If we set up
    // the nft rule first and nfqws fails to start, the user is black-holed
    // (v2 brick). Always: engine first, then routing.
    let mut child = zapret::spawn_engine(&nfqws, &strategy, target_uid)
        .map_err(|e| { zapret::nft_flush_safely(); e })?;
    // Give nfqws a moment to bind NFQUEUE.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // STEP 2: install nft rule. If this fails, kill nfqws and flush.
    if let Err(e) = zapret::nft_install(target_uid, 200) {
        let _ = child.kill();
        zapret::nft_flush_safely();
        return Err(e);
    }

    // STEP 3: gsettings SOCKS for gsettings-aware apps (GNOME/KDE), and
    // /etc/hosts overrides for any IP-direct sites (Phase 2).
    let _ = system::system_proxy_enable(port);
    // Merge config overrides + static hosts (Telegram Web + AI unblock).
    let mut all_overrides = cfg.hosts_overrides.clone();
    let static_dirs = [
        std::path::PathBuf::from("config/hosts"),
        std::path::PathBuf::from("/etc/unblocker/hostlists"),
    ];
    for dir in &static_dirs {
        if dir.exists() {
            let static_entries = system::load_static_hosts(dir);
            for entry in static_entries {
                if !all_overrides.iter().any(|o| o.domain == entry.domain) {
                    all_overrides.push(entry);
                }
            }
            break;
        }
    }
    let _ = system::hosts_apply(&all_overrides);
    let _ = system::write_state(port, &cfg.profile);

    // STEP 4: start Telegram WSS proxy + nft redirect for ALL Telegram traffic.
    // The tg-ws-proxy listens on localhost:1443 (configurable) and bridges
    // Telegram MTProto traffic through Cloudflare-hosted WebSocket tunnels.
    // nft nat rules redirect ALL Telegram TCP (149.154.x.x, 91.108.x.x) to
    // the proxy port — zero user setup required.
    let tg_port: u16 = std::env::var("TG_PROXY_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1444);

    // Stop the standalone tg-ws-proxy service if it's running (it conflicts
    // with our built-in proxy). This is the package-installed standalone app.
    let _ = Command::new("systemctl")
        .args(["stop", "tg-ws-proxy.service"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = Command::new("systemctl")
        .args(["disable", "tg-ws-proxy.service"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = Command::new("systemctl")
        .args(["mask", "tg-ws-proxy.service"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    // Also kill any lingering standalone proxy process on port 1443.
    let _ = Command::new("fuser")
        .args(["-k", "1443/tcp"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = Command::new("pkill")
        .args(["-f", "tg-ws-proxy"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    // Use a PERSISTENT secret stored at /etc/unblocker/tg-proxy.secret.
    // This way re-enabling doesn't create a new proxy entry in Telegram Desktop.
    let tg_secret = std::env::var("TG_PROXY_SECRET")
        .ok()
        .unwrap_or_else(|| {
            let secret_path = std::path::Path::new("/etc/unblocker/tg-proxy.secret");
            if let Ok(existing) = std::fs::read_to_string(secret_path) {
                let s = existing.trim().to_string();
                if !s.is_empty() && s.len() == 32 {
                    return s;
                }
            }
            // Generate a new 32-char hex secret and persist it.
            use std::io::Read;
            let mut bytes = [0u8; 16];
            let _ = std::fs::File::open("/dev/urandom")
                .and_then(|mut f| f.read_exact(&mut bytes));
            let s: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
            let _ = std::fs::create_dir_all("/etc/unblocker");
            let _ = std::fs::write(secret_path, &s);
            s
        });
    let tg_url = format!(
        "tg://proxy?server=127.0.0.1&port={tg_port}&secret=dd{tg_secret}"
    );

    // Write URL for the GUI's "Setup Telegram" button to read.
    let _ = std::fs::create_dir_all("/etc/unblocker");
    let _ = std::fs::write("/etc/unblocker/tg-proxy.url", &tg_url);

    // Start the native Rust tg-ws-proxy (replaces Python).
    // First: kill any stale proxy on the tg port (from a previous crashed daemon
    // or the standalone tg-ws-proxy binary / Python).
    let _ = std::process::Command::new("fuser")
        .args(["-k", &format!("{tg_port}/tcp")])
        .output();
    let _ = std::process::Command::new("pkill")
        .args(["-f", "tg-ws-proxy"])
        .output();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Cache dir: XDG_CACHE_HOME/unblocker or ~/.cache/unblocker or /var/cache/unblocker
    let tg_cache_dir = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!("{}/unblocker", s.trim_end_matches('/')))
        .or_else(|| dirs::home_dir().map(|p| p.join(".cache/unblocker").to_string_lossy().to_string()))
        .unwrap_or_else(|| "/var/cache/unblocker".to_string());

    let mut dc_map = std::collections::HashMap::new();
    dc_map.insert(2, "149.154.167.220".to_string());
    dc_map.insert(4, "149.154.167.220".to_string());

    let tg_proxy_native = match telegram::start("127.0.0.1", tg_port, &tg_secret, dc_map, Some(tg_cache_dir)).await {
        Ok((cancel, handle)) => {
            println!("TG proxy (Rust): listening on 127.0.0.1:{tg_port}");
            if let Err(e) = zapret::nft_install_tg_redirect(tg_port) {
                tracing::warn!("TG nft redirect failed: {e}");
                println!("TG nft redirect: FAILED ({e})");
            } else {
                println!("TG nft redirect: active (Telegram traffic → proxy)");
            }
            Some((cancel, handle))
        }
        Err(e) => {
            tracing::warn!("TG proxy failed to start: {e}");
            println!("TG proxy: FAILED ({e}). Telegram will work via DPI bypass only.");
            None
        }
    };

    // STEP 5: self-tuning health loop. v5b rewrite deferred until we
    // can prove a probe that goes through the actual nft+nfqws path
    // (i.e. runs as the desktop user, not as root). For now, the
    // runtime uses the proven default strat; if it ever stops working
    // the user re-runs `unblocker install-discover` which picks a
    // new strat and rewrites the symlink. We do NOT auto-rotate strats
    // on a broken root probe because that probe is meaningless and
    // causes log noise + wrong strat swaps.
    let _ = strategy_path;

    println!("✓ Enabled — all apps bypass DPI + TG proxy active");
    println!("  Telegram proxy: 127.0.0.1:{tg_port} secret={tg_secret}");
    // Save the TG proxy URL for the GUI's "Setup Telegram" button.
    let _ = std::fs::write("/etc/unblocker/tg-proxy.url", &tg_url);

    // STEP 5: keep alive until SIGTERM (systemd stop). On shutdown:
    //   kill nfqws FIRST (so it's not processing the nft rule we delete
    //   underneath it), then delete the rule, then atomic-flush the
    //   table as the safety net.
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
    println!("\nShutting down...");
    // Stop TG proxy first (cancel token + abort task).
    if let Some((cancel, handle)) = tg_proxy_native {
        cancel.cancel();
        handle.abort();
        // give it a moment to close listeners/pools
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    // Then stop nfqws (so it's not processing the nft rule we delete underneath it).
    let _ = child.kill();
    zapret::nft_remove().ok();
    zapret::nft_flush_safely();
    let _ = system::system_proxy_disable();
    let _ = system::hosts_remove();
    let _ = system::clear_state_inline();
    Ok(())
}

async fn run_probe(url: String, socks: Option<String>, cfg: unblocker_core::config::AppConfig) -> Result<()> {
    let socks_url = socks.unwrap_or_else(|| format!("socks5://{}:{}", cfg.socks_addr, cfg.socks_port));
    // try direct vs via socks
    let (direct, via) = check_site(&url, Some(&socks_url)).await;
    println!("Direct:  {}  ok={}  status={:?}  {}ms", direct.host, direct.ok, direct.status, direct.ms);
    if let Some(v) = via {
        println!("Via SOCKS ({}): ok={} status={:?} {}ms", socks_url, v.ok, v.status, v.ms);
        if !direct.ok && v.ok {
            println!("→ Bypass works (direct blocked, proxied ok)");
        } else if direct.ok && v.ok {
            println!("→ Both ok (no block or already handled)");
        } else if !v.ok {
            println!("→ Still blocked via proxy — try: unblocker tune");
        }
    }
    Ok(())
}

async fn run_tune(cfg: unblocker_core::config::AppConfig) -> Result<()> {
    let root = project_root().await;
    let bin = ensure_byedpi(&root).await?;
    if tokio::fs::metadata(&bin).await.is_err() {
        eprintln!("byedpi not found at {bin}. Cannot tune without it. Build vendor/byedpi first.");
        eprintln!("  git submodule add https://github.com/hufrea/byedpi vendor/byedpi");
        eprintln!("  make -C vendor/byedpi -j$(nproc)");
        std::process::exit(2);
    }
    let targets = default_targets();
    println!("Tuning {bin} across {} targets (each candidate gets ephemeral SOCKS)…", targets.len());
    let report = quick_tune(&bin, cfg.socks_port, &targets).await?;
    println!("\nBest: {}", report.best_args);
    println!("\nScores (ok / {}):", targets.len());
    for (args, ok, ms) in &report.scores {
        println!("  {ok:>2}  {ms:>4}ms  {args}");
    }
    // offer to save
    let mut new_cfg = cfg.clone();
    new_cfg.profile = report.best_args.clone();
    new_cfg.byedpi_bin = bin;
    save(&new_cfg)?;
    println!("\n✓ Saved to {}", unblocker_core::config::config_path().display());
    Ok(())
}

async fn run_status(cfg: unblocker_core::config::AppConfig) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&cfg)?);
    Ok(())
}

async fn run_hosts(cfg: unblocker_core::config::AppConfig) -> Result<()> {
    let dns = DnsConfig {
        primary: cfg.doh_primary.clone(),
        fallback: cfg.doh_fallback.clone(),
        ru: cfg.doh_ru.clone(),
        hosts: cfg.hosts_overrides.clone(),
    };
    print!("{}", dns.hosts_file_content());
    eprintln!("\n# sing-box dns.rules preview:");
    eprintln!("{}", serde_json::to_string_pretty(&dns.singbox_dns_rules())?);
    Ok(())
}

async fn run_enable(_cfg: unblocker_core::config::AppConfig) -> Result<()> {
    println!("Enabling bypass…");
    let result = std::process::Command::new("systemctl")
        .args(["start", "unblocker.service"])
        .output()?;
    if result.status.success() {
        println!("✓ Enabled — nfqws NFQUEUE + nft + hosts applied");
    } else {
        let err = String::from_utf8_lossy(&result.stderr);
        anyhow::bail!("enable failed: {err}");
    }
    Ok(())
}

async fn run_disable(_cfg: unblocker_core::config::AppConfig) -> Result<()> {
    println!("Disabling bypass…");
    let result = std::process::Command::new("systemctl")
        .args(["stop", "unblocker.service"])
        .output()?;
    if result.status.success() {
        println!("✓ Disabled — nfqws stopped, nft flushed, hosts cleaned.");
    } else {
        let err = String::from_utf8_lossy(&result.stderr);
        // systemctl stop may fail if service wasn't running — that's OK
        if err.contains("not loaded") || err.contains("not active") {
            println!("✓ Disabled (service was not running).");
        } else {
            anyhow::bail!("disable failed: {err}");
        }
    }
    Ok(())
}

/// One-shot install-time discovery (v6): for each site, try every
/// strategy in the pool, fall back to an IP scan if none work.
/// Writes the result to /etc/unblocker/sites/<host>.conf so the
/// runtime nfqws invocation reads it.
async fn run_install_discover(mut cfg: unblocker_core::config::AppConfig) -> Result<()> {
    use unblocker_core::zapret::{probe, sites::{self, SiteKind, expand_ranges}};
    use unblocker_core::ip_discovery::{self, HostsEntry};
    use unblocker_core::config::HostOverride;
    use std::net::IpAddr;

    let sites = sites::all();
    let pool = discover_strategy_pool().await?;

    println!("→ install-discover: scanning {} sites against {} strategies",
             sites.len(), pool.len());

    // Step 0: seed the runtime config with MagilaWEB-derived CDN IPs
    // (/etc/unblocker/hostlists/magila-cdn.txt, installed by the package;
    // falls back to the repo copy when running from source). These are
    // edge IPs that the user's ISP DPI does NOT fingerprint, so IG/FB/WA
    // images and static assets load even when the default DNS-resolved
    // IPs are TLS-blocked. Source: MagilaWEB/unblock-youtube-discord.
    let root = project_root().await;
    let magila_candidates = [
        std::path::PathBuf::from("/etc/unblocker/hostlists/magila-cdn.txt"),
        root.join("config/hosts/magila-cdn.txt"),
    ];
    let magila_hosts = magila_candidates.iter().find(|p| p.exists()).cloned();
    if let Some(magila_hosts) = &magila_hosts {
        match std::fs::read_to_string(magila_hosts) {
            Ok(content) => {
                let mut new_overrides: Vec<HostOverride> = Vec::new();
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    let mut parts = trimmed.split_whitespace();
                    if let (Some(ip), Some(host)) = (parts.next(), parts.next()) {
                        if let Ok(_parsed) = ip.parse::<std::net::IpAddr>() {
                            new_overrides.push(HostOverride {
                                domain: host.to_string(),
                                ips: vec![ip.to_string()],
                            });
                        }
                    }
                }
                println!("→ MagilaWEB CDN hosts: applying {} verified CDN edge IPs", new_overrides.len());
                cfg.hosts_overrides = new_overrides;
                if let Err(e) = unblocker_core::config::save(&cfg) {
                    tracing::warn!("could not save config: {}", e);
                }
                // Write to /etc/hosts right now (handles case where the
                // runtime is already running and hasn't picked up the
                // new config yet).
                let _ = unblocker_core::system::hosts_apply(&cfg.hosts_overrides);
            }
            Err(e) => tracing::warn!("could not read {}: {}", magila_hosts.display(), e),
        }
    } else {
        println!("→ MagilaWEB CDN hosts: not found in /etc/unblocker/hostlists or dev tree (skipping)");
    }

    // Step 0b: merge GeoHide DNS community hosts. This is a community-vetted
    // file with ~5,000 entries covering 200+ blocked services (RKN, RU ISP
    // blocks). Source: Internet-Helper/GeoHideDNS. We fetch the full file
    // (1 HTTPS GET, 10 s timeout), filter to entries that match our target
    // hosts or any parent domain, and merge into the same hosts_overrides
    // list (MagilaWEB entries first, GeoHide as a fresh tier). On fetch
    // failure (offline, blocked, etc.) we silently keep the MagilaWEB-only
    // set — the runtime keeps working.
    {
        let gh = ip_discovery::geohide::fetch_geohide_hosts();
        if gh.is_empty() {
            println!("→ GeoHide DNS: fetch failed or empty, using MagilaWEB-only set");
        } else {
            // Build the list of host strings we care about: the user's
            // main sites + all known subdomains (cdninstagram.com,
            // scontent.cdninstagram.com, mmg.whatsapp.net, etc.).
            let mut target_hosts: Vec<String> = Vec::new();
            for s in &sites {
                target_hosts.push(s.host.to_string());
            }
            // Add common subdomains that aren't in `sites::all()` but we
            // need overrides for. The Geohide list has 5,000 entries; we
            // only apply the ones that match these specific subdomains.
            for h in [
                "cdninstagram.com", "scontent.cdninstagram.com",
                "static.cdninstagram.com", "graph.instagram.com",
                "i.instagram.com", "api.instagram.com",
                "fbcdn.net", "static.xx.fbcdn.net", "scontent.xx.fbcdn.net",
                "mmg.whatsapp.net", "g.whatsapp.net", "mmg-fna.whatsapp.net",
                "mm-fna.whatsapp.net", "pps.whatsapp.net",
                "web.whatsapp.com", "whatsapp.com", "faq.whatsapp.com",
                "t.me", "telegram.org",
            ] {
                target_hosts.push(h.to_string());
            }
            let target_refs: Vec<&str> = target_hosts.iter().map(|s| s.as_str()).collect();
            let filtered = ip_discovery::geohide::filter_to_targets(gh, &target_refs);
            let mut added = 0usize;
            for (host, ip) in filtered {
                if ip.parse::<std::net::IpAddr>().is_ok() {
                    // Only add if not already in the list (MagilaWEB
                    // first-write wins, GeoHide adds new domains).
                    if !cfg.hosts_overrides.iter().any(|h| h.domain == host) {
                        cfg.hosts_overrides.push(unblocker_core::config::HostOverride {
                            domain: host,
                            ips: vec![ip],
                        });
                        added += 1;
                    }
                }
            }
            println!("→ GeoHide DNS: merged {} new host entries (total now {})",
                     added, cfg.hosts_overrides.len());
            if let Err(e) = unblocker_core::config::save(&cfg) {
                tracing::warn!("could not save config: {}", e);
            }
            // Re-write /etc/hosts with the merged list.
            let _ = unblocker_core::system::hosts_apply(&cfg.hosts_overrides);
        }
    }

    let sites_dir = std::path::PathBuf::from("/etc/unblocker/sites");
    if let Err(e) = std::fs::create_dir_all(&sites_dir) {
        tracing::warn!("cannot create {}: {} (running as non-root?); skipping per-site config write",
                      sites_dir.display(), e);
    }

    for s in &sites {
        if s.kind == SiteKind::Wss {
            println!("  {:<22}  skipped (WSS — needs tg-ws-proxy, Phase 2c)", s.host);
            continue;
        }
        print!("  {:<22}  ", s.host);

        // Step 1: try every strategy in the pool (only for Dpi and Both)
        let mut working = None;
        if matches!(s.kind, SiteKind::Dpi | SiteKind::Both) {
            if let Ok(Some(strat)) = probe::find_working_strategy(&s.host, s.test_url, &pool).await {
                println!("DPI strat: {} works", strat.file_name().unwrap_or_default().to_string_lossy());
                working = Some(strat);
            } else {
                println!("DPI strats: no winner  ");
            }
        }
        // Step 2: if no strat works, try IP scan (only for Ip and Both)
        if working.is_none() && matches!(s.kind, SiteKind::Ip | SiteKind::Both) {
            let candidates: Vec<IpAddr> = expand_ranges(s.allowed_ip_ranges)
                .into_iter()
                .take(2048)  // cap per site to keep the scan bounded
                .collect();
            print!("  IP scan: probing {} candidates... ", candidates.len());
            match probe::scan_for_clean_ip(&s.host, s.test_url, &candidates).await {
                Ok(Some(ip)) => {
                    println!("found {} (clean CDN edge)", ip);
                    let entries = vec![HostsEntry { host: s.host.to_string(), ip }];
                    let _ = ip_discovery::add_to_hosts(&entries);
                }
                _ => println!("no clean IP found (ISP blocks the whole range)"),
            }
        } else if working.is_none() {
            println!("  (skipped IP scan — site is Dpi-only)");
        }
        // Step 3: record the winning strategy (if any) for runtime nfqws
        if let Some(strat) = working {
            if let Err(e) = write_per_site_config(&sites_dir, &s.host, &strat) {
                tracing::warn!(?e, "could not write per-site config for {}", s.host);
            }
        }
    }

    // Clean up ALL leftover probe tables from the scan.
    // A stale probe table with no queue listener silently drops any
    // matching traffic, black-holing the user.
    zapret::probe::nft_flush_probe_tables();
    println!("→ done. runtime nfqws reads /etc/unblocker/sites/*.conf on each start.");
    Ok(())
}

/// On-demand IP refresh for a single host (or all). v6.5:
///
/// 1. For the given host, build the candidate list
///    (precompiled + GeoHide + Google DoH + Cloudflare DoH).
/// 2. TCP+TLS-probe each candidate (SNI = host) until one answers.
/// 3. Remove any existing /etc/hosts entries for that host.
/// 4. Write the new IP to /etc/hosts.
/// 5. Print a one-line summary.
///
/// The runtime doesn't poll this. The user runs `unblocker refresh
/// <host>` only when they notice a site broken. This is "detection on
/// user action, not cooldown poll" per the v6.5 plan.
async fn run_refresh(
    mut cfg: unblocker_core::config::AppConfig,
    host: Option<String>,
    all: bool,
) -> Result<()> {
    use unblocker_core::ip_discovery::{self, geohide};
    use unblocker_core::zapret::sites::SiteKind;
    use std::collections::HashSet;

    if !all && host.is_none() {
        anyhow::bail!("usage: unblocker refresh <host> | unblocker refresh --all");
    }

    // Build the list of hosts to refresh.
    // Use zapret::sites::all() — the canonical site list with
    // SiteKind::Dpi/Ip/Both/Wss and curated IP ranges.
    let sites_list = unblocker_core::zapret::sites::all();
    let hosts: Vec<String> = if all {
        let mut out: Vec<String> = Vec::new();
        for s in &sites_list {
            if s.kind == SiteKind::Wss {
                continue; // Telegram: WSS tunnel is Phase 2c, skip.
            }
            out.push(s.host.to_string());
        }
        out
    } else {
        vec![host.unwrap()]
    };

    if hosts.is_empty() {
        println!("→ nothing to refresh (all sites are WSS-tunnel-only)");
        return Ok(());
    }

    // Build a set of hosts currently in the runtime config so we can
    // detect "new" hosts that were not there before.
    let mut known: HashSet<String> = cfg.hosts_overrides.iter()
        .map(|h| h.domain.clone())
        .collect();

    // Also pull the GeoHide list once so per-host refresh hits the
    // community-vetted entries. fetch is 1 HTTP GET, 10s timeout.
    println!("→ refresh: fetching GeoHide DNS community list (1 HTTPS GET)...");
    let gh = geohide::fetch_geohide_hosts();
    println!("→ refresh: GeoHide returned {} entries; {} hosts to refresh",
             gh.len(), hosts.len());

    for h in &hosts {
        // 1. Remove the old /etc/hosts entry for this host.
        if let Ok(entries) = ip_discovery::current_hosts_entries() {
            let keep: Vec<_> = entries.into_iter()
                .filter(|e| e.host != *h)
                .collect();
            if let Err(e) = ip_discovery::add_to_hosts(&keep) {
                tracing::warn!(host = %h, "could not strip old entry: {}", e);
            }
        }
        // Also strip from the in-memory config.
        cfg.hosts_overrides.retain(|o| o.domain != *h);

        // 2. Build candidates (precompiled + GeoHide filtered + DoH).
        //    We don't reuse discover_candidates_with_geohide here
        //    because we already fetched the GeoHide list above and
        //    don't want to fetch it again per host.
        let mut candidates: Vec<std::net::IpAddr> = Vec::new();
        for ip in ip_discovery::pool::precompiled_ips_for(h) {
            candidates.push(ip);
        }
        // Filter GeoHide to entries that match this host (or any
        // parent domain).
        let parents = {
            let mut p = vec![h.as_str()];
            let mut cur = h.as_str();
            while let Some(idx) = cur.find('.') {
                cur = &cur[idx + 1..];
                if cur.is_empty() { break; }
                p.push(cur);
            }
            p
        };
        for (_, ip_s) in geohide::filter_to_targets(gh.clone(), &parents) {
            if let Ok(ip) = ip_s.parse::<std::net::IpAddr>() {
                if !candidates.contains(&ip) {
                    candidates.push(ip);
                }
            }
        }
        for ip in ip_discovery::query_google_doh(h).unwrap_or_default() {
            if !candidates.contains(&ip) {
                candidates.push(ip);
            }
        }
        for ip in ip_discovery::query_cloudflare_doh(h).unwrap_or_default() {
            if !candidates.contains(&ip) {
                candidates.push(ip);
            }
        }

        // 3. Probe each candidate.
        let timeout = std::time::Duration::from_secs(4);
        let mut found: Option<std::net::IpAddr> = None;
        for ip in &candidates {
            if ip_discovery::probe_tcp_443(*ip, timeout)
                && ip_discovery::probe_tls(*ip, h, timeout)
            {
                found = Some(*ip);
                break;
            }
        }
        // 4. Last-ditch: try system DNS.
        if found.is_none() {
            if let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(&(h.as_str(), 443u16)) {
                for a in addrs {
                    if ip_discovery::probe_tcp_443(a.ip(), timeout)
                        && ip_discovery::probe_tls(a.ip(), h, timeout)
                    {
                        found = Some(a.ip());
                        break;
                    }
                }
            }
        }

        match found {
            Some(ip) => {
                cfg.hosts_overrides.push(unblocker_core::config::HostOverride {
                    domain: h.clone(),
                    ips: vec![ip.to_string()],
                });
                known.insert(h.clone());
                println!("  ✓ {} -> {} ({} candidates probed)", h, ip, candidates.len());
            }
            None => {
                println!("  ✗ {} -> NO WORKING IP ({} candidates probed, all failed)",
                         h, candidates.len());
            }
        }
    }

    // 5. Persist + apply to /etc/hosts.
    if let Err(e) = unblocker_core::config::save(&cfg) {
        tracing::warn!("could not save config: {}", e);
    }
    let _ = unblocker_core::system::hosts_apply(&cfg.hosts_overrides);
    println!("→ refresh: done. /etc/hosts and config updated.");
    Ok(())
}
/// The runtime's `/etc/unblocker/strategies/general-hostfakesplit.strat`
/// is prepended (when present) because it's the proven-working default
/// from the user's flatpak zapret — testing it first means we never
/// miss a known-good profile just because the bundled pool has some
/// other (possibly broken) entry.
async fn discover_strategy_pool() -> Result<Vec<std::path::PathBuf>> {
    use std::path::PathBuf;
    let mut out: Vec<PathBuf> = Vec::new();

    // 1. Runtime default (highest priority — known to work on this
    //    user's flatpak-derived config).
    let runtime_default = PathBuf::from("/etc/unblocker/strategies/general-hostfakesplit.strat");
    if runtime_default.exists() {
        out.push(runtime_default);
    }

    // 2. Bundled pool, then bundled byebyedpi subpool. Installed layout
    //    is /etc/unblocker/strategies/pool; dev tree falls back to
    //    config/zapret/strategies/pool relative to the workspace.
    let pool_dirs: Vec<PathBuf> = vec![
        PathBuf::from("/etc/unblocker/strategies/pool"),
        project_root().await.join("config/zapret/strategies/pool"),
    ];
    for dir in &pool_dirs {
        if let Ok(read) = std::fs::read_dir(dir) {
            for entry in read.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("strat") {
                    out.push(p);
                }
            }
        }
        let sub = dir.join("byebyedpi");
        if let Ok(read) = std::fs::read_dir(&sub) {
            for entry in read.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("strat") {
                    out.push(p);
                }
            }
        }
    }

    if out.is_empty() {
        anyhow::bail!("strategy pool is empty: expected files in {}", pool_dirs[0].display());
    }
    tracing::info!("strategy pool: {} candidates (runtime default first)", out.len());
    Ok(out)
}

/// Write the per-site config the runtime nfqws will read. One file per
/// host that has a winning strategy. Format is a single line: the path
/// of the strategy file. The runtime reads these and concatenates the
/// per-site `--hostlist=` lines.
fn write_per_site_config(
    dir: &std::path::Path,
    host: &str,
    strat: &std::path::Path,
) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("create dir {}", dir.display()))?;
    let target = dir.join(format!("{}.conf", host));
    let body = format!("# winning strategy for {} (written by install-discover)\n{}\n",
                      host, strat.display());
    std::fs::write(&target, body)
        .with_context(|| format!("write {}", target.display()))?;
    Ok(())
}

fn run_diag() -> Result<()> {
    // Run diagnostics on a separate thread to avoid tokio runtime deadlock
    // (run_diagnostics uses its own RUNTIME.block_on()).
    let handle = std::thread::spawn(|| {
        let result = unsafe {
            let ptr = unblocker_core::api::run_diagnostics();
            if ptr.is_null() {
                return Err(anyhow::anyhow!("run_diagnostics returned null"));
            }
            let s = std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned();
            unblocker_core::api::free_string(ptr);
            Ok(s)
        };
        result
    });

    match handle.join() {
        Ok(Ok(json)) => {
            println!("{json}");
            Ok(())
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(anyhow::anyhow!("diagnostics thread panicked")),
    }
}
