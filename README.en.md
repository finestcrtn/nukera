# Nukera

[Русский](README.ru.md) · [English](README.en.md)

## What it is

![Nukera](assets/nukera_img.png)

Nukera is a GUI app based on Zapret’s principles and other ways of bypass, designed to unblock **Instagram, Telegram, YouTube, Discord**, and all other blocked websites — even those that other Zapret alternatives cannot unblock. Best of all, it requires no manual configuration: Just turn it on, and it works.

## Why Nukera

- **Just turn it on.** Open it, press ▶, done. No terminal, no config files, no manual proxy setup.
- **Telegram proxy built in.** The Telegram app connects through it automatically — nothing to configure.
- **Works where other tools give up.** Instagram Reels, WhatsApp and Telegram — right away, no fiddling with settings.
- **Free, local, private.** Everything runs on your machine; no data is sent anywhere.
- **Open source.** Rust core + Flutter GUI.

## Install — 30 seconds

1. Download **`Nukera-Linux-x86_64.AppImage`** from [Releases](https://github.com/finestcrtn/nukera/releases/latest).
2. Double-click it.
3. Accept **one** password prompt on first launch — Nukera installs itself and opens. After that it works without prompts.



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
