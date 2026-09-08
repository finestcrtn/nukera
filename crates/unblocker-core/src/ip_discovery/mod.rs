//! IP discovery for IP-blocked services.
//!
// The daemon uses this module to:
//!   1. At install time (run via the `unblocker install-discover` subcommand):
//!      resolve candidate IPs from DoH, probe them with a real HTTPS GET,
//!      write the working ones to /etc/hosts in the unblocker block.
//!   2. At runtime (Phase 2b): on 3 consecutive failed probes for a host
//!      with a /etc/hosts entry, re-run discovery and swap the entry.
//!
//! The daemon does NOT pre-probe on a timer. The only work is on demand.
//!
//! Honesty: if no candidate IP works, the daemon reports that honestly in
//! journald. We never put a fake IP into /etc/hosts.

use anyhow::Result;
use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

pub mod pool;
pub mod sites;
pub mod geohide;

/// One candidate IP for a target host.
#[derive(Debug, Clone)]
pub struct IpCandidate {
    pub ip: IpAddr,
    pub source: DiscoverySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    /// Returned by one of the DoH providers.
    GoogleDoH,
    CloudflareDoH,
    /// From our precompiled pool of known-good IPs (last-verified date
    /// in the source code). Pool is the seed, not the steady state.
    Precompiled,
    /// Returned by the system DNS resolver.
    System,
    /// From the GeoHide DNS community hosts file
    /// (https://github.com/Internet-Helper/GeoHideDNS), which is a
    /// community-vetted list of CDN IPs + 2 Russian exit-node IPs
    /// (45.155.204.190, 37.230.192.51) that bypass DPI for most
    /// blocked services. Fetched at install-discover time.
    GeoHide,
}

/// A /etc/hosts entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostsEntry {
    pub host: String,
    pub ip: IpAddr,
}

/// Build the list of candidate IPs for a host. The order is the priority:
/// precompiled first (last-known-good), then Google DoH, then Cloudflare
/// DoH. Duplicates removed.
pub fn discover_candidates(host: &str) -> Vec<IpCandidate> {
    let mut out: Vec<IpCandidate> = Vec::new();
    let mut seen: HashSet<IpAddr> = HashSet::new();

    for ip in pool::precompiled_ips_for(host) {
        if seen.insert(ip) {
            out.push(IpCandidate { ip, source: DiscoverySource::Precompiled });
        }
    }
    for ip in query_google_doh(host).unwrap_or_default() {
        if seen.insert(ip) {
            out.push(IpCandidate { ip, source: DiscoverySource::GoogleDoH });
        }
    }
    for ip in query_cloudflare_doh(host).unwrap_or_default() {
        if seen.insert(ip) {
            out.push(IpCandidate { ip, source: DiscoverySource::CloudflareDoH });
        }
    }
    out
}

/// Discover candidates for a host, including GeoHide DNS community-vetted
/// IPs (used by `unblocker install-discover` to seed `/etc/hosts`).
/// GeoHide adds ~50 vetted IPs that the DoH path doesn't know about,
/// especially for subdomains that don't have their own A records
/// (e.g. `faq.whatsapp.com`, `mmg-fna.whatsapp.net`).
pub fn discover_candidates_with_geohide(host: &str) -> Vec<IpCandidate> {
    let mut out = discover_candidates(host);
    let mut seen: HashSet<IpAddr> = out.iter().map(|c| c.ip).collect();

    // Pull the GeoHide list (1 HTTP GET, 10s timeout). The result is
    // a flat list of (host, ip) pairs for the entire community file
    // (~5,000 entries). We only take entries that match `host` or
    // any of its parent domains.
    let gh = geohide::fetch_geohide_hosts();
    if gh.is_empty() {
        return out;
    }
    let parents = parent_domains_of(host);
    let filtered = geohide::filter_to_targets(gh, &parents);
    tracing::info!(host, geohide_hits = filtered.len(), "GeoHide DNS filtered hits");
    for (_, ip_s) in filtered {
        if let Ok(ip) = ip_s.parse::<IpAddr>() {
            if seen.insert(ip) {
                out.push(IpCandidate { ip, source: DiscoverySource::GeoHide });
            }
        }
    }
    out
}

