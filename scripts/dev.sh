#!/usr/bin/env bash
# Development loop: rebuild the test copy, start it off-screen and offline, and save a
# screenshot of its window to .reference/shot.png. The real app is never touched: see
# scripts/uitest.sh.
# Usage: scripts/dev.sh [chat-title-to-open] [seconds-to-wait]
# Variables such as WA_STYLE, WA_THEME, WA_PALETTE, WA_SECTION pass through.
cd "$(dirname "$0")/.." || exit 1
scripts/uitest.sh build || { echo "BUILD FAILED"; exit 1; }
scripts/uitest.sh start "${1:-}"
sleep "${2:-8}"
scripts/uitest.sh shot shot.png
tail -3 .reference/uitest.log | cut -c1-300
