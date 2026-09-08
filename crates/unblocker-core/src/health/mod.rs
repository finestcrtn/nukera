use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{info, warn};

use crate::tuner::{ProbeTarget, default_targets};

#[derive(Debug, Clone)]
pub struct HealthState {
    pub last_ok: HashMap<String, Instant>,
    pub fails: HashMap<String, u32>,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            last_ok: HashMap::new(),
            fails: HashMap::new(),
        }
    }
    pub fn record(&mut self, host: &str, ok: bool) {
        if ok {
            self.last_ok.insert(host.into(), Instant::now());
            self.fails.insert(host.into(), 0);
        } else {
            *self.fails.entry(host.into()).or_insert(0) += 1;
        }
    }
    pub fn needs_mitigation(&self, host: &str) -> bool {
        self.fails.get(host).copied().unwrap_or(0) >= 3
    }
}

pub async fn probe_once(url: &str, _socks_url: &str) -> bool {
    // The byedpi engine runs in --transparent mode: iptables REDIRECT
    // hands our 80/443 SYN to byedpi. So a direct HTTPS request is
    // the correct probe — if iptables is up, we get a real response;
    // if not, we get a connect error. We do NOT need a SOCKS client.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(2))
        .build();
    let Ok(client) = client else { return false };
    match client.get(url).send().await {
        Ok(r) => r.status().is_success() || r.status().is_redirection(),
        Err(e) => {
            warn!(%url, err=%e, "health probe failed");
            false
        }
    }
}

pub async fn health_loop(socks_url: String, interval: Duration) {
    let mut state = HealthState::new();
    let targets = default_targets();
    let mut idx = 0usize;
    loop {
        tokio::time::sleep(interval).await;
        // rotate 3 hosts
        let batch: Vec<ProbeTarget> = (0..3)
            .map(|k| targets[(idx + k) % targets.len()].clone())
            .collect();
        idx = (idx + 3) % targets.len();
        for t in &batch {
            let ok = probe_once(&t.url, &socks_url).await;
            state.record(&t.host, ok);
            if ok {
                info!(host=%t.host, "health ok");
            } else if state.needs_mitigation(&t.host) {
                warn!(host=%t.host, "health needs mitigation (3 fails) — would try next strategy / mini-sweep");
                // hook: trigger tuner quick_tune for this host only, update cache
            } else {
                warn!(host=%t.host, "health fail");
            }
        }
    }
}
