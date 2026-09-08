# Project Map — Nukera (unblocker)

A one-toggle, system-wide censorship-circumvention tool for Russian DPI (TSPU) + IP blocks.
Standalone app. **Rust core engine + Flutter GUI**. Linux (Arch/Manjaro packaged; other distros via `scripts/unblocker-install.sh`).

---

## 1. Architecture

```
GUI button → systemctl start/stop unblocker.service → systemd runs unblocker daemon (root)
                                                     ↓
                                            nfqws NFQUEUE + nft + /etc/hosts + TG proxy
```

**One engine, two ways to control it:**
- **GUI** (`/usr/lib/nukera/unblocker-gui`, wrapper `unblocker-gui`) — Flutter app, calls `systemctl start/stop`
- **CLI** (`unblocker enable/disable/daemon/status`) — package installs to `/usr/bin/unblocker` + bundled copy at `/usr/lib/nukera/unblocker`

**Privilege escalation:** user-agnostic polkit rules auto-approve `systemctl` for `unblocker.service` (any active local user, no username pin). No password prompts after first install.

### Bypass layers (all active via one toggle)

1. **DPI desync** — `nfqws` (bol-van/zapret) NFQUEUE + `.strat` strategies
2. **IP/hosts override** — `/etc/hosts` maps blocked domains to working IPs
   - Static hosts: Telegram Web (`config/hosts/telegram-web.txt` → `149.154.167.220`)
   - Magila CDN: `config/hosts/magila-cdn.txt` (Meta/Instagram edge IPs)
3. **nsswitch fix** — swaps `resolve` before `files` to `files` before `resolve` so `/etc/hosts` is consulted before systemd-resolved
4. **Telegram native proxy** — redirect of TG port-80 traffic to port 1444 (native HTTPS) via `nft` nat rules

---

## 2. Workspace Structure

```
nukera/
├── Cargo.toml / Cargo.lock          # workspace — 2 crates
├── project.md                       # this map
├── AGENTS.md                        # workflow rules
├── crates/
│   ├── unblocker/                   # CLI binary
│   │   └── src/main.rs
│   └── unblocker-core/              # core engine (libunblocker_core.so + rlib)
│       └── src/
│           ├── lib.rs
│           ├── api.rs               # C FFI surface
│           ├── system.rs            # hosts, state, proxy, nsswitch, running_as_root
│           ├── platform/
│           │   ├── mod.rs           # NetworkDriver trait
│           │   ├── linux.rs         # nfqws + nft + hosts + TG proxy
│           │   └── stub.rs          # non-Linux stub
│           ├── telegram/            # native Rust TG WSS proxy
│           ├── zapret/              # nfqws/nft management, strategy loader, probe
│           ├── config/              # AppConfig, host overrides
│           ├── dns/                 # DoH resolver
│           ├── health/              # health checks
│           └── ip_discovery/        # IP probing, CDN discovery
├── unblocker_gui/                   # Flutter GUI (Linux only)
│   ├── lib/
│   │   ├── main.dart
│   │   ├── src/app.dart
│   │   ├── src/home_page.dart       # one-button toggle (calls systemctl)
│   │   └── src/native.dart          # FFI bridge
│   └── linux/                       # Linux runner
├── config/
│   ├── hosts/
│   │   ├── telegram-web.txt         # Telegram Web hosts (34 entries)
│   │   └── magila-cdn.txt           # Meta CDN edge IPs
│   └── zapret/                      # strategies (.strat, pool/, byebyedpi/) + hostlists (.sites)
├── scripts/
│   ├── unblocker-install.sh         # dev fallback installer (new /usr/lib/nukera layout)
│   ├── build-arch-pkg.sh            # makepkg wrapper → unblocker-x.y.z-x86_64.pkg.tar.zst
│   ├── build-appimage.sh            # manual type-2 assembly → Nukera-x86_64.AppImage
│   ├── unblocker-kill.sh            # emergency cleanup (systemctl stop + nft flush)
│   ├── unblocker-root-cleanup.sh    # root helper (pkexec helper, listed in polkit rules)
│   └── ... (verify, safe-test, fetch-zapret)
├── vendor/
│   └── zapret/                      # nfqws binary (tpws removed)
├── packaging/
│   ├── arch/                        # PKGBUILD + unblocker.install (pacman hooks)
│   ├── polkit/50-unblocker.rules    # user-agnostic polkit rules (+ reload-daemon)
│   ├── systemd/unblocker.service    # daemon unit (packaged to /usr/lib/systemd/system/)
│   └── appimage/                    # AppRun (install/launch/uninstall) + nukera.desktop
├── assets/hosts/                    # curated CDN hosts
└── docs/                            # methodology
```

