use std::env;
use std::path::PathBuf;

/// Compile the vendored `hev-socks5-tunnel` C stack (TUN→SOCKS5 forwarder)
/// into `libunblocker_core.so` when targeting Android. The GUI's VpnService
/// allocates a TUN fd; hev forwards its packets to our in-process SOCKS
/// endpoint. Source vendored at `vendor/tun2socks-c/` (kept from the
/// `tun2socks` crate, which itself only ships Linux/Apple build scripts).
///
/// All flags mirror the upstream `Android.mk` + `configs.mk` files so the
/// result matches an ndk-build (ndk-build itself is unusable here because the
/// repo path contains spaces, which the NDK rejects).
fn main() {
    println!("cargo:rerun-if-changed=../../vendor/tun2socks-c");
    println!("cargo:rerun-if-changed=../../vendor/byedpi");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        return;
    }

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // ── byedpi DPI desync engine (static lib) ──────────────────────────
    let byedpi_dir = manifest.join("../../vendor/byedpi");
    let mut byedpi = cc::Build::new();
    byedpi
        .include(&byedpi_dir)
        .define("FAKE_SUPPORT", Some("1"))
        .define("TIMEOUT_SUPPORT", Some("1"))
        .define("__linux__", None)
        .flag_if_supported("-fvisibility=hidden")
        .file(byedpi_dir.join("proxy.c"))
        .file(byedpi_dir.join("desync.c"))
        .file(byedpi_dir.join("conev.c"))
        .file(byedpi_dir.join("extend.c"))
        .file(byedpi_dir.join("packets.c"))
        .file(byedpi_dir.join("mpool.c"))
        .file(byedpi_dir.join("main.c"))
        // Library shim (our thin wrapper exposing start/run/stop/cleanup).
        .file(manifest.join("src/desync/byedpi_shim.c"));
    byedpi.compile("byedpi");

    // ── hev-socks5-tunnel (existing) ───────────────────────────────────
    let vendor = manifest.join("../../vendor/tun2socks-c");
    let base = vendor.join("third-part");

    // hev-task-system (event loop / coroutines).
    let mut task = cc::Build::new();
    task.include(base.join("hev-task-system/src"))
        .include(base.join("hev-task-system/include"))
        .define("ENABLE_STACK_OVERFLOW_DETECTION", None)
        .define("ENABLE_MEMALLOC_SLICE", None)
        .define("ENABLE_IO_SPLICE_SYSCALL", None)
        .define("CONFIG_STACK_BACKEND", "STACK_MMAP")
        .define("CONFIG_STACK_OVERFLOW_DETECTION", "1")
        .define("CONFIG_MEMALLOC_SLICE_ALIGN", "64")
        .define("CONFIG_MEMALLOC_SLICE_MAX_SIZE", "4096")
        .define("CONFIG_MEMALLOC_SLICE_MAX_COUNT", "1000") // NOLINT
        .define("CONFIG_SCHED_CLOCK", "CLOCK_NONE")
        .flag_if_supported("-fvisibility=hidden")
        ;
    add_tree(&mut task, base.join("hev-task-system/src"));
    task.compile("hev-task-system");

    // yaml (config parser).
    let mut yaml = cc::Build::new();
    yaml.include(base.join("yaml/src"))
        .define("YAML_VERSION_MAJOR", "0")
        .define("YAML_VERSION_MINOR", "2")
        .define("YAML_VERSION_PATCH", "5")
        .define("YAML_VERSION_STRING", "\"0.2.5\"")
        ;
    add_tree(&mut yaml, base.join("yaml/src"));
    yaml.compile("yaml");

    // lwip (remote TCP/UDP handling inside the tunnel).
    let mut lwip = cc::Build::new();
    lwip.include(base.join("lwip/src/include"))
        .include(base.join("lwip/src/ports/include"))
        .define("FD_SET_DEFINED", None)
        .define("SOCKLEN_T_DEFINED", None)
        ;
    add_tree(&mut lwip, base.join("lwip/src"));
    lwip.compile("lwip");

    // hev-socks5-tunnel itself.
    let mut tun = cc::Build::new();
    tun.include(vendor.join("src"))
        .include(vendor.join("src/misc"))
        .include(vendor.join("src/core/include"))
        .include(base.join("yaml/include"))
        .include(base.join("lwip/src/include"))
        .include(base.join("lwip/src/ports/include"))
        .include(base.join("hev-task-system/include"))
        .define("FD_SET_DEFINED", None)
        .define("SOCKLEN_T_DEFINED", None)
        .define("ENABLE_LIBRARY", None)
        ;
    add_tree(&mut tun, vendor.join("src"));
    tun.compile("hev-socks5-tunnel");
}

fn add_tree(build: &mut cc::Build, dir: PathBuf) {
    let mut files: Vec<_> = walkdir_files(&dir, "c");
    files.extend(walkdir_files(&dir, "S"));
    files.sort();
    for f in files {
        build.file(f);
    }
}

fn walkdir_files(dir: &PathBuf, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walkdir_files(&p, ext));
            } else {
                let is_ext = p
                    .extension()
                    .map(|e| e == ext)
                    .unwrap_or(false);
                if is_ext {
                    out.push(p);
                }
            }
        }
    }
    out
}
