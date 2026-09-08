//! List of sites the daemon watches. Static, compiled into the binary.
//!
//! Each site has a `kind` that drives the discovery strategy:
//! - `Dpi` sites: nfqws DPI desync + a working strategy from the pool.
//! - `Ip` sites: a non-blocked CDN edge IP from the known-good pool.
//! - `Both`: try DPI first, fall back to IP override.
//! - `Wss`: needs a WSS tunnel (Phase 2c, Telegram).

use std::fmt;
use std::net::IpAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteKind {
    /// TLS DPI block — try a winning DPI strategy from the pool.
    Dpi,
    /// IP block on the entire CDN range — need a known-clean IP in /etc/hosts.
    Ip,
    /// Try DPI first, fall back to IP override if DPI fails.
    Both,
    /// Telegram — needs WSS proxy (Phase 2c, deferred).
    Wss,
}

#[derive(Debug, Clone)]
pub struct Site {
    pub host: &'static str,
    pub kind: SiteKind,
    pub test_url: &'static str,
    /// List of candidate /24s to scan if DPI fails (for `Ip` and `Both` sites).
    /// These are the curated "known-good" ranges from V3nilla, the user's
    /// empirical probe of this network, and bol-van's proxytest data.
    pub allowed_ip_ranges: &'static [&'static str],
}

impl fmt::Display for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<24} ({:?})", self.host, self.kind)
    }
}

/// The full list. Compiled into the binary; user can extend via
/// `/etc/unblocker/sites.json` (future work).
pub fn all() -> Vec<Site> {
    vec![
        // === DPI-blocked (DPI desync wins on this network) ===
        Site {
            host: "youtube.com",
            kind: SiteKind::Dpi,
            test_url: "https://www.youtube.com/",
            allowed_ip_ranges: &[],
        },
        Site {
            host: "discord.com",
            kind: SiteKind::Dpi,
            test_url: "https://discord.com/",
            allowed_ip_ranges: &[],
        },
        Site {
            host: "meduza.io",
            kind: SiteKind::Dpi,
            test_url: "https://meduza.io/",
            allowed_ip_ranges: &[],
        },
        Site {
            host: "rutracker.org",
            kind: SiteKind::Dpi,
            test_url: "http://rutracker.org/",
            allowed_ip_ranges: &[],
        },
        Site {
            host: "vk.com",
            kind: SiteKind::Dpi,
            test_url: "https://vk.com/",
            allowed_ip_ranges: &[],
        },
        Site {
            host: "bbc.com",
            kind: SiteKind::Dpi,
            test_url: "https://www.bbc.com/",
            allowed_ip_ranges: &[],
        },
        Site {
            host: "x.com",
            kind: SiteKind::Dpi,
            test_url: "https://x.com/",
            allowed_ip_ranges: &[],
        },
        Site {
            host: "signal.org",
            kind: SiteKind::Dpi,
            test_url: "https://signal.org/",
            allowed_ip_ranges: &[],
        },
        Site {
            host: "google.com",
            kind: SiteKind::Dpi,
            test_url: "https://www.google.com/",
            allowed_ip_ranges: &[],
        },

        // === IP-blocked (need a known-good CDN edge IP) ===
        //
        // We seed the IP scan with the full CDN anycast space for
        // Meta / WhatsApp / Twitter / Telegram. The scanner caps the
        // expanded set at ~2k IPs per site to keep the brute-force
        // scan bounded. If a particular /24 / /22 is blocked by the
        // user's ISP, the rest of the range is the next-best bet.
        //
        // Sources for these ranges:
        //  - https://www.cloudflare.com/ips/
        //  - https://docs.fastly.com/products/fastly-network/ip-addresses
        // === DPI-blocked Meta/WA services (v6.1 course correction) ===
        // Per user testing Aug 30 2026: IG/FB/WA are NOT IP-blocked in
        // Russia on the user's network. The IG app works through byeDPI,
        // so Meta IPs route fine. The failure of the website is a SNI-
        // based DPI match on the literal string "www.instagram.com" /
        // "www.facebook.com" / "web.whatsapp.com". The fix is to nfqws-
        // desync these specific SNIs (hostfakesplit + ts + autottl=2
        // already works for *.youtube.com — same family).
        Site {
            host: "instagram.com",
            kind: SiteKind::Dpi,
            test_url: "https://www.instagram.com/",
            allowed_ip_ranges: &[],
        },
        Site {
            host: "facebook.com",
            kind: SiteKind::Dpi,
            test_url: "https://www.facebook.com/",
            allowed_ip_ranges: &[],
        },
        Site {
            host: "web.whatsapp.com",
            kind: SiteKind::Dpi,
            test_url: "https://web.whatsapp.com/",
            allowed_ip_ranges: &[],
        },
        // === DPI-blocked (DPI desync wins on this network) ===
        // x.com is the same family as IG/FB: app works through DPI
        // bypass, web doesn't. Reclassified Dpi per v6.1.
        Site {
            host: "x.com",
            kind: SiteKind::Dpi,
            test_url: "https://x.com/",
            allowed_ip_ranges: &[],
        },

        // === Telegram (WSS — Phase 2c) ===
        Site {
            host: "t.me",
            kind: SiteKind::Wss,
            test_url: "https://t.me/",
            allowed_ip_ranges: &["91.108.4.0/22", "91.108.8.0/22",
                                  "91.108.12.0/22", "91.108.16.0/22",
                                  "91.108.20.0/22", "91.108.56.0/22",
                                  "149.154.160.0/20", "149.154.175.50/32"],
        },
        Site {
            host: "telegram.org",
            kind: SiteKind::Wss,
            test_url: "https://telegram.org/",
            allowed_ip_ranges: &["91.108.4.0/22", "91.108.8.0/22",
                                  "91.108.12.0/22", "91.108.16.0/22",
                                  "91.108.20.0/22", "91.108.56.0/22",
                                  "149.154.160.0/20", "149.154.175.50/32"],
        },
    ]
}

/// Expand each /24 in `ranges` into the 256 individual IPs to probe.
/// Cheap — 256 entries per /24 — but is the right shape for our
/// connection-probe loop. If a /16 is in the list, that's 4096 IPs
/// — too many; the daemon should narrow to /24 chunks in practice.
pub fn expand_ranges(ranges: &[&str]) -> Vec<IpAddr> {
    use ipnet::IpNet;
    let mut out = Vec::new();
    for r in ranges {
        if let Ok(net) = r.parse::<IpNet>() {
            for ip in net.hosts() {
                out.push(ip);
            }
        }
    }
    out
}