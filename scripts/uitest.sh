#!/usr/bin/env bash
# Starts a test copy of the interface that cannot disturb the real app:
# - built into target/uitest and copied to wa-uitest.exe, so the real app's files are
#   never locked or replaced and stopping the test never stops the real app;
# - off the screen (WA_OFFSCREEN), never connected (WA_OFFLINE), read-only.
# It shows whatever is in the local store. Screenshots still work (scripts/shot.ps1
# -ProcessName wa-uitest), and so do wheel events (scripts/wheel.ps1 -ProcessName
# wa-uitest).
#
# Usage: scripts/uitest.sh start [chat title]   (extra variables: WA_STYLE, WA_PERF, ...)
#        scripts/uitest.sh shot [file.png]
#        scripts/uitest.sh stop
#        scripts/uitest.sh build
cd "$(dirname "$0")/.." || exit 1
exe=target/uitest/release/wa-uitest.exe
case "${1:-}" in
  build)
    cargo build --release --bin whatsapp-gpui --target-dir target/uitest 2>&1 | grep -E "^(error|warning)" -A14 | head -60
    taskkill //IM wa-uitest.exe //F >/dev/null 2>&1
    cp target/uitest/release/whatsapp-gpui.exe "$exe" || exit 1
    ;;
  start)
    taskkill //IM wa-uitest.exe //F >/dev/null 2>&1
    sleep 0.5
    (WA_OFFLINE=1 WA_OFFSCREEN=1 WA_READONLY=1 WA_OPEN_CHAT="${2:-}" "$exe" > .reference/uitest.log 2>&1 &)
    ;;
  shot)
    powershell -NoProfile -ExecutionPolicy Bypass -File scripts/shot.ps1 -ProcessName wa-uitest -Out "$(pwd -W)/.reference/${2:-shot.png}"
    ;;
  stop)
    taskkill //IM wa-uitest.exe //F >/dev/null 2>&1
    ;;
  *)
    echo "usage: scripts/uitest.sh build | start [chat] | shot [file] | stop"
    ;;
esac
