//! The list of sites the daemon watches. Static, compiled into the
//! binary. Both DPI-blocked and IP-blocked.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteKind {
    /// TLS DPI block — nfqws with the strategy pool handles it.
    Dpi,
    /// Whole IP range blocked — needs an unblocked IP in /etc/hosts.
    Ip,
    /// Both DPI and IP — try DPI first, fall back to IP discovery.
    Both,
    /// Special: needs WSS tunnel (Phase 2c, telegram).
    Wss,
}

#[derive(Debug, Clone)]
pub struct Site {
    pub host: String,
    pub kind: SiteKind,
    pub test_url: String,
}

impl fmt::Display for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<22} ({:?})", self.host, self.kind)
    }
}

/// The full list. The daemon probes all of these every 30s and
/// adapts the strategy or IP based on results.
pub fn all() -> Vec<Site> {
    vec![
        // DPI-blocked on this network
        Site { host: "youtube.com".into(),      kind: SiteKind::Dpi,  test_url: "https://www.youtube.com/".into() },
        Site { host: "discord.com".into(),      kind: SiteKind::Dpi,  test_url: "https://discord.com/".into() },
        Site { host: "meduza.io".into(),        kind: SiteKind::Dpi,  test_url: "https://meduza.io/".into() },
        Site { host: "rutracker.org".into(),    kind: SiteKind::Dpi,  test_url: "http://rutracker.org/".into() },
        Site { host: "vk.com".into(),           kind: SiteKind::Dpi,  test_url: "https://vk.com/".into() },
        Site { host: "bbc.com".into(),          kind: SiteKind::Dpi,  test_url: "https://www.bbc.com/".into() },
        Site { host: "x.com".into(),            kind: SiteKind::Dpi,  test_url: "https://x.com/".into() },
        Site { host: "signal.org".into(),       kind: SiteKind::Dpi,  test_url: "https://signal.org/".into() },
        Site { host: "google.com".into(),       kind: SiteKind::Dpi,  test_url: "https://www.google.com/".into() },
        // IP-blocked on this network (full /etc/hosts bypass needed)
        Site { host: "instagram.com".into(),    kind: SiteKind::Ip,   test_url: "https://www.instagram.com/".into() },
        Site { host: "facebook.com".into(),     kind: SiteKind::Ip,   test_url: "https://www.facebook.com/".into() },
        Site { host: "web.whatsapp.com".into(), kind: SiteKind::Ip,   test_url: "https://web.whatsapp.com/".into() },
        // Telegram — both
        Site { host: "t.me".into(),             kind: SiteKind::Wss,  test_url: "https://t.me/".into() },
        Site { host: "telegram.org".into(),     kind: SiteKind::Wss,  test_url: "https://telegram.org/".into() },
    ]
}

/// Map a host to a small representative URL used for health probes.
/// The runtime uses these on each enable-cycle to detect "this host's
/// IP went bad" and trigger a single-host re-discovery (Unit 3 of
/// v6.5 plan). The list is small and hand-curated from the user's
/// confirmed-working URLs.
///
/// Returns None for hosts we don't care about. The probe should be
/// cheap (one HTTP GET, < 1 s) and the response should be a real
/// body (not a redirect) so we can confirm the request actually
/// reached the right backend.
pub fn asset_probe_url(host: &str) -> Option<&'static str> {
    match host {
        // IG: the user's exact image URL — proves images load
        "instagram.com" | "www.instagram.com" | "cdninstagram.com" | "scontent.cdninstagram.com"
        | "scontent-hel3-1.cdninstagram.com" => Some(
            "https://scontent-hel3-1.cdninstagram.com/v/t51.2885-19/455596973_468815979375023_2483373531962989209_n.jpg?stp=dst-jpg_s150x150_tt6&_nc_cat=100&ccb=7-5&_nc_sid=f7ccc5&efg=eyJ2ZW5jb2RlX3RhZyI6InByb2ZpbGVfcGljLnd3dy4zMzMuQzMifQ%3D%3D&_nc_ohc=jDweqdYxbNwQ7kNvwELuwPm&_nc_oc=AdrrXnEwmrkQM9Y6J3FL83r9Ya0QLDT6ZsYx5i4SFlhdDJRDHqZ6FD2h5ukvlPG7H28&_nc_zt=24&_nc_ht=scontent-hel3-1.cdninstagram.com&_nc_ss=7b6a8&oh=00_AQJ8RUB_46NQeCLWqPyrMKgqDNRhpkNFbskPUn9ZE-a1sA&oe=6A9B87FF"
        ),
        // FB: main page
        "facebook.com" | "www.facebook.com" => Some("https://www.facebook.com/"),
        // WA: faq page (the user's test URL — different host from web.whatsapp.com,
        // exercises a separate CDN path)
        "web.whatsapp.com" | "whatsapp.com" | "www.whatsapp.com" | "faq.whatsapp.com"
        | "blog.whatsapp.com" | "g.whatsapp.net" => Some(
            "https://faq.whatsapp.com/ru/web/26000012/?category=5245235"
        ),
        // Default for any IP-blocked site: just probe the main page
        _ if host.ends_with(".fbcdn.net")
            || host == "fbcdn.net"
            || host.ends_with(".cdninstagram.com")
            || host == "cdninstagram.com"
            || host.ends_with(".whatsapp.net")
            || host == "whatsapp.net" => Some("https://www.instagram.com/"),
        _ => None,
    }
}