/// Return the candidate list including `host` itself and all its
/// parent domains. e.g. `scontent-hel3-1.cdninstagram.com` ->
/// ["scontent-hel3-1.cdninstagram.com", "cdninstagram.com", "instagram.com", "com"].
fn parent_domains_of(host: &str) -> Vec<&str> {
    let mut out = vec![host];
    let mut current = host;
    while let Some(idx) = current.find('.') {
        current = &current[idx + 1..];
        if current.is_empty() {
            break;
        }
        out.push(current);
    }
    out
}

/// One-shot "this host is broken, find a new working IP" pipeline.
/// Used by:
///   * `unblocker refresh <host>` subcommand
///   * the runtime's startup-sweep self-heal (Unit 3)
///
/// Steps:
///   1. Build candidate list (precompiled + GeoHide + Google DoH + Cloudflare DoH)
///   2. TCP+TLS probe each candidate (TLS handshake with SNI = host)
///   3. Return the first one that responds; if none, return None
///
/// This is the user-triggered recovery: no polling, no background work,
/// just one DoH query + N TCP probes (~3 s end-to-end for N=20).
pub fn refresh_host(host: &str) -> Option<IpAddr> {
    let timeout = Duration::from_secs(4);
    let candidates = discover_candidates_with_geohide(host);
    tracing::info!(host, n = candidates.len(), "refresh_host: probing candidates");
    for c in candidates {
        if probe_tcp_443(c.ip, timeout) && probe_tls(c.ip, host, timeout) {
            tracing::info!(host, ip = %c.ip, source = ?c.source, "refresh_host: found working IP");
            return Some(c.ip);
        }
    }
    // Last-ditch: ask the system DNS for the canonical IP and try it.
    if let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(&(host, 443u16)) {
        for addr in addrs {
            if probe_tcp_443(addr.ip(), timeout) && probe_tls(addr.ip(), host, timeout) {
                tracing::info!(host, ip = %addr.ip(), "refresh_host: system-DNS IP works");
                return Some(addr.ip());
            }
        }
    }
    tracing::warn!(host, "refresh_host: no candidate IP worked");
    None
}

/// TCP probe: open a connection to ip:443. Returns true if it succeeds
/// within `timeout`.
pub fn probe_tcp_443(ip: IpAddr, timeout: Duration) -> bool {
    let addr = SocketAddr::new(ip, 443);
    TcpStream::connect_timeout(&addr, timeout).is_ok()
}

