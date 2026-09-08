# Nukera

[English](README.md) · [Русский](README.ru.md)

![Cross-Distro Build](https://github.com/finestcrtn/nukera/actions/workflows/distro-test.yml/badge.svg)

## What it is

Nukera unblocks websites blocked by your ISP or state censorship:

**Instagram (incl. Reels), Telegram, YouTube, Discord, WhatsApp** — and other blocked sites. Out of the box.

## Why Nukera

- **Just works.** Launch, click ▶, done. No terminal, no config files, no manual proxy setup.
- **Telegram proxy built in.** The Telegram app connects through Nukera's built-in proxy automatically — no separate setup needed.
- **Works where other tools give up.** Instagram Reels, WhatsApp and Telegram are handled by default — no fiddling with per-site settings.
- **Free, local, private.** Runs 100% on your machine. Nothing is sent anywhere.
- **Open source.** Rust core + Flutter GUI.

## Install — 30 seconds

1. Download **`Nukera-Linux-x86_64.AppImage`** from [Releases](https://github.com/finestcrtn/nukera/releases/latest).
2. Double-click it.
3. Accept **one** password prompt on first launch — Nukera installs itself and opens. After that it works without prompts.

- **Update:** download the new AppImage, double-click it.
- **Remove:** run the AppImage with `--uninstall`.

## Where it may not work

- Requires Linux with **systemd** and a recent glibc (≥ 2.39): Arch/Manjaro, Fedora ≥ 39 (incl. Silverblue), Ubuntu 24.04 LTS, Debian 13, openSUSE Tumbleweed.
- Older Ubuntu 22.04 / Debian 12 may not start — a warning is shown.
- Some ISPs still block some sites; no tool can guarantee every site on every network.
- No Windows / macOS / mobile build yet.

## From source (for developers)

```bash
# needs: cargo (rustup), Flutter stable, squashfs-tools
scripts/build-appimage.sh
```

## What's inside

Rust core + Flutter (GTK) GUI · `nfqws` (zapret) DPI desync with strategy pool · native Telegram WebSocket proxy · DoH resolver + curated hosts IP maps · systemd daemon with passwordless polkit.

## Help

`Nukera-Linux-x86_64.AppImage --verify` — check what your system supports before installing.