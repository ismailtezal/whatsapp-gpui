@echo off
rem Starts the optimized build, building it first if it is missing.
cd /d "%~dp0"
if not exist "target\release\whatsapp-gpui.exe" (
  echo Building the app, this takes a few minutes the first time...
  cargo build --release --bin whatsapp-gpui || exit /b 1
)
start "" "target\release\whatsapp-gpui.exe"