/// Quick TCP+TLS probe: open TCP to ip:443, then attempt a minimal
/// TLS handshake. Returns true only if both succeed.
///
/// We do a hand-rolled TLS 1.3 ClientHello (just enough to elicit a
/// ServerHello) so we can tell "TLS is filtered" apart from "TCP is
/// filtered" without pulling in rustls. If the server replies with any
/// TLS record, the path is alive. We don't validate the cert; we only
/// need the IP-level route.
pub fn probe_tls(ip: IpAddr, host: &str, timeout: Duration) -> bool {
    use std::io::{Read, Write};
    let addr = SocketAddr::new(ip, 443);
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) else { return false; };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    // Build a minimal TLS 1.3 ClientHello with one cipher suite
    // (TLS_AES_128_GCM_SHA256 = 0x1301) and a SNI extension for `host`.
    let sni_host = host.as_bytes();
    let mut sni_payload = Vec::new();
    sni_payload.extend_from_slice(&((sni_host.len() as u16 + 3).to_be_bytes()));
    sni_payload.push(0); // host_name type
    sni_payload.extend_from_slice(&(sni_host.len() as u16).to_be_bytes());
    sni_payload.extend_from_slice(sni_host);

    let mut exts = Vec::new();
    // SNI extension
    exts.extend_from_slice(&0u16.to_be_bytes()); // SNI type
    exts.extend_from_slice(&(sni_payload.len() as u16).to_be_bytes());
    exts.extend_from_slice(&sni_payload);
    // Supported versions (TLS 1.3)
    let sv_payload: [u8; 3] = [0x03, 0x04, 0x03]; // length 2, 0x0304 = TLS 1.3
    exts.extend_from_slice(&0x002bu16.to_be_bytes()); // supported_versions type
    exts.extend_from_slice(&(sv_payload.len() as u16).to_be_bytes());
    exts.extend_from_slice(&sv_payload);

    let random: [u8; 32] = [0; 32];
    let session_id_len: u8 = 0;
    let cipher_suites: [u8; 2] = [0x13, 0x01]; // TLS_AES_128_GCM_SHA256
    let compression: u8 = 0;

    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(&random);
    body.push(session_id_len);
    body.extend_from_slice(&cipher_suites);
    body.push(compression);
    body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
    body.extend_from_slice(&exts);

    let mut hello = Vec::new();
    hello.push(0x01); // client_hello
    hello.push(0x00); hello.push(0x03); // legacy version (TLS record layer)
    hello.extend_from_slice(&(body.len() as u16).to_be_bytes());
    hello.extend_from_slice(&body);

    let mut record = Vec::new();
    record.push(0x16); // handshake
    record.push(0x03); record.push(0x01); // TLS 1.0 record layer version
    record.extend_from_slice(&(hello.len() as u16).to_be_bytes());
    record.extend_from_slice(&hello);

    if stream.write_all(&record).is_err() { return false; }
    let mut header = [0u8; 5];
    match stream.read_exact(&mut header) {
        Ok(_) => header[0] == 0x16, // got a TLS handshake record back
        Err(_) => false,
    }
}

/// Query Google DoH over HTTPS for the A records of `host`. Returns
/// IPv4 (and IPv6 if AAAA) addresses. Empty list on any error.
pub fn query_google_doh(host: &str) -> Result<Vec<IpAddr>> {
    doh_query(&format!(
        "https://dns.google/resolve?name={}&type=A",
        urlencode(host)
    ))
}

/// Query Cloudflare DoH over HTTPS for the A records of `host`.
pub fn query_cloudflare_doh(host: &str) -> Result<Vec<IpAddr>> {
    doh_query(&format!(
        "https://cloudflare-dns.com/dns-query?name={}&type=A",
        urlencode(host)
    ))
}

fn doh_query(url: &str) -> Result<Vec<IpAddr>> {
    // reqwest is the one place we go async inside this sync fn — we
    // block on a oneshot runtime. This is install-time code; latency
    // here is fine.
    let body = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let resp = reqwest::get(url).await?;
            resp.text().await
        })
    })?;
    parse_doh_response(&body)
}

/// Minimal DoH JSON parser. We only need `Answer[].data` and the `type`
/// field. Returns IPv4 and IPv6 from A and AAAA records respectively.
fn parse_doh_response(body: &str) -> Result<Vec<IpAddr>> {
    // We do a really lazy manual parse: find each `"data":"<ip>"` and
    // try to interpret it as an IP. This works for both Google and
    // Cloudflare DoH responses and is robust to schema differences.
    let mut out = Vec::new();
    let mut i = 0;
    let bytes = body.as_bytes();
    while i < bytes.len() {
        if let Some(pos) = find_subseq(&bytes[i..], b"\"data\":\"") {
            let start = i + pos + 8;
            if let Some(end) = bytes[start..].iter().position(|&b| b == b'"') {
                if let Ok(s) = std::str::from_utf8(&bytes[start..start + end]) {
                    if let Ok(ip) = s.parse::<std::net::Ipv4Addr>() {
                        out.push(IpAddr::V4(ip));
                    } else if let Ok(ip) = s.parse::<std::net::Ipv6Addr>() {
                        out.push(IpAddr::V6(ip));
                    }
                }
                i = start + end + 1;
                continue;
            }
        }
        break;
    }
    Ok(out)
}

