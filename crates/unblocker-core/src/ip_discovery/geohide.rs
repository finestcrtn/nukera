//! GeoHide DNS hosts-file fetcher.
//!
//! Source: `https://raw.githubusercontent.com/Internet-Helper/GeoHideDNS/
//! refs/heads/main/hosts/hosts`
//!
//! This is a community-vetted hosts file that maps 200+ blocked services
//! to a small set of anti-DPI forward-proxies (currently 45.155.204.190
//! and 37.230.192.51 — Russian exit nodes the user's ISP does not
//! fingerprint) plus a few CDN IPs that have been working across multiple
//! regions.
//!
//! MagilaWEB pulls this same file on every startup (see
//! `src/unblock/dns_host.cpp::_loadInfo` in
//! `MagilaWEB/unblock-youtube-discord`). We do the same on `install-discover`
//! so that the runtime's `/etc/hosts` has a fresh community list without
//! us having to maintain our own.

use std::time::Duration;

/// Source URL for the community-vetted hosts file.
///
/// We use two mirrors in priority order:
/// 1. `cdn.jsdelivr.net` (works through most CDNs, rarely blocked by DPI)
/// 2. `raw.githubusercontent.com` (original source, sometimes blocked)
///
/// On any failure (offline, blocked, timeout) we fall back to the
/// bundled MagilaWEB list.
pub const GEOHIDE_PRIMARY: &str =
    "https://cdn.jsdelivr.net/gh/Internet-Helper/GeoHideDNS@main/hosts/hosts";
pub const GEOHIDE_FALLBACK: &str =
    "https://raw.githubusercontent.com/Internet-Helper/GeoHideDNS/refs/heads/main/hosts/hosts";

/// Fetch the GeoHide community hosts file. Returns one (host, ip) entry
/// per non-comment, non-blank line. Empty on network/parse error.
///
/// Tries two mirrors in order: jsDelivr CDN (more reliable through DPI),
/// then raw.githubusercontent.com. Falls back to bundled MagilaWEB list
/// on total failure.
pub fn fetch_geohide_hosts() -> Vec<(String, String)> {
    let body = match fetch_with_timeout(GEOHIDE_PRIMARY, Duration::from_secs(10)) {
        Ok(s) => {
            tracing::info!("GeoHide: fetched via jsDelivr CDN ({} bytes)", s.len());
            s
        }
        Err(e1) => {
            tracing::warn!("GeoHide jsDelivr: {}; trying raw.githubusercontent.com", e1);
            match fetch_with_timeout(GEOHIDE_FALLBACK, Duration::from_secs(10)) {
                Ok(s) => {
                    tracing::info!("GeoHide: fetched via raw.githubusercontent.com ({} bytes)", s.len());
                    s
                }
                Err(e2) => {
                    tracing::warn!(err1 = %e1, err2 = %e2, "GeoHide: both mirrors failed; using MagilaWEB-only set");
                    return Vec::new();
                }
            }
        }
    };
    parse_hosts_format(&body)
}

/// Synchronous HTTP GET with a hard timeout. Uses reqwest in blocking
/// mode. We don't reuse the doh_query() helper because that's tied
/// to a specific DoH JSON shape, not a plain text hosts file.
fn fetch_with_timeout(url: &str, timeout: Duration) -> anyhow::Result<String> {
    let body = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let client = reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .map_err(|e| anyhow::anyhow!("GeoHide client: {}", e))?;
            let resp = client.get(url).send().await
                .map_err(|e| anyhow::anyhow!("GeoHide fetch: {}", e))?;
            let status = resp.status();
            if !status.is_success() {
                anyhow::bail!("GeoHide HTTP {}", status.as_u16());
            }
            let text = resp.text().await
                .map_err(|e| anyhow::anyhow!("GeoHide read: {}", e))?;
            Ok(text)
        })
    })?;
    Ok(body)
}

/// Parse a hosts-format file: `IP hostname [more-hostnames...]` per line.
/// Comment lines start with `#` and are skipped. Blank lines are
/// skipped. Lines that don't parse cleanly are skipped (we'd rather
/// lose a few entries than fail the whole fetch).
pub fn parse_hosts_format(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw_line in body.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Strip inline comments (some hosts files have `IP host # comment`).
        let line = match line.find(" #") {
            Some(idx) => &line[..idx],
            None => line,
        };
        let line = line.trim();
        let mut parts = line.split_whitespace();
        let Some(ip) = parts.next() else { continue };
        // The IP must look like one. Cheap check: contains a dot or colon.
        if !ip.contains('.') && !ip.contains(':') {
            continue;
        }
        for host in parts {
            // Skip obvious junk lines (e.g. "AMD" section headers in the
            // GeoHide file are single-word comments with no IP — already
            // caught by the starts_with('#') check, but defensive).
            if host.is_empty() || host.contains(' ') {
                continue;
            }
            out.push((host.to_string(), ip.to_string()));
        }
    }
    out
}

/// Filter the GeoHide list down to entries that are interesting for
/// our known target set. The full GeoHide file has 5,000+ entries; we
/// only need the ones that match sites we know about. The caller passes
/// a slice of host strings and we return the subset of GeoHide entries
/// whose host is in that set.
pub fn filter_to_targets(
    entries: Vec<(String, String)>,
    target_hosts: &[&str],
) -> Vec<(String, String)> {
    entries
        .into_iter()
        .filter(|(host, _)| {
            let h = host.to_lowercase();
            target_hosts.iter().any(|t| {
                let t = t.to_lowercase();
                h == t || h.ends_with(&format!(".{t}"))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_hosts() {
        let body = "\
# this is a comment
157.240.0.174 instagram.com
31.13.66.63 scontent.cdninstagram.com
57.144.244.192 static.cdninstagram.com graph.instagram.com

# blank line above, then a malformed line
notanip foo
157.240.0.174 # inline comment
";
        let out = parse_hosts_format(body);
        assert_eq!(out.len(), 4, "got {:?}", out);
        assert_eq!(out[0], ("instagram.com".into(), "157.240.0.174".into()));
        assert_eq!(out[1], ("scontent.cdninstagram.com".into(), "31.13.66.63".into()));
        assert_eq!(out[2], ("static.cdninstagram.com".into(), "57.144.244.192".into()));
        assert_eq!(out[3], ("graph.instagram.com".into(), "57.144.244.192".into()));
    }

    #[test]
    fn filter_subdomain_match() {
        let entries = vec![
            ("scontent-hel3-1.cdninstagram.com".into(), "31.13.66.63".into()),
            ("www.instagram.com".into(), "157.240.0.174".into()),
            ("unrelated.com".into(), "1.2.3.4".into()),
        ];
        let targets = vec!["instagram.com", "facebook.com", "cdninstagram.com"];
        let out = filter_to_targets(entries, &targets);
        // scontent-hel3-1.cdninstagram.com is subdomain of cdninstagram.com -> match
        // www.instagram.com is a subdomain of instagram.com -> match
        // unrelated.com -> no match
        assert_eq!(out.len(), 2, "got {:?}", out);
    }
}
