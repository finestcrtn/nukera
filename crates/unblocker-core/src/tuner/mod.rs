use anyhow::Result;
use std::time::Duration;
use tracing::{info, warn};

use crate::bypass::{BypassConfig, BypassHandle, candidate_pool};

#[derive(Debug, Clone)]
pub struct ProbeTarget {
    pub host: String,
    pub url: String,
}

pub fn default_targets() -> Vec<ProbeTarget> {
    vec![
        ProbeTarget {
            host: "youtube.com".into(),
            url: "https://youtube.com/".into(),
        },
        ProbeTarget {
            host: "discord.com".into(),
            url: "https://discord.com/".into(),
        },
        ProbeTarget {
            host: "instagram.com".into(),
            url: "https://instagram.com/".into(),
        },
        ProbeTarget {
            host: "x.com".into(),
            url: "https://x.com/".into(),
        },
        ProbeTarget {
            host: "bbc.com".into(),
            url: "https://www.bbc.com/".into(),
        },
    ]
}

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub host: String,
    pub ok: bool,
    pub ms: u64,
    pub status: Option<u16>,
}

async fn probe_via_socks(url: &str, socks_url: &str) -> ProbeResult {
    let t0 = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .proxy(reqwest::Proxy::all(socks_url).unwrap())
        .redirect(reqwest::redirect::Policy::limited(2))
        .danger_accept_invalid_certs(false)
        .build();
    let host = url.to_string();
    let client = match client {
        Ok(c) => c,
        Err(_) => {
            return ProbeResult {
                host,
                ok: false,
                ms: t0.elapsed().as_millis() as u64,
                status: None,
            }
        }
    };
    let resp = client.get(url).send().await;
    let ms = t0.elapsed().as_millis() as u64;
    match resp {
        Ok(r) => ProbeResult {
            host: url.into(),
            ok: r.status().is_success() || r.status().is_redirection(),
            ms,
            status: Some(r.status().as_u16()),
        },
        Err(e) => {
            warn!(%url, err=%e, "probe failed");
            ProbeResult {
                host: url.into(),
                ok: false,
                ms,
                status: None,
            }
        }
    }
}

async fn probe_direct(url: &str) -> ProbeResult {
    let t0 = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let resp = client.get(url).send().await;
    let ms = t0.elapsed().as_millis() as u64;
    match resp {
        Ok(r) => ProbeResult {
            host: url.into(),
            ok: r.status().is_success() || r.status().is_redirection(),
            ms,
            status: Some(r.status().as_u16()),
        },
        Err(_) => ProbeResult {
            host: url.into(),
            ok: false,
            ms,
            status: None,
        },
    }
}

pub async fn check_site(url: &str, socks_url: Option<&str>) -> (ProbeResult, Option<ProbeResult>) {
    let direct = probe_direct(url).await;
    let via = if let Some(s) = socks_url {
        Some(probe_via_socks(url, s).await)
    } else {
        None
    };
    (direct, via)
}

pub struct TuneReport {
    pub best_args: String,
    pub scores: Vec<(String, usize, u64)>, // args, ok_count, avg_ms
}

pub async fn quick_tune(
    bin: &str,
    socks_base_port: u16,
    targets: &[ProbeTarget],
) -> Result<TuneReport> {
    let pool = candidate_pool();
    let mut scores: Vec<(String, usize, u64)> = Vec::new();

    for (i, args) in pool.iter().enumerate() {
        let port = socks_base_port + i as u16 + 10;
        let cfg = BypassConfig {
            bin: bin.into(),
            socks_addr: "127.0.0.1".into(),
            socks_port: port,
            args: args.clone(),
            transparent: false,
        };
        let handle = match BypassHandle::spawn(cfg).await {
            Ok(h) => h,
            Err(e) => {
                warn!(%args, err=%e, "skip candidate — spawn failed");
                scores.push((args.clone(), 0, 9999));
                continue;
            }
        };
        tokio::time::sleep(Duration::from_millis(900)).await;
        let socks = handle.socks_url();
        let mut ok = 0usize;
        let mut total_ms = 0u64;
        for t in targets {
            let r = probe_via_socks(&t.url, &socks).await;
            if r.ok {
                ok += 1;
            }
            total_ms += r.ms;
        }
        let avg = if targets.is_empty() {
            0
        } else {
            total_ms / targets.len() as u64
        };
        info!(%args, ok, avg_ms=%avg, "candidate scored");
        scores.push((args.clone(), ok, avg));
        let _ = handle.stop().await;
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    scores.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    let best = scores.first().cloned().unwrap_or((
        "--disorder 1 --auto=torst --tlsrec 1+s".into(),
        0,
        0,
    ));
    Ok(TuneReport {
        best_args: best.0.clone(),
        scores,
    })
}
