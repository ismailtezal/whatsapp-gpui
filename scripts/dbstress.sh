#!/usr/bin/env bash
# Tries to damage a scratch SQLite file the way development could: several processes
# writing to it at once, with processes force-killed at random and new ones started.
# Then checks the file. Never point it at the real chat database.
# Usage: scripts/dbstress.sh <scratch.db> [seconds]
cd "$(dirname "$0")/.." || exit 1
db="$1"; seconds="${2:-40}"
[ -f "$db" ] || { echo "no such file: $db"; exit 1; }
exe=./target/debug/wa-dbstress.exe
end=$(( $(date +%s) + seconds ))
kills=0; starts=0
while [ "$(date +%s)" -lt "$end" ]; do
  running=$(tasklist //FI "IMAGENAME eq wa-dbstress.exe" //FO CSV //NH 2>/dev/null | grep -c wa-dbstress)
  while [ "$running" -lt 3 ]; do
    ("$exe" writer "$db" 30 "w$starts" >/dev/null 2>&1 &)
    starts=$((starts + 1)); running=$((running + 1))
  done
  sleep 0.$(( (RANDOM % 6) + 2 ))
  # Force-kill one writer picked at random, wherever it is in its work.
  pid=$(tasklist //FI "IMAGENAME eq wa-dbstress.exe" //FO CSV //NH 2>/dev/null | grep wa-dbstress | tr -d '"' | cut -d, -f2 | shuf -n 1)
  if [ -n "$pid" ]; then
    taskkill //PID "$pid" //F >/dev/null 2>&1 && kills=$((kills + 1))
  fi
done
taskkill //IM wa-dbstress.exe //F >/dev/null 2>&1
sleep 1
echo "started $starts writers, force-killed $kills"
./target/debug/wa-dbcheck.exe "$db" 2>&1 | grep -E "stress|messages:|chats:|free pages|FAILED" | cut -c1-200
