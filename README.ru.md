# Nukera

[English](README.md) · [Русский](README.ru.md)

![Cross-Distro Build](https://github.com/finestcrtn/nukera/actions/workflows/distro-test.yml/badge.svg)

## Что это

Nukera разблокирует сайты, заблокированные провайдером или государственной цензурой:

**Instagram (включая Reels), Telegram, YouTube, Discord, WhatsApp** — и другие заблокированные сайты. Из коробки.

## Почему Nukera

- **Просто работает.** Запустил, нажал ▶ — готово. Без терминала, без конфигов, без ручной настройки прокси.
- **Встроенный прокси для Telegram.** Приложение Telegram автоматически подключается через встроенный прокси Nukera — настраивать ничего не нужно.
- **Работает там, где другие сдаются.** Instagram Reels, WhatsApp и Telegram закрыты по умолчанию — не нужно возиться с настройками под каждый сайт.
- **Бесплатно, локально, приватно.** Всё работает на вашем компьютере. Данные никуда не отправляются.
- **Открытый исходный код.** Ядро на Rust + GUI на Flutter.

## Установка — 30 секунд

1. Скачайте **`Nukera-Linux-x86_64.AppImage`** из [Releases](https://github.com/finestcrtn/nukera/releases/latest).
2. Запустите двойным кликом.
3. Примите **один** запрос пароля при первом запуске — Nukera установится и откроется. Дальше работает без запросов.

- **Обновление:** скачайте новый AppImage и запустите двойным кликом.
- **Удаление:** запустите AppImage с флагом `--uninstall`.

## Где может не работать

- Нужен Linux с **systemd** и свежим glibc (≥ 2.39): Arch/Manjaro, Fedora ≥ 39 (вкл. Silverblue), Ubuntu 24.04 LTS, Debian 13, openSUSE Tumbleweed.
- На старых Ubuntu 22.04 / Debian 12 может не запуститься — будет показано предупреждение.
- Некоторые провайдеры всё же блокируют часть сайтов — никакой инструмент не может гарантировать работу на каждой сети.
- Пока нет сборок под Windows / macOS / мобильные.

## Из исходников (для разработчиков)

```bash
# нужно: cargo (rustup), Flutter stable, squashfs-tools
scripts/build-appimage.sh
```

## Что внутри

Ядро на Rust + GUI на Flutter (GTK) · `nfqws` (zapret) DPI-десинхронизация со стратегиями · нативный WebSocket-прокси для Telegram · DoH-резолвер + карты IP в hosts · systemd-демон с polkit без пароля.

## Помощь

`Nukera-Linux-x86_64.AppImage --verify` — проверка, что поддерживает ваша система, до установки.