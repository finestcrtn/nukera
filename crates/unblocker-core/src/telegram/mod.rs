pub mod balancer;
pub mod cfproxy;
pub mod config;
pub mod crypto;
pub mod proxy;
pub mod ws;

pub use config::{
    CFPROXY, CFPROXY_429, CFPROXY_ENABLED, DC_DEFAULT_IPS, DC_FAIL_UNTIL, DC_OPT,
    DC_OVERRIDES, LOG_VERBOSE, POOL_SIZE, PROXY_SECRET, STATS, WS_BLACKLIST, ZERO64,
};
pub use proxy::{run_proxy, WsPool};

use tokio_util::sync::CancellationToken;
use std::collections::HashMap;
use tokio::net::TcpListener;

fn default_cache_dir() -> String {
    // Prefer XDG cache: $XDG_CACHE_HOME or ~/.cache/unblocker
    // For system daemon (root), fallback to /var/cache/unblocker so it survives reboot.
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.trim().is_empty() {
            return format!("{}/unblocker", xdg.trim_end_matches('/'));
        }
    }
    if let Some(home) = dirs::home_dir() {
        let p = home.join(".cache/unblocker");
        if let Some(s) = p.to_str() {
            return s.to_string();
        }
    }
    "/var/cache/unblocker".to_string()
}

pub async fn start(
    host: &str,
    port: u16,
    secret: &str,
    dc_map: HashMap<i32, String>,
    cache_dir: Option<String>,
) -> anyhow::Result<(CancellationToken, tokio::task::JoinHandle<std::io::Result<()>>)> {
    // Ensure rustls crypto provider is installed (required since rustls 0.23)
    let _ = rustls::crypto::ring::default_provider().install_default();
    {
        let mut s = config::PROXY_SECRET.write();
        *s = secret.to_string();
    }
    {
        let mut cfg = config::CFPROXY.write();
        // cache dir: explicit > default XDG > fallback. Always set if empty.
        let dir = cache_dir.unwrap_or_else(default_cache_dir);
        if cfg.cache_dir.trim().is_empty() {
            cfg.cache_dir = dir;
        } else if !dir.is_empty() && cfg.cache_dir != dir {
            // explicit override wins
            cfg.cache_dir = dir;
        }
        // migrate from old /etc/unblocker cache if present (backward compat)
        let old_path = std::path::PathBuf::from("/etc/unblocker").join(config::CFPROXY_CACHE_FILE_NAME);
        if old_path.exists() {
            let new_path = std::path::PathBuf::from(cfg.cache_dir.clone()).join(config::CFPROXY_CACHE_FILE_NAME);
            if !new_path.exists() {
                let _ = std::fs::create_dir_all(std::path::Path::new(&cfg.cache_dir));
                let _ = std::fs::copy(&old_path, &new_path);
            }
        }
        // init domains if empty
        if cfg.domains.is_empty() {
            drop(cfg);
            cfproxy::init_cfproxy_domains();
        }
    }
    let cancel = CancellationToken::new();
    let pool = std::sync::Arc::new(WsPool::new(cancel.clone()));
    let listener = TcpListener::bind(format!("{}:{}", host, port)).await?;
    let pool_clone = pool.clone();
    let cancel_clone = cancel.clone();
    let host_s = host.to_string();
    let handle = tokio::spawn(async move {
        run_proxy(pool_clone, host_s, port, dc_map, cancel_clone, listener).await
    });
    // give it a moment to bind
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    Ok((cancel, handle))
}
