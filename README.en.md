# Nukera

[Русский](README.md) · [English](README.en.md)

<div align="center">

# 💾 DOWNLOAD — СКАЧАТЬ

<a href="https://github.com/finestcrtn/nukera/releases/download/v0.2.0/Nukera-Windows.exe"><img height="24" src="https://skillicons.dev/icons?i=windows" alt=""/> <strong>Windows</strong></a>
<a href="https://github.com/finestcrtn/nukera/releases/download/v0.2.0/Nukera-Android-arm64-v8a.apk"><img height="24" src="https://skillicons.dev/icons?i=androidstudio" alt=""/> <strong>Android</strong></a>
<a href="https://github.com/finestcrtn/nukera/releases/download/v0.2.0/Nukera-Linux-x86_64.AppImage"><img height="24" src="https://skillicons.dev/icons?i=linux" alt=""/> <strong>Linux</strong></a>
<a href="https://github.com/finestcrtn/nukera/releases/download/v0.2.0/Nukera-MacOS.dmg"><img height="24" src="https://skillicons.dev/icons?i=apple" alt=""/> <strong>MacOS</strong></a>

</div>

## What it is

<p align="center">
  <img width="300" src="assets/nukera_img.png" alt="Nukera" />
</p>

**Nukera is the best and easiest version of Zapret/ByeDPI: it unblocks more sites than any alternative, and takes the least time to set up.**

Nukera is a GUI app based on Zapret’s principles and other ways of bypass, designed to unblock **Instagram, Telegram, YouTube, Discord**, and all other blocked websites — even those that other Zapret alternatives cannot unblock. Just turn it on, and it works.

No strategy picking, configs, terminals or guides. Instead of one fragile method it runs a full strategy pool — `nfqws` DPI-desync strategies, ByeDPI-style tricks, a built-in WebSocket proxy for the Telegram app, and a DoH resolver with IP maps — so it unlocks sites on networks where other tools fail. Everything runs locally, nothing is sent anywhere, and all of it is open source.

Nukera is cross-platform: Windows, MacOS, Linux and Android — the same app, the same interface, working the same way on every device.

## Why Nukera

- **Just turn it on.** Everything works right away — like a VPN, but free and private. No terminal, no config files, no manual proxy setup.
- **Telegram proxy built in.** The Telegram app connects through it automatically — nothing to configure.
- **Works where other tools give up.** Instagram Reels, WhatsApp and Telegram — right away, no fiddling with settings.
- **Free, local, private.** Everything runs on your machine; no data is sent anywhere.
- **Open source.** Rust core + Flutter GUI.
- **Cross-platform.** Windows, MacOS, Linux, Android — one app, one way of working, the same result on every OS.
<p align="center">
  <img width="400" alt="Image" src="https://github.com/user-attachments/assets/6ca7f2b6-b4ba-41f4-a06e-a7af56927057" />
  <img width="400" alt="Image" src="https://github.com/user-attachments/assets/8060c040-69f4-4fa7-9847-da16747d283a" />
</p>

## Install — 30 seconds

- **Windows:** download **`Nukera.exe`** and run it.
- **Android:** install **`Nukera-arm64-v8a.apk`** (allow installing from unknown sources).
- **Linux:** run **`Nukera-Linux-x86_64.AppImage`** by double-click and accept **one** password prompt on first launch — after that it works without prompts.
- **MacOS:** double-click **`Nukera-MacOS.dmg`** and in the window that opens, click on Install Nukera.command to install Nukera into the Applications folder. After the first launch, accept **one** password prompt — after that it runs without prompts.

##  Security — antivirus false positives


During my tests, Windows Defender on my PC deletes the downloaded .exe file, flagging it as Trojan:Script/Wacatac.H!ml:

Short answer: This is a known false positive; the application contains no malware. I have already submitted a request to Microsoft for a review to have this marked as a false positive at: Submission ID: bd322a22-89b6-417f-8a91-765a41bb8e0d

The !ml suffix in the threat name stands for Machine Learning and heuristic analysis. It means the file was flagged by automated algorithms rather than an exact match with a database of known viruses. Windows Defender automatically flags new files if they meet three specific criteria:

   1. Lack of a digital signature: Purchasing an expensive commercial certificate is impractical for a free, open-source project.
   2. Low file reputation network-wide: The file is brand new, and Microsoft has not yet gathered enough download statistics for it.
   3. Operational specifics: Our application functions as a CLI tool, interacts with system commands, and modifies the PATH environment variable—specifically unpacking data into AppData upon its first launch. To the antivirus AI, this behavior looks suspicious at first glance, causing it to overprotect.

Why you can trust the project:

* Completely open-source: The entire project code is publicly available in this repository. You can personally verify every single line.
* No signature matches: Not a single antivirus program in the world finds actual signatures (digital fingerprints) of real malware within our application's code.

How to run the application if Windows blocked it:

   1. Open Windows Security -> Virus & threat protection -> Protection history.
   2. Find the blocked file, click the Actions button, and select Allow on device. Then download it again.
   3. Open Windows Security.
   4. Click Virus & threat protection.
   5. Click Manage settings.
   6. Toggle the Real-time protection switch to Off.
   7. Click Yes in the confirmation prompt.
   8. Open the application. If SmartScreen blocks the window from launching, click More info, and then click Run anyway. Then activate the bypass.
   9. After launching, you can turn Real-time protection back on; the application has already unpacked its data and will no longer trigger Defender.


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


## From source (for developers)

```bash
# needs: cargo (rustup), Flutter stable, squashfs-tools
scripts/build-appimage.sh
```

## What's inside

Rust core + Flutter (GTK) GUI · `nfqws` (zapret) DPI desync with strategy pool · native Telegram WebSocket proxy · DoH resolver + curated hosts IP maps · systemd daemon with passwordless polkit.

## Help

`Nukera-Linux-x86_64.AppImage --verify` — check what your system supports before installing.

## Based on

- Zapret
- tg-ws-proxy
- ByeDPI

Nukera wouldn't exist without these projects.