Deleted as part of release cleanup: `vendor/byedpi/`, `vendor/zapret/tpws`, `config/lists/`, `config/hosts/geohide-hosts.txt`, `assets/hosts/hosts-{instagram,grok,imo}.txt`, `scripts/unblocker-firstrun.sh`, `scripts/install-system.sh`.

---

## 3. The Crates

### 3.1 `crates/unblocker-core` — the engine

Compiles as `cdylib` (`libunblocker_core.so`) for FFI + `rlib` for CLI.

| Module | Responsibility |
|---|---|
| `api.rs` | C FFI: `enable`/`disable`/`is_enabled`/`get_tg_url`/`run_diagnostics` |
| `system.rs` | Hosts apply/remove, state file, proxy, nsswitch fix, static hosts loader, `running_as_root()` (gsettings skip) |
| `platform/mod.rs` | `NetworkDriver` trait + process-global driver |
| `platform/linux.rs` | nfqws NFQUEUE + nft + hosts + TG proxy |
| `telegram/` | Native Rust MTProto↔WebSocket proxy |
| `zapret/` | nfqws/nft management, strategy loader, probe, sites |
| `config/` | `AppConfig`, host overrides, config loading |
| `dns/` | DoH resolver |
| `health/` | health checks |
| `ip_discovery/` | IP probing, CDN discovery |

### 3.2 `crates/unblocker` — CLI binary

Subcommands: `daemon` / `enable` / `disable` / `status` / `probe` / `hosts` / `install-discover` / `refresh` / `diag`

- `enable`/`disable` → `systemctl start/stop unblocker.service`
- `daemon` → nfqws + nft + hosts + TG proxy (runs as root under systemd)
- `run_autotune`/`parse_socks` dead code removed; strategy pool resolved from `/etc/unblocker/strategies/pool` first, dev tree fallback

### 3.3 `unblocker_gui/` — Flutter UI

- `lib/main.dart` — entry point
- `lib/src/home_page.dart` — one-button Enable/Disable toggle, calls `systemctl start/stop`
- `lib/src/native.dart` — FFI bridge for diagnostics

---

## 4. Bundled App (`/usr/lib/nukera/`)

```
/usr/lib/nukera/
├── unblocker-gui          # Flutter binary
├── unblocker              # CLI binary (bundled; unit uses this path)
├── unblocker-kill.sh      # Emergency cleanup (systemctl stop)
├── unblocker-root-cleanup.sh  # Root helper (pkexec, polkit-approved)
├── lib/                   # Flutter runtime libs (libunblocker_core.so, libflutter_linux_gtk.so, libapp.so)
└── data/                  # nukera_icon.png
```

DPI engine `nfqws` lives separately at `/usr/lib/unblocker/nfqws` (file capabilities re-applied on every install/upgrade).

GUI resolves its own directory via `Platform.resolvedExecutable` and calls local binaries.

---

## 5. Privilege Escalation — how it works without password prompts

Goal: **never show a password dialog to the user after install.**

GUI/CLI never run root commands directly. Everything is delegated to a **systemd service** running as root:

```
GUI click → systemctl start unblocker.service → systemd runs unblocker daemon (root)
                                                    ↓
                                            nfqws + nft + hosts + TG proxy
```

### Three layers that make it passwordless

