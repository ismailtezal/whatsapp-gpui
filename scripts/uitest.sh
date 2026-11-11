#!/usr/bin/env bash
# Starts a test copy of the interface that cannot disturb the real app or its data:
# - built into target/uitest and copied to wa-uitest.exe, so the real app's files are
#   never locked or replaced and stopping the test never stops the real app;
# - it works on a throwaway snapshot of the data (WA_DATA_DIR), never on the real chat
#   database, so it can be force-stopped at any moment;
# - off the screen (WA_OFFSCREEN), never connected (WA_OFFLINE).
# Screenshots still work (scripts/uitest.sh shot), and so do wheel events and clicks
# (scripts/wheel.ps1, scripts/click.ps1 with -ProcessName wa-uitest).
#
# Usage: scripts/uitest.sh build
#        scripts/uitest.sh refresh              (take a new snapshot of the real data)
#        scripts/uitest.sh start [chat title]   (extra variables: WA_STYLE, WA_PERF, ...)
#        scripts/uitest.sh shot [file.png]
#        scripts/uitest.sh stop
cd "$(dirname "$0")/.." || exit 1
exe=target/uitest/release/wa-uitest.exe
data="$LOCALAPPDATA/whatsapp-gpui-uitest"
snapshot() {
  [ -f target/debug/wa-snapshot.exe ] || cargo build --bin wa-snapshot 2>&1 | grep -E "^error" -A8
  # The snapshot tool reads the real folder; it must not inherit a test data folder.
  env -u WA_DATA_DIR ./target/debug/wa-snapshot.exe "$(cygpath -w "$data")"
}
case "${1:-}" in
  build)
    cargo build --release --bin whatsapp-gpui --target-dir target/uitest 2>&1 | grep -E "^(error|warning)" -A14 | head -60
    taskkill //IM wa-uitest.exe //F >/dev/null 2>&1
    cp target/uitest/release/whatsapp-gpui.exe "$exe" || exit 1
    ;;
  refresh)
    taskkill //IM wa-uitest.exe //F >/dev/null 2>&1
    sleep 0.5
    snapshot
    ;;
  start)
    taskkill //IM wa-uitest.exe //F >/dev/null 2>&1
    sleep 0.5
    [ -f "$data/chats.db" ] || snapshot
    (WA_DATA_DIR="$(cygpath -w "$data")" WA_OFFLINE=1 WA_OFFSCREEN=1 WA_READONLY=1 WA_OPEN_CHAT="${2:-}" "$exe" > .reference/uitest.log 2>&1 &)
    ;;
  shot)
    powershell -NoProfile -ExecutionPolicy Bypass -File scripts/shot.ps1 -ProcessName wa-uitest -Out "$(pwd -W)/.reference/${2:-shot.png}"
    ;;
  stop)
    taskkill //IM wa-uitest.exe //F >/dev/null 2>&1
    ;;
  *)
    echo "usage: scripts/uitest.sh build | refresh | start [chat] | shot [file] | stop"
    ;;
esac
