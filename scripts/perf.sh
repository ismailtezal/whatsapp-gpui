#!/usr/bin/env bash
# Measures frame times on the test copy (off-screen, offline; see scripts/uitest.sh),
# with the message list scrolling by itself, and prints the last timing lines.
# Run scripts/uitest.sh build first. Usage: scripts/perf.sh [chat title] [seconds]
cd "$(dirname "$0")/.." || exit 1
WA_PERF=1 WA_PERF_SCROLL="${WA_PERF_SCROLL-1}" scripts/uitest.sh start "${1:-Dosyalar}"
sleep "${2:-14}"
scripts/uitest.sh stop
grep "^perf:" .reference/uitest.log | tail -2 | tr '|' '\n' | grep -E "frame|render|column|reload"
grep "^gpui:" .reference/uitest.log | tail -2 | tr '|' '\n'
grep "^note:" .reference/uitest.log | tail -2