**1. Polkit rules** (`/etc/polkit-1/rules.d/50-unblocker.rules`) — user-agnostic.
Normally `subject.user` is pinned; we drop the pin and use `subject.local && subject.active`, so no username leaks into install scripts on any machine.
- auto-approves `org.freedesktop.systemd1.manage-units/unit-files` for `unblocker.service`
- auto-approves `org.freedesktop.policykit.exec` for the pkexec helper list: `/usr/lib/nukera/unblocker-root-cleanup.sh`, nft, cp, pkill, rm

**2. Systemd service** (`/usr/lib/systemd/system/unblocker.service`, packaged; shadowing stale `/etc/systemd/system/unblocker.service` purged on install/upgrade)
```ini
[Service]
ExecStart=/usr/lib/nukera/unblocker daemon
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW CAP_DAC_OVERRIDE CAP_SETPCAP CAP_SYS_ADMIN
KillSignal=SIGTERM          # daemon owns cleanup (nft flush, hosts strip)
TimeoutStopSec=15
```

**3. File capabilities** on `nfqws` (`setcap …=+ep`) — allows non-root spawn, no pkexec.

No sudoers entries installed anymore — emergency cleanup uses `pkexec` (covered by the polkit exec rules) as fallback when systemd is unavailable.

### How to add a new privileged operation

1. **Don't use pkexec directly** — it shows a dialog
2. **Add it to the systemd service** — the daemon runs as root
3. **Or add to `unblocker-root-cleanup.sh`** + list it in the polkit exec allowlist

---

## 6. Config / state files

| Path | Purpose |
|---|---|
| `~/.config/unblocker/config.json` | `AppConfig` (DoH, SOCKS port, profile, hosts) |
| `/tmp/unblocker.state` | runtime state — `is_enabled()` checks this |
| `/etc/unblocker/strategies/*.strat` | DPI strategies |
| `/etc/unblocker/strategies/pool/` (+ `byebyedpi/`) | strategy pool resolved first (dev fallback: `config/zapret/strategies/pool`) |
| `/etc/unblocker/hostlists/*.sites` + `auto.txt` + `reestr.txt` | nfqws hostlists; `reestr.txt` downloaded at package install |
| `/etc/unblocker/sites/*.conf` | per-site winning strategy |
| `/etc/unblocker/tg-proxy.secret` / `.url` | persistent TG secret + proxy URL |
| `/etc/polkit-1/rules.d/50-unblocker.rules` | passwordless polkit (user-agnostic) |
| `/usr/lib/systemd/system/unblocker.service` | systemd unit (packaged) |

---

## 7. Build & install

### Dev build + install (any distro)

```bash
cargo build --release
(cd unblocker_gui && flutter build linux --release)
# installs to /usr/lib/nukera + /usr/lib/unblocker/nfqws + /etc config
scripts/unblocker-install.sh
```

Install script does: bundle CLI + scripts into `/usr/lib/nukera`; wrapper in `PREFIX/bin`; polkit rules; systemd unit (dev: `/etc/systemd/system/`); strategies + hostlists; file caps on nfqws; `install-discover`.

### Arch/Manjaro package (one file)

```bash
scripts/build-arch-pkg.sh          # locates flutter, makepkg -Ccf
# → unblocker-0.1.0-1-x86_64.pkg.tar.zst
sudo pacman -U ./unblocker-0.1.0-1-x86_64.pkg.tar.zst   # or pamac double-click
```

- `PKGBUILD` (`packaging/arch/`) builds Rust + Flutter, stages full payload; `options=('!lto')` (ring fails fat-LTO), `install=unblocker.install`
- `unblocker.install` hooks: re-`setcap`, daemon-reload, enable, purge stale `/etc` unit, reestr download, `install-discover` (≤240s), pre_remove `disable --now`, post_remove nuclear nft flush + hosts strip + state rm
- Deps (runtime): gtk3 nftables polkit psmisc glib2 curl ca-certificates xdg-utils libcap

### AppImage (portable, any distro)

