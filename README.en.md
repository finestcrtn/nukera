# Nukera

[Русский](README.md) · [English](README.en.md)

<div align="center">

# 💾 DOWNLOAD — СКАЧАТЬ

<a href="https://github.com/finestcrtn/nukera/releases/download/v0.1.0/Nukera.exe"><img height="24" src="https://skillicons.dev/icons?i=windows" alt=""/> <strong>Windows</strong></a>
<a href="https://github.com/finestcrtn/nukera/releases/download/v0.1.0/Nukera-arm64-v8a.apk"><img height="24" src="https://skillicons.dev/icons?i=androidstudio" alt=""/> <strong>Android</strong></a>
<a href="https://github.com/finestcrtn/nukera/releases/download/v0.1.0/Nukera-Linux-x86_64.AppImage"><img height="24" src="https://skillicons.dev/icons?i=linux" alt=""/> <strong>Linux</strong></a>

</div>

## What it is

![Nukera](assets/nukera_img.png)

Nukera is a GUI app based on Zapret’s principles and other ways of bypass, designed to unblock **Instagram, Telegram, YouTube, Discord**, and all other blocked websites — even those that other Zapret alternatives cannot unblock. Best of all, it requires no manual configuration: Just turn it on, and it works.

Nukera is cross-platform: Windows, Linux and Android — the same app, the same interface, working the same way on every device.

## Why Nukera

- **Just turn it on.** Everything works right away — like a VPN, but free and private. No terminal, no config files, no manual proxy setup.
- **Telegram proxy built in.** The Telegram app connects through it automatically — nothing to configure.
- **Works where other tools give up.** Instagram Reels, WhatsApp and Telegram — right away, no fiddling with settings.
- **Free, local, private.** Everything runs on your machine; no data is sent anywhere.
- **Open source.** Rust core + Flutter GUI.
- **Cross-platform.** Windows, Linux, Android — one app, one way of working, the same result on every OS.

## Install — 30 seconds

- **Windows:** download **`Nukera.exe`** and run it.
- **Android:** install **`Nukera-arm64-v8a.apk`** (allow installing from unknown sources).
- **Linux:** run **`Nukera-Linux-x86_64.AppImage`** by double-click and accept **one** password prompt on first launch — after that it works without prompts.

## 🛡️ Security — antivirus false positives

Your antivirus (most often Windows Defender) may flag `Nukera.exe` as a threat. **That's a false positive — the app is 100% safe.**

Why it happens:

- **The app is unsigned.** I'm an open-source developer, and a code-signing certificate with Microsoft Store registration costs hundreds of dollars a year. Windows treats unsigned new executables as suspicious by default.
- **Familiar malware toolchain:** the Windows build is made with Flutter and packaged via PyInstaller and Inno Setup — the exact tools malware authors use. That's why scanner heuristics flag harmless apps built with the same stack.

Don't worry:

- The code is fully open-source — check it right in the repository or build the app yourself.
- Nukera **doesn't collect or send your data anywhere**. It only forwards your traffic to bypass censorship; everything stays on your device.

## How to use

- **All sites are unblocked by default** — nothing extra to set up.
- **Telegram in a browser just works**, out of the box.
- **For the Telegram app:** tap the **“Set up Telegram”** button — Telegram opens and you just accept the proxy. Do this once, and it works while Nukera is on. Do it again if the proxy stops working.
- **If some sites don't work:** tap **“Optimize” («Адаптировать под ваш интернет»)**. A strategy test runs, and in a few minutes Nukera picks the best one for your network. Only do this if it really doesn't work.

**Android, page won't load:** refresh it 3–5 times — it will load and open instantly afterwards. Make sure the app is actually on; if needed, force-stop it and restart.

## Where it may not work

- Requires Linux with **systemd** and a recent glibc (≥ 2.39): Arch/Manjaro, Fedora ≥ 39 (incl. Silverblue), Ubuntu 24.04 LTS, Debian 13, openSUSE Tumbleweed.
- Older Ubuntu 22.04 / Debian 12 may not start — a warning is shown.
- Some ISPs still block some sites; no tool can guarantee every site on every network.
- No macOS build yet.

## From source (for developers)

```bash
# needs: cargo (rustup), Flutter stable, squashfs-tools
scripts/build-appimage.sh
```

## What's inside

Rust core + Flutter (GTK) GUI · `nfqws` (zapret) DPI desync with strategy pool · native Telegram WebSocket proxy · DoH resolver + curated hosts IP maps · systemd daemon with passwordless polkit.

## Help

`Nukera-Linux-x86_64.AppImage --verify` — check what your system supports before installing.