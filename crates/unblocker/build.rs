fn main() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let vendor = manifest.join("../../vendor/byedpi");
    let ciadpi = vendor.join("ciadpi");
    if vendor.join("Makefile").exists() {
        if !ciadpi.exists() {
            println!("cargo:warning=building vendor/byedpi/ciadpi via make...");
            let out = std::process::Command::new("make")
                .arg("-C")
                .arg(&vendor)
                .arg("-j4")
                .output();
            match out {
                Ok(o) if o.status.success() => println!("cargo:warning=byedpi built"),
                Ok(o) => println!("cargo:warning=byedpi make failed: {}", String::from_utf8_lossy(&o.stderr)),
                Err(e) => println!("cargo:warning=byedpi make spawn failed: {e}"),
            }
        }
        if ciadpi.exists() {
            let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
            let dest_dir = out_dir.join("vendor/byedpi");
            let _ = std::fs::create_dir_all(&dest_dir);
            let dest = dest_dir.join("ciadpi");
            let _ = std::fs::copy(&ciadpi, &dest);
            // Also copy next to target/debug|release for exe-relative lookup
            if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
                let _ = std::process::Command::new("sh").arg("-c").arg(format!("mkdir -p {}/vendor/byedpi && cp -f {} {}/vendor/byedpi/ciadpi 2>/dev/null || true", target_dir, ciadpi.display(), target_dir)).output();
            } else {
                // heuristic: OUT_DIR is like .../target/debug/build/unblocker-xxx/out
                let mut p = out_dir.as_path();
                for _ in 0..5 { if let Some(par) = p.parent() { p = par; } }
                // p now roughly .../target
                let _ = std::process::Command::new("sh").arg("-c").arg(format!("for d in {}/debug {}/release; do mkdir -p $d/vendor/byedpi && cp -f {} $d/vendor/byedpi/ciadpi 2>/dev/null || true; done", p.display(), p.display(), ciadpi.display())).output();
            }
        }
    }
    println!("cargo:rerun-if-changed=../../vendor/byedpi/ciadpi");
    println!("cargo:rerun-if-changed=../../vendor/byedpi/Makefile");
}
