use anyhow::{Context, Result};
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct BypassConfig {
    pub bin: String,
    pub socks_addr: String,
    pub socks_port: u16,
    /// Raw byedpi args, e.g. "--disorder 1 --auto=torst --tlsrec 1+s"
    pub args: String,
    /// Use byedpi --transparent (raw TCP + SOCKS, accepts iptables REDIRECT)
    pub transparent: bool,
}

impl Default for BypassConfig {
    fn default() -> Self {
        Self {
            bin: String::new(),
            socks_addr: "127.0.0.1".into(),
            socks_port: 1080,
            args: String::new(),
            transparent: true,
        }
    }
}

impl BypassConfig {
    pub fn socks_url(&self) -> String {
        format!("socks5://{}:{}", self.socks_addr, self.socks_port)
    }
    pub fn to_argv(&self) -> Vec<String> {
        let mut v = vec![
            "--ip".into(),
            self.socks_addr.clone(),
            "--port".into(),
            self.socks_port.to_string(),
        ];
        if self.transparent {
            v.push("--transparent".into());
        }
        let has_conn_ip = self.args.contains("--conn-ip") || self.args.contains("conn-ip");
        for tok in shell_words(&self.args) {
            v.push(tok);
        }
        if !has_conn_ip {
            v.push("--conn-ip".into());
            v.push("0.0.0.0".into());
        }
        v
    }
}

fn shell_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut q = ' ';
    for ch in s.chars() {
        if in_q {
            if ch == q {
                in_q = false;
            } else {
                cur.push(ch);
            }
        } else if ch == '\'' || ch == '"' {
            in_q = true;
            q = ch;
        } else if ch.is_whitespace() {
            if !cur.is_empty() {
                out.push(cur.clone());
                cur.clear();
            }
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub struct BypassHandle {
    child: Child,
    cfg: BypassConfig,
}

impl BypassHandle {
    pub async fn spawn(cfg: BypassConfig) -> Result<Self> {
        let argv = cfg.to_argv();
        info!(bin=%cfg.bin, args=?argv, "spawning byedpi");
        let mut cmd = Command::new(&cfg.bin);
        cmd.args(&argv)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = cmd.spawn().with_context(|| format!("spawn {}", cfg.bin))?;
        // quick liveness check: if binary missing, error is already returned.
        // Give it 200ms to crash early.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        Ok(Self { child, cfg })
    }

    pub fn socks_url(&self) -> String {
        self.cfg.socks_url()
    }

    pub async fn stop(mut self) -> Result<()> {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
        Ok(())
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

pub fn candidate_pool() -> Vec<String> {
    vec![
        "--tlsrec 1+s --conn-ip 0.0.0.0".into(),
        "--fake -1 --ttl 8 --conn-ip 0.0.0.0".into(),
        "--disorder 1 --tlsrec 1+s --conn-ip 0.0.0.0".into(),
        "--disorder 1 --conn-ip 0.0.0.0".into(),
        "--oob 3+s --conn-ip 0.0.0.0".into(),
        "--fake -1 --md5sig --conn-ip 0.0.0.0".into(),
        "--fake -1 --ttl 10 --conn-ip 0.0.0.0".into(),
        "--disorder 1 --auto=torst --tlsrec 1+s --conn-ip 0.0.0.0".into(),
        "--split 1+s --disorder 3+s --conn-ip 0.0.0.0".into(),
    ]
}

pub async fn check_bin(bin: &str) -> bool {
    tokio::fs::metadata(bin).await.is_ok()
        || Command::new(bin)
            .arg("--help")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
}

pub async fn ensure_byedpi(project_root: &std::path::Path) -> Result<String> {
    let exe_candidates = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
        .map(|exe_dir| {
            vec![
                exe_dir.join("vendor/byedpi/ciadpi"),
                exe_dir.join("../vendor/byedpi/ciadpi"),
                exe_dir.join("../../vendor/byedpi/ciadpi"),
            ]
        })
        .unwrap_or_default();
    let mut all: Vec<std::path::PathBuf> = Vec::new();
    all.extend(exe_candidates);
    all.extend([
        project_root.join("vendor/byedpi/ciadpi"),
        project_root.join("vendor/byedpi/byedpi"),
        project_root.join("vendor/byedpi/build/ciadpi"),
        std::path::PathBuf::from("/usr/local/bin/ciadpi"),
        std::path::PathBuf::from("/usr/bin/ciadpi"),
        std::path::PathBuf::from("/usr/bin/byedpi"),
    ]);
    for p in &all {
        if p.exists() {
            return Ok(p.to_string_lossy().into_owned());
        }
    }
    // Last chance: try building via make -C vendor/byedpi if Makefile exists
    let makefile = project_root.join("vendor/byedpi/Makefile");
    if makefile.exists() {
        let _ = std::process::Command::new("make")
            .arg("-C")
            .arg(project_root.join("vendor/byedpi"))
            .arg("-j4")
            .output();
        for p in &all {
            if p.exists() {
                return Ok(p.to_string_lossy().into_owned());
            }
        }
    }
    warn!("byedpi binary not found — tried exe-relative and project/vendor paths; will attempt to use vendor/byedpi/ciadpi");
    Ok(project_root
        .join("vendor/byedpi/ciadpi")
        .to_string_lossy()
        .into_owned())
}
