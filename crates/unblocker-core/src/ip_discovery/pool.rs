//! Precompiled IP pool for the IP-blocked services.
//!
// These are *candidates* — the daemon probes every one before adding
//! it to /etc/hosts. If all of them are dead on this network, the
//! daemon reports "no working IP" honestly and falls back to the
//! WSS-tunnel path (Phase 2c) for Telegram and to the static IP only
//! for the others.

/// Return a list of candidate IPs to try for `host`. Order: most-recently
/// confirmed first. Discovery will TCP+TLS probe each one.
pub fn precompiled_ips_for(host: &str) -> Vec<std::net::IpAddr> {
    let addrs: &[&str] = match host {
        "instagram.com" => &[
            // Confirmed working on this user's network (May 2026 probe).
            "157.240.9.174",
            "157.240.0.35",
            // Standard Meta CDN, kept as fallback in case the above
            // two go bad. These were TCP-OK but TLS-RST on this user's
            // network; may work elsewhere.
            "157.240.5.35",
            "157.240.3.35",
        ],
        "facebook.com" => &[
            "157.240.0.35",
            "157.240.9.174",
            "157.240.3.35",
        ],
        "web.whatsapp.com" => &[
            "157.240.0.174",
            "157.240.9.174",
            "157.240.0.35",
        ],
        "faq.whatsapp.com" => &[
            "157.240.0.174",
            "31.13.72.52",
        ],
        "t.me" | "telegram.org" => &[
            // Telegram DC IP ranges. The standard 149.154.167.99 is
            // blocked on this network; we try the others via discovery.
            "149.154.175.50",
            "91.108.4.1",
            "91.108.8.1",
            "91.108.12.1",
            "91.108.16.1",
            "91.108.20.1",
            "91.108.56.1",
            "185.76.151.1",
        ],
        _ => &[],
    };
    addrs.iter().filter_map(|s| s.parse().ok()).collect()
}
