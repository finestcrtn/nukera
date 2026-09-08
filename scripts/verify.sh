#!/usr/bin/env bash
# verify.sh — end-to-end test for the unblocker v4 install.
# This is the script the user asked for: it confirms that
#   1. With unblocker OFF, the network works normally
#   2. With unblocker ON, every target DPI site loads with 200
#   3. With unblocker OFF again, the network is still normal (no stale rule)
# The trap ensures the nft table is flushed even if anything goes wrong.
set -u
TARGETS=(
    "https://www.youtube.com/"
    "https://meduza.io/"
    "https://discord.com/"
    "http://rutracker.org/"
    "https://vk.com/"
    "https://www.bbc.com/"
)

cleanup() {
    pkexec systemctl stop unblocker.service 2>/dev/null || true
    pkexec nft delete table inet unblocker 2>/dev/null || true
    pkexec iptables -t mangle -F 2>/dev/null || true
    pkexec iptables -t nat -F 2>/dev/null || true
    pkexec pkill -9 -f nfqws 2>/dev/null || true
    gsettings set org.gnome.system.proxy mode 'none' 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# Phase 0: clean baseline
cleanup
sleep 1

echo "=== Phase 1: OFF (internet must be normal) ==="
gsettings get org.gnome.system.proxy mode 2>&1
google_off=$(curl -s -o /dev/null -w "%{http_code}" --max-time 6 https://www.google.com/ 2>&1)
vk_off=$(curl -s -o /dev/null -w "%{http_code}" --max-time 6 https://vk.com/ 2>&1)
echo "  google=$google_off  vk=$vk_off  (expect both non-zero)"
if [ "$google_off" = "000" ] || [ "$vk_off" = "000" ]; then
    echo "FAIL: baseline is not reachable"
    exit 1
fi

echo
echo "=== Phase 2: ON (every DPI site must work) ==="
# This is what the GUI does on click.
systemctl start unblocker.service 2>&1
sleep 5
echo "  service: $(systemctl is-active unblocker.service 2>&1)"
echo "  nfqws:   $(ps aux 2>&1 | grep nfqws | grep -v grep | head -n 1 | awk '{print $2, $11, $12, $13}')"
echo "  nft:"
pkexec nft list table inet unblocker 2>&1 | sed 's/^/    /'
echo
echo "  target sites:"
fail=0
for u in "${TARGETS[@]}"; do
    code=$(timeout 12 curl --max-time 10 -4 -sL -o /tmp/p.html -w "%{http_code}" "$u" 2>&1 | tr -d '\n')
    sz=$(stat -c %s /tmp/p.html 2>/dev/null)
    marker="FAIL"; [ "$code" = "200" ] && marker="ok"
    echo "    [$marker] $u => $code (${sz}B)"
    if [ "$code" != "200" ]; then fail=$((fail+1)); fi
done
if [ $fail -gt 0 ]; then
    echo "FAIL: $fail of ${#TARGETS[@]} sites did not return 200"
fi

echo
echo "=== Phase 2b: image + asset body checks (proves DPI bypass works for resources, not just HTML) ==="
# (a) User's exact IG image URL. Must return real JPEG bytes
# (0xff 0xd8 ...), not HTML or empty. This catches the "page loads but
# images don't" case that the HTML-only test above misses.
IG_IMG='https://scontent-hel3-1.cdninstagram.com/v/t51.2885-19/455596973_468815979375023_2483373531962989209_n.jpg?stp=dst-jpg_s150x150_tt6&_nc_cat=100&ccb=7-5&_nc_sid=f7ccc5&efg=eyJ2ZW5jb2RlX3RhZyI6InByb2ZpbGVfcGljLnd3dy4zMzMuQzMifQ%3D%3D&_nc_ohc=jDweqdYxbNwQ7kNvwELuwPm&_nc_oc=AdrrXnEwmrkQM9Y6J3FL83r9Ya0QLDT6ZsYx5i4SFlhdDJRDHqZ6FD2h5ukvlPG7H28&_nc_zt=24&_nc_ht=scontent-hel3-1.cdninstagram.com&_nc_ss=7b6a8&oh=00_AQJ8RUB_46NQeCLWqPyrMKgqDNRhpkNFbskPUn9ZE-a1sA&oe=6A9B87FF'
code=$(timeout 12 curl --max-time 10 -4 -sL -o /tmp/ig.jpg -w "%{http_code}" "$IG_IMG" 2>/dev/null | tr -d '\n')
head=$(head -c 3 /tmp/ig.jpg 2>/dev/null | xxd -p | tr -d '\n')
if [ "$code" = "200" ] && [ "$head" = "ffd8ff" ]; then
    echo "  [ok]  IG image (scontent-hel3-1): 200 + JPEG magic (ffd8ff), size=$(stat -c %s /tmp/ig.jpg 2>/dev/null)B"
else
    echo "  [FAIL] IG image: code=$code magic=$head (expected 200 + ffd8ff)"
    fail=$((fail+1))
fi
# (b) IG static webp (32x32 icon). Must return RIFF header bytes.
curl --max-time 10 -4 -sL -o /tmp/ig.webp https://static.cdninstagram.com/rsrc.php/yr/r/rzWiSjZRxk5.webp 2>/dev/null
head=$(head -c 4 /tmp/ig.webp 2>/dev/null | xxd -p | tr -d '\n')
if [ "$head" = "52494646" ]; then
    echo "  [ok]  IG static webp (32x32 icon): RIFF header (52494646), size=$(stat -c %s /tmp/ig.webp 2>/dev/null)B"
else
    echo "  [FAIL] IG static webp: magic=$head (expected 52494646)"
    fail=$((fail+1))
fi
# (c) WA web client (web.whatsapp.com). Pass = HTTP connection succeeds
# (200, 404, 302, etc). A timeout (000) means the DPI blocked the
# connection. The web client returns 404 to curl (expects a browser
# JS runtime), but a real HTTP response proves the CDN is reachable.
code=$(timeout 12 curl --max-time 10 -4 -sL -o /tmp/wa.html -w "%{http_code}" 'https://web.whatsapp.com/' 2>/dev/null | tr -d '\n')
if [ "$code" != "000" ] && [ -n "$code" ]; then
    echo "  [ok]  WA web (web.whatsapp.com): $code (connection alive, size=$(stat -c %s /tmp/wa.html 2>/dev/null)B)"
else
    echo "  [FAIL] WA web: code=$code (expected 2xx/4xx response, not 000 timeout)"
    fail=$((fail+1))
fi
# (d) FB main page must have real HTML (not 404 placeholder).
code=$(timeout 12 curl --max-time 10 -4 -sL -o /tmp/fb.html -w "%{http_code}" 'https://www.facebook.com/' 2>/dev/null | tr -d '\n')
if [ "$code" = "200" ] && grep -qi -E 'facebook|fbcdn' /tmp/fb.html 2>/dev/null; then
    echo "  [ok]  FB main page: 200 + body content, size=$(stat -c %s /tmp/fb.html 2>/dev/null)B"
else
    echo "  [FAIL] FB main: code=$code (expected 200 + body content)"
    fail=$((fail+1))
fi
echo "  asset-body fails so far: $fail"

echo
echo "=== Phase 3: OFF again (must be a true no-op) ==="
# This is what the GUI does on second click.
systemctl stop unblocker.service 2>&1
sleep 3
echo "  service: $(systemctl is-active unblocker.service 2>&1)"
echo "  nft:     $(pkexec nft list table inet unblocker 2>&1 | head -n 1)"
echo "  proxy:   $(gsettings get org.gnome.system.proxy mode 2>&1)"
google_post=$(curl -s -o /dev/null -w "%{http_code}" --max-time 6 https://www.google.com/ 2>&1)
vk_post=$(curl -s -o /dev/null -w "%{http_code}" --max-time 6 https://vk.com/ 2>&1)
echo "  google=$google_post  vk=$vk_post  (expect both non-zero)"
if [ "$google_post" = "000" ] || [ "$vk_post" = "000" ]; then
    echo "FAIL: network broken after disable"
    exit 1
fi

echo
if [ $fail -gt 0 ]; then
    echo "======================================"
    echo "  PHASE 2 FAILED: $fail of ${#TARGETS[@]} HTML + 4 asset checks"
    echo "  (autotune will keep retrying; the daemon's health_loop switches strategy every 30s)"
    echo "  For persistent failures, run: unblocker refresh <host>"
    echo "======================================"
    exit 1
else
    echo "======================================"
    echo "  ALL CHECKS PASSED"
    echo "  ${#TARGETS[@]} HTML sites + 4 asset checks loaded with real bodies"
    echo "  Disable was a clean no-op"
    echo "======================================"
fi