```bash
scripts/build-appimage.sh          # needs cargo + flutter + squashfs-tools
# → Nukera-<rev>-x86_64.AppImage
./Nukera-xxxx.AppImage             # first run installs + launches
./Nukera-xxxx.AppImage --uninstall # stops daemon, removes system install
./Nukera-xxxx.AppImage --verify    # prints detected layout + deps, no root
```

- **One universal AppImage, layout chosen at runtime** — no per-distro variants. Detect: `findmnt -no OPTIONS /usr` shows `ro` → **immutable** (Fedora Silverblue, SteamOS-like) else **standard**
  - standard → `/usr/lib/nukera` exact Arch-package layout (unit + desktop + icon into `/usr`)
  - immutable → `/var/lib/nukera` + `/var/lib/unblocker`, unit + polkit into `/etc`, desktop into `~/.local/share/applications` (user-write, no root); `/usr/lib` paths rewritten via `sed` while staging
- **Static type2-runtime + `mksquashfs -comp zstd`** (manual `runtime || squashfs` assembly — same layout appimagetool produces). Static/musl runtime = no libfuse2 dependency → works on Debian 12/24.04/Fedora that lack libfuse.so.2. Verified: `ldd` → static-pie.
- AppRun `do_install`: stages `payload/rootfs` into `$TMPDIR` as the mount owner first (pkexec'd root cannot read the user-owned FUSE mount), then a single `pkexec cp -a` writes the whole tree to `/` (cp is in the polkit exec allowlist); polkit rule ships inside the copied tree so `daemon-reload`/`enable` afterwards are silent
- SELinux: on Enforcing + immutable, `restorecon -RF /var/lib/nukera /var/lib/unblocker` after copy (Fedora/Silverblue)
- Gates: `systemctl` + `pkexec` missing → clear message + exit (non-systemd / headless)
- glibc < 2.39 → warn at install/launch (build host is rolling); musl → warn
- Uninstall (both layouts): `systemctl disable --now` (daemon SIGTERM flushes nft/hosts) → `pkexec rm -rf` app dirs + both unit paths → reload → rule last → user desktop removed (no root)

**Supported matrix**: full Arch/Manjaro/CachyOS · Fedora ≥ 39 (+ Silverblue immutable) · Ubuntu 24.04 LTS · Debian 13 · openSUSE Tumbleweed; warn-only Debian 12/Ubuntu 22.04 (glibc 2.34–2.38); unsupported musl, non-systemd, no-polkit.

---

## 8. Static Hosts (always-on)

### Telegram Web (`config/hosts/telegram-web.txt`)
34 domains → `149.154.167.220` (Telegram DC2 edge, not blocked by RKN).
Includes: web.telegram.org, api.telegram.org, t.me, telegram.org, all wss/zws/kws subdomains.

### Magila CDN (`config/hosts/magila-cdn.txt`)
Meta/Instagram CDN edge IPs; candidates resolved at runtime (packaged dir first, dev tree fallback).

`hosts-{instagram,grok,imo}.txt` and `geohide-hosts.txt` deleted (stale/community).

### How it works
1. On enable, `hosts_apply()` merges config overrides + static hosts
2. Writes entries to `/etc/hosts` between `# BEGIN unblocker` / `# END unblocker` markers
3. nsswitch.conf is fixed: `files` before `resolve` so `/etc/hosts` is consulted before systemd-resolved
4. nfqws desyncs SNI so ISP doesn't see blocked domains in TLS handshake
5. Browser connects to working IP → ISP firewall passes → site loads

---

## 9. Key workflow rules

1. **Build and test new features immediately** — `cargo build --release`, `flutter build linux --release`, then test with real sites
2. **Keep docs in sync** — update `project.md` after every structural change
3. **Git snapshot before experiments** — commit the known-working state first
4. **Never leave the network broken** — `unblocker-kill.sh` for emergency cleanup (`bash /tmp/unblocker-kill.sh` on user report)
5. **Arch packaging** — rebuild package + reinstall after any payload change; verify off/on/off before handing to user