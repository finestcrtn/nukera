use std::time::Duration;

use anyhow::Result;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use super::{NetworkDriver, Platform};
use crate::config::AppConfig;

/// Linux: transparent bypass via nfqws (NFQUEUE) + nftables + `/etc/hosts`,
/// plus the in-process Rust TG proxy. This is the proven desktop path —
/// moved verbatim from `api.rs` and now living behind `NetworkDriver`.
pub struct Driver {
    nfqws: Option<std::process::Child>,
    tg: Option<(CancellationToken, JoinHandle<std::io::Result<()>>)>,
    port: u16,
}

impl Driver {
    pub(crate) fn new() -> Self {
        Self {
            nfqws: None,
            tg: None,
            port: 0,
        }
    }
}

fn project_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

impl NetworkDriver for Driver {
    fn enable(
        &mut self,
        rt: &tokio::runtime::Runtime,
        cfg: &AppConfig,
    ) -> Result<()> {
        rt.block_on(self.enable_inner(cfg))
    }

    fn disable(&mut self, rt: &tokio::runtime::Runtime) -> Result<()> {
        rt.block_on(self.disable_inner())
    }
}

impl Driver {
    pub async fn enable_inner(&mut self, cfg: &AppConfig) -> Result<()> {
        let port = cfg.socks_port;

        // Safety: flush any stale nft from previous runs
        crate::zapret::nft_flush_safely();
        crate::zapret::probe::nft_flush_probe_tables();

        let root = project_root();
        let nfqws = crate::zapret::find_nfqws(&root)?;
        let strategy_path = crate::zapret::default_strategy_path();
        let strategy = crate::zapret::load_strategy_file(&strategy_path).unwrap_or_else(|e| {
            warn!("strategy load: {e}");
            vec![]
        });

        let target_uid = std::env::var("PKEXEC_UID")
            .ok()
            .or_else(|| std::env::var("SUDO_UID").ok())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1000);

        let mut child = crate::zapret::spawn_engine(&nfqws, &strategy, target_uid)
            .map_err(|e| {
                crate::zapret::nft_flush_safely();
                e
            })?;
        tokio::time::sleep(Duration::from_millis(600)).await;

