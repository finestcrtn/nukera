# Nukera (unblocker) — One-Toggle DPI + IP-Block Bypass

Universal bypass for Russia: DPI desync (TSPU) + IP/DNS blocks. Linux (Arch/Manjaro packaged). Rust core + Flutter GUI.

## What it does
- DPI: `nfqws` (bol-van/zapret) NFQUEUE desync driven by `.strat` strategies (`hostfakesplit`/`quic`/`tlsrec`/`disorder`/misc pool) — one toggle, no per-site fuss
- IP/DNS: DoH + curated `hosts` IP maps for Telegram Web and Magila (Meta/Instagram) CDN edges — desync alone can't fix IP blocks
- Telegram: native Rust MTProto↔WebSocket proxy + nft redirect of TG port-80 to native HTTPS on `127.0.0.1:1444`
- Root daemon under systemd: enable/disable = `systemctl start/stop unblocker.service`, passwordless via user-agnostic polkit rules

## Install — Arch / Manjaro (one file)
```bash
# build on any Arch box (needs cargo + flutter in PATH)
scripts/build-arch-pkg.sh
sudo pacman -U ./unblocker-0.1.0-1-x86_64.pkg.tar.zst   # or pamac double-click
```
Package installs to `/usr/lib/nukera/` (GUI + bundled CLI), `/usr/lib/unblocker/nfqws` (DPI engine), `/usr/lib/systemd/system/unblocker.service`, config under `/etc/unblocker/`.

## Install — AppImage (portable, any Linux)
```bash
scripts/build-appimage.sh          # cargo + flutter + squashfs-tools
./Nukera-<rev>-x86_64.AppImage            # first run installs + launches
./Nukera-<rev>-x86_64.AppImage --uninstall  # remove (stops daemon, flushes nft/hosts)
./Nukera-<rev>-x86_64.AppImage --verify     # prints detected layout, no root
```
One universal AppImage — the launcher detects the host and picks a layout,
so there are no per-distro variants:

- **standard** (`/usr` writable): installs the same `/usr/lib/nukera` layout
  as the Arch package; one pkexec password prompt on first run.
- **immutable** (`/usr` mounted read-only, e.g. Fedora Silverblue): installs
  into `/var/lib/nukera`, unit into `/etc/systemd/system`, polkit rule into
  `/etc/polkit-1/rules.d`, launcher into the user's `~/.local/share/applications`.

Redownload + double-click to update (version marker written to the install
root). Built on a rolling glibc 2.39 host — on older distros a glibc warning
is printed at launch; unsupported (`glibc < 2.39`, non-systemd, no polkit)
is detected via `--verify`.

### Supported distro matrix
| Tier | Distros |
|---|---|
| Full (standard) | Arch / Manjaro / CachyOS, Fedora ≥ 39, Ubuntu 24.04 LTS, Debian 13, openSUSE Tumbleweed |
| Full (immutable) | Fedora Silverblue ≥ 39 |
| Unsupported (warn only) | glibc 2.34–2.38: Debian 12, Ubuntu 22.04/23.10 |
| Not supported | musl/Alpine, non-systemd, distros without polkit |

## Other distros (dev install)
```bash
cargo build --release
(cd unblocker_gui && flutter build linux --release)
scripts/unblocker-install.sh
```
Installs the same layout manually.

## CLI (no GUI needed)
```bash
unblocker daemon        # root daemon: nfqws + nft + hosts + TG proxy (systemd)
unblocker enable        # systemctl start unblocker.service
unblocker disable       # systemctl stop — flushes nft, strips /etc/hosts
unblocker status
unblocker install-discover   # one-shot IP discovery for your ISP
```

## GUI
`unblocker-gui` is a Flutter desktop app — Linux first, cross-platform ready.
Click ▶ to enable system-wide bypass (hosts + nfqws DPI desync + TG proxy),
■ to disable. The "Setup Telegram" button (visible only when enabled) opens
`tg://proxy` so you can add the proxy inside Telegram Desktop.

## Emergency cleanup
If the network ever breaks after enabling: `bash /tmp/unblocker-kill.sh` (or `unblocker disable`).
The daemon's SIGTERM handler flushes all `unblocker*` nft tables, strips the `/etc/hosts` block, and kills nfqws.

## Notes
- Package built with `options=('!lto')` — ring 0.17 fails fat-LTO linking.
- Stale `/etc/systemd/system/unblocker.service` (old pre-package install) is purged on install/upgrade so it can't shadow the packaged unit.
- Install-time hooks: download RKN `reestr` list, probe working IPs (`install-discover` ≤240s), `setcap` on nfqws, enable service.