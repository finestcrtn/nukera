use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: u32,
    pub doh_primary: String,
    pub doh_fallback: String,
    pub doh_ru: String,
    pub socks_addr: String,
    pub socks_port: u16,
    pub byedpi_bin: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub hosts_overrides: Vec<HostOverride>,
    #[serde(default)]
    pub bypass_domains: Vec<String>,
    #[serde(default)]
    pub exclude_domains: Vec<String>,
    #[serde(default = "default_true")]
    pub telegram_bridge: bool,
    pub tg_ws_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostOverride {
    pub domain: String,
    pub ips: Vec<String>,
}

fn default_profile() -> String {
    // Disorder only. --conn-ip forces IPv4 bind (byeDPI defaults to IPv6 :: which fails on Android).
    "--disorder 1 --conn-ip 0.0.0.0".into()
}
fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            doh_primary: "https://dns.google/dns-query".into(),
            doh_fallback: "https://doh.opendns.com/dns-query".into(),
            doh_ru: "https://xbox-dns.ru/dns-query".into(),
            socks_addr: "127.0.0.1".into(),
            socks_port: 1080,
            byedpi_bin: "vendor/byedpi/ciadpi".into(),
            profile: default_profile(),
            hosts_overrides: default_hosts(),
            bypass_domains: default_bypass_domains(),
            exclude_domains: default_exclude_domains(),
            telegram_bridge: true,
            tg_ws_port: 1443,
        }
    }
}

fn default_bypass_domains() -> Vec<String> {
    vec![
        "youtube.com".into(),
        "googlevideo.com".into(),
        "ytimg.com".into(),
        "discord.com".into(),
        "discord.gg".into(),
        "instagram.com".into(),
        "cdninstagram.com".into(),
        "fbcdn.net".into(),
        "facebook.com".into(),
        "fb.com".into(),
        "fbsbx.com".into(),
        "scontent.cdninstagram.com".into(),
        "x.com".into(),
        "twitter.com".into(),
        "t.co".into(),
        "twimg.com".into(),
        "bbc.com".into(),
        "bbc.co.uk".into(),
        "t.me".into(),
        "telegram.org".into(),
        "whatsapp.com".into(),
        "grok.com".into(),
        "x.ai".into(),
        "imo.im".into(),
    ]
}

fn default_exclude_domains() -> Vec<String> {
    vec![
        "yandex.ru".into(),
        "ya.ru".into(),
        "mail.ru".into(),
        "gosuslugi.ru".into(),
        "sberbank.ru".into(),
    ]
}

fn default_hosts() -> Vec<HostOverride> {
    // v5: we do NOT hardcode IPs. The daemon's `install-discover` subcommand
    // probes DoH + TCP + TLS and writes only the IPs that actually work
    // on this network. The result is honest: if Meta's IP range is
    // blocked on the user's ISP, the daemon reports "no working IP" and
    // writes nothing — no fake entries.
    vec![]
}

pub fn config_dir() -> PathBuf {
    if let Some(d) = ProjectDirs::from("com", "unblocker", "unblocker") {
        d.config_dir().to_path_buf()
    } else {
        PathBuf::from(".")
    }
}

// Override via FFI set_app_paths() on mobile (project_dir is a no-op in an
// Android sandbox without a HOME/XDG). Unset => linux default.
use parking_lot::Mutex as ParkMutex;
static CONFIG_DIR: ParkMutex<Option<PathBuf>> = ParkMutex::new(None);

pub fn set_config_dir(p: PathBuf) {
    *CONFIG_DIR.lock() = Some(p);
}

pub fn config_path() -> PathBuf {
    if let Some(d) = CONFIG_DIR.lock().as_ref() {
        return d.join("config.json");
    }
    config_dir().join("config.json")
}

pub fn load_or_create() -> Result<AppConfig> {
    let p = config_path();
    if p.exists() {
        let s = std::fs::read_to_string(&p)?;
        Ok(serde_json::from_str(&s)?)
    } else {
        let cfg = AppConfig::default();
        save(&cfg)?;
        Ok(cfg)
    }
}

pub fn save(cfg: &AppConfig) -> Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let p = config_path();
    let s = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&p, s)?;
    Ok(())
}