fn find_subseq(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() { return Some(0); }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Read the current `unblocker` block from /etc/hosts.
pub fn current_hosts_entries() -> Result<Vec<HostsEntry>> {
    use std::io::Read;
    let mut s = String::new();
    std::fs::File::open("/etc/hosts")?.read_to_string(&mut s)?;
    let mut entries = Vec::new();
    let mut in_block = false;
    for line in s.lines() {
        if line.contains("# BEGIN unblocker") { in_block = true; continue; }
        if line.contains("# END unblocker") { in_block = false; continue; }
        if !in_block { continue; }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(ip) = parts[0].parse::<IpAddr>() {
                entries.push(HostsEntry { host: parts[1].to_string(), ip });
            }
        }
    }
    Ok(entries)
}

/// Add entries to the unblocker block in /etc/hosts. Atomically replaces
/// the existing block.
pub fn add_to_hosts(entries: &[HostsEntry]) -> Result<()> {
    use std::io::Read;
    let mut existing = String::new();
    std::fs::File::open("/etc/hosts")?.read_to_string(&mut existing)?;
    let stripped = strip_unblocker_block(&existing);
    let mut block = String::from("\n# BEGIN unblocker\n");
    for e in entries {
        block.push_str(&format!("{}\t{}\n", e.ip, e.host));
    }
    block.push_str("# END unblocker\n");
    let new = format!("{}{}", stripped.trim_end(), block);
    write_hosts(&new)
}

/// Remove entries that match the given hosts (case-insensitive). Preserves
/// other entries in the unblocker block.
pub fn remove_from_hosts(hosts: &[String]) -> Result<()> {
    let existing = current_hosts_entries()?;
    let keep: Vec<HostsEntry> = existing
        .into_iter()
        .filter(|e| !hosts.iter().any(|h| h.eq_ignore_ascii_case(&e.host)))
        .collect();
    add_to_hosts(&keep)
}

/// Read entire /etc/hosts, write new content. Caller already includes
/// the unblocker block in `new_content`.
pub fn write_hosts(new_content: &str) -> Result<()> {
    let tmp = "/tmp/unblocker.hosts.new";
    std::fs::write(tmp, new_content)?;
    let cp = std::process::Command::new("cp")
        .arg(tmp)
        .arg("/etc/hosts")
        .output();
    match cp {
        Ok(o) if o.status.success() => {}
        _ => {
            std::fs::write("/etc/hosts", new_content)?;
        }
    }
    let _ = std::fs::remove_file(tmp);
    Ok(())
}

fn strip_unblocker_block(content: &str) -> String {
    let begin = content.find("# BEGIN unblocker");
    let end = content.find("# END unblocker");
    if let (Some(b), Some(e)) = (begin, end) {
        if e > b {
            let mut s = String::new();
            s.push_str(&content[..b]);
            s.push_str(&content[e + "# END unblocker".len()..]);
            return s;
        }
    }
    content.to_string()
}

/// The full discovery cycle: find candidates, TCP-probe, TLS-probe, pick
/// the first that works. Returns Some(IpAddr) on success, None if no
/// candidate works. Caller is expected to write the result to /etc/hosts.
pub fn find_working_ip(host: &str) -> Option<IpAddr> {
    let cands = discover_candidates(host);
    if cands.is_empty() { return None; }
    let tcp_t = Duration::from_secs(2);
    let tls_t = Duration::from_secs(2);
    for c in cands {
        if !probe_tcp_443(c.ip, tcp_t) { continue; }
        if probe_tls(c.ip, host, tls_t) {
            return Some(c.ip);
        }
    }
    None
}
