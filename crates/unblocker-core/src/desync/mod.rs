//! DPI desync engine — embedded byedpi (ciadpi) as a static C library.
//!
//! The engine listens on a local SOCKS5 port and applies DPI evasion
//! techniques (split, disorder, fake, tlsrec, oob) to outgoing TCP
//! connections. Traffic is routed: `hev tunnel → SOCKS5 → byedpi → internet`.
//!
//! Only one instance may run per process (byedpi uses global state).

use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use anyhow::{anyhow, Result};
use tracing::info;

// ── C FFI declarations (byedpi_shim.c) ─────────────────────────────────────

extern "C" {
    /// Parse CLI arguments + create listening socket. Returns fd (>= 0) or -1.
    fn byedpi_start(argc: c_int, argv: *mut *mut c_char) -> c_int;
    /// Run the event loop. Blocks until byedpi_stop() is called.
    fn byedpi_run(fd: c_int) -> c_int;
    /// Signal the event loop to stop. Thread-safe.
    fn byedpi_stop();
    /// Free all byedpi-allocated memory. Must be called after byedpi_run returns.
    fn byedpi_cleanup();
}

// ── Rust wrapper ────────────────────────────────────────────────────────────

static RUNNING: AtomicBool = AtomicBool::new(false);

/// Handle to a running byedpi DPI desync engine.
pub struct DesyncEngine {
    thread: Option<JoinHandle<()>>,
    port: u16,
    /// Keep CString data alive — byeDPI's params.protect_path points into this.
    _args: Vec<CString>,
}

impl DesyncEngine {
    /// Start the desync engine on `port` with the given CLI args string.
    ///
    /// `args` is a space-separated string of byedpi CLI flags, e.g.
    /// `"--disorder 1 --tlsrec 1+s --conn-ip 0.0.0.0"`.
    ///
    /// The engine listens on `127.0.0.1:<port>` as a SOCKS5 server.
    /// Returns once the engine is listening (or on startup error).
    pub fn start(args: &str, port: u16) -> Result<Self> {
        if RUNNING.swap(true, Ordering::SeqCst) {
            return Err(anyhow!("desync engine already running"));
        }

        // Build argv from the args string + our listen address.
        let ip = CString::new("127.0.0.1").unwrap();
        let port_str = CString::new(port.to_string()).unwrap();
        let args_vec = build_argv(args, &ip, &port_str)?;
        let argc = args_vec.len() as c_int;
        let mut argv: Vec<*mut c_char> = args_vec.iter().map(|s| s.as_ptr() as *mut c_char).collect();
        argv.push(std::ptr::null_mut()); // NULL sentinel

        // Create the listening socket (non-blocking, quick).
        let fd = unsafe { byedpi_start(argc, argv.as_mut_ptr()) };
        if fd < 0 {
            RUNNING.store(false, Ordering::SeqCst);
            return Err(anyhow!("byedpi_start failed (fd={fd})"));
        }

        info!(port, fd, "desync engine: listening on 127.0.0.1:{port}");

        // Run the event loop on a dedicated thread.
        let thread = std::thread::Builder::new()
            .name("byedpi-engine".into())
            .spawn(move || {
                let rc = unsafe { byedpi_run(fd) };
                unsafe { byedpi_cleanup() };
                RUNNING.store(false, Ordering::SeqCst);
                if rc != 0 {
                    tracing::warn!("byedpi event loop exited rc={rc}");
                }
            })?;

        // Brief wait to catch immediate startup errors.
        std::thread::sleep(std::time::Duration::from_millis(200));
        if !RUNNING.load(Ordering::SeqCst) {
            return Err(anyhow!("byedpi exited immediately after start"));
        }

        Ok(Self {
            thread: Some(thread),
            port,
            _args: args_vec,
        })
    }

    /// SOCKS5 URL for routing traffic through this engine.
    pub fn socks_url(&self) -> String {
        format!("socks5://127.0.0.1:{}", self.port)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn is_alive(&self) -> bool {
        RUNNING.load(Ordering::SeqCst)
            && self
                .thread
                .as_ref()
                .map(|t| !t.is_finished())
                .unwrap_or(false)
    }

    /// Stop the engine and join its thread.
    pub fn stop(&mut self) {
        if self.thread.is_some() {
            unsafe { byedpi_stop() }
            if let Some(t) = self.thread.take() {
                let deadline =
                    std::time::Instant::now() + std::time::Duration::from_secs(5);
                while !t.is_finished() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                if !t.is_finished() {
                    tracing::warn!("byedpi engine did not stop within 5s");
                }
            }
            RUNNING.store(false, Ordering::SeqCst);
            info!("desync engine stopped");
        }
    }
}

/// Build an argv array from a space-separated args string plus explicit
/// `--ip` and `--port` flags.
fn build_argv(args: &str, ip: &CString, port: &CString) -> Result<Vec<CString>> {
    let mut v: Vec<CString> = Vec::new();
    // argv[0] is the program name — getopt_long skips it.
    // Must be present or getopt starts at argv[1] and misses our options.
    v.push(CString::new("ciadpi").unwrap());
    // Always set listen address explicitly.
    v.push(CString::new("--ip").unwrap());
    v.push(ip.clone());
    v.push(CString::new("--port").unwrap());
    v.push(port.clone());
    // Skip daemon mode (not applicable in library).
    // Tokenize the user-supplied args.
    for tok in args.split_whitespace() {
        if tok == "-D" || tok == "--daemon" || tok == "-w" || tok == "--pidfile" {
            continue;
        }
        v.push(
            CString::new(tok.to_owned())
                .map_err(|e| anyhow!("invalid arg '{tok}': {e}"))?,
        );
    }
    Ok(v)
}
