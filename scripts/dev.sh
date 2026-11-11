#!/usr/bin/env bash
# Development loop: stop the running app, rebuild, relaunch in read-only test mode,
# and save a screenshot of the window to .reference/shot.png.
# Usage: scripts/dev.sh [chat-title-to-open] [seconds-to-wait]
cd "$(dirname "$0")/.." || exit 1
for pid in $(tasklist //FI "IMAGENAME eq whatsapp-gpui.exe" //FO CSV //NH 2>/dev/null | tr -d '"' | cut -d, -f2); do
  taskkill //PID "$pid" //F >/dev/null 2>&1
done
sleep 1
if ! cargo build --bin whatsapp-gpui 2>&1 | grep -E "^(error|warning)" -A12 | head -${LINES_MAX:-120}; then :; fi
if [ "${PIPESTATUS[0]}" != "0" ] && ! [ -f target/debug/whatsapp-gpui.exe ]; then exit 1; fi
cargo build --bin whatsapp-gpui >/dev/null 2>&1 || { echo "BUILD FAILED"; exit 1; }
(WA_READONLY=1 WA_HOVER="${WA_HOVER:-}" WA_OPEN_CHAT="${1:-}" ./target/debug/whatsapp-gpui.exe > .reference/app.log 2>&1 &)
sleep "${2:-8}"
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/shot.ps1 -Out "$(pwd -W)/.reference/shot.png"
tail -3 .reference/app.log | cut -c1-300