        let has_daemon = std::process::Command::new("pgrep")
            .args(["-f", "nfqws.*--qnum 200"])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);
        if !has_daemon {
            crate::zapret::nft_flush_safely();
            anyhow::bail!("nfqws daemon not found after spawn");
        }

        if let Err(e) = crate::zapret::nft_install(target_uid, 200) {
            let _ = child.kill();
            crate::zapret::nft_flush_safely();
            anyhow::bail!("nft_install: {}", e);
        }
        info!("zapret: nfqws queue 200 + nft rules installed (uid {})", target_uid);

        // Transparent bypass only — no system proxy/gsettings: nfqws desyncs
        // at the kernel level, hosts overrides IPs via /etc/hosts.
        // Merge config overrides + static hosts (Telegram Web + AI unblock).
        let mut all_overrides = cfg.hosts_overrides.clone();
        // Check both source path and installed path for static host files
        let source_hosts_dir = root.join("config/hosts");
        let installed_hosts_dir = std::path::PathBuf::from("/etc/unblocker/hostlists");
        let static_hosts_dir = if source_hosts_dir.exists() {
            source_hosts_dir
        } else {
            installed_hosts_dir
        };
        let static_entries = crate::system::load_static_hosts(&static_hosts_dir);
        for entry in static_entries {
            if !all_overrides.iter().any(|o| o.domain == entry.domain) {
                all_overrides.push(entry);
            }
        }
        let _ = crate::system::hosts_apply(&all_overrides);
        let _ = crate::system::write_state(port, &cfg.profile);
        info!(
            "zapret: hosts override ({} config + {} static = {} total) + state written",
            cfg.hosts_overrides.len(),
            all_overrides.len() - cfg.hosts_overrides.len(),
            all_overrides.len()
        );

        // TG proxy — Rust in-process, shared with Android.
        let tg_port: u16 = std::env::var("TG_PROXY_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1444);
        let (cancel, handle) = crate::api::start_tg().await?;
        if let Err(e) = crate::zapret::nft_install_tg_redirect(tg_port) {
            warn!("TG nft redirect failed: {e}");
        }

        self.nfqws = Some(child);
        self.tg = Some((cancel, handle));
        self.port = port;
        Ok(())
    }

    pub async fn disable_inner(&mut self) -> Result<()> {
        // Stop in-process TG first.
        if let Some((cancel, handle)) = self.tg.take() {
            cancel.cancel();
            handle.abort();
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        // nfqws spawned via file caps drops to UID 2147483647 (zapret
        // "nobody"). That process is NOT owned by the desktop user, so
        // child.kill() gets EPERM. Root-kill it FIRST via pkexec, then
        // kill+wait — otherwise wait() blocks forever on the uid-swapped
        // child.
        if let Some(mut child) = self.nfqws.take() {
            let _ = std::process::Command::new("pkexec")
                .args(["pkill", "-9", "-x", "nfqws"])
                .output();
            tokio::time::sleep(Duration::from_millis(300)).await;
            let _ = child.kill();
            let _ = child.wait();
        }

        // Safety net: root-kill any uid-swapped nfqws that escaped STATE
        // teardown (e.g. from a systemd daemon's --daemon fork). Use `-x
        // nfqws` (exact name), never `-f`: a -f pattern like
        // "nfqws.*--qnum" also matches the `pkexec pkill` argv itself and
        // the root pkill would kill itself mid-flight.
        let alive = std::process::Command::new("pgrep")
            .args(["-x", "nfqws"])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);
        if alive {
            warn!("nfqws survived STATE teardown — root-killing via pkexec");
            let _ = std::process::Command::new("pkexec")
                .args(["pkill", "-9", "-x", "nfqws"])
                .output();
            tokio::time::sleep(Duration::from_millis(300)).await;
        }

        // Hybrid mode: if zapret was enabled via the systemd daemon, stop it.
        let is_systemd_active = std::process::Command::new("systemctl")
            .args(["is-active", "unblocker.service"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
            .unwrap_or(false);
        if is_systemd_active {
            let _ = std::process::Command::new("systemctl")
                .args(["stop", "unblocker.service"])
                .output();
            for _ in 0..20 {
                tokio::time::sleep(Duration::from_millis(300)).await;
                let out = std::process::Command::new("systemctl")
                    .args(["is-active", "unblocker.service"])
                    .output()?;
                if String::from_utf8_lossy(&out.stdout).trim() != "active" {
                    break;
                }
            }
        } else {
            // FFI case: pkill fallback + nft cleanup.
            let _ = std::process::Command::new("pkill").args(["-9", "-x", "nfqws"]).output();
            crate::zapret::nft_remove().ok();
            crate::zapret::nft_flush_safely();
            crate::zapret::probe::nft_flush_probe_tables();
        }

        // Always clean proxy/hosts/state (both paths).
        let _ = crate::system::system_proxy_disable();
        let _ = crate::system::hosts_remove();
        let _ = crate::system::clear_state_inline();
        let _ = crate::system::iptables_flush_output();

        // Also ensure TG nat table cleaned.
        crate::zapret::nft_flush_safely();
        self.port = 0;
        Ok(())
    }
}

pub struct LinuxPlatform;
impl Platform for LinuxPlatform {
    fn name(&self) -> &'static str {
        "linux"
    }
    fn setup(&self) -> Result<()> {
        info!("Linux platform setup — SOCKS mode needs no root; TUN via sing-box when available");
        Ok(())
    }
    fn teardown(&self) -> Result<()> {
        info!("Linux platform teardown");
        Ok(())
    }
}

pub fn platform() -> Box<dyn Platform> {
    Box::new(LinuxPlatform)
}

pub fn driver() -> Driver {
    Driver::new()
}

pub fn has_cap_net_admin() -> bool {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/net/tun")
        .is_ok()
}