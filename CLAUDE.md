# whatsapp-gpui

A native Windows WhatsApp client: GPUI for the interface, `whatsapp-rust` for the
connection. Windows only. Private now, open source later.

## Owner's standing rules

- **Goal is full parity with the official WhatsApp client.** Never drop a feature to
  make the work easier. Report honestly what works, what is built but untested, and
  what is not done. `docs/ROADMAP.md` is the feature checklist and status; keep it true.
- **Usage limit:** the owner first set a stop at 50% weekly plan usage, then lifted it on
  2026-11-11 ("yes keep going and finish everything"). There is no usage stop now; if the
  owner sets a new one, record it here and follow it.
- **Testing on the real account:** never send a message to a real person. Send tests go
  only to a group with nobody else affected: the owner's "Dosyalar" group (one member),
  or a test group created for the purpose. `wa-test` enforces group-only sends. Do not
  mark the owner's real chats as read from test runs: set `WA_READONLY=1`, which disables
  read receipts, typing and presence. Never add, remove or promote real people in groups.
- **Do not play audio during unattended tests** (it comes out of the owner's speakers).
- **Design:** take the design tokens (colours, type, radii, spacing, surfaces, menus) of
  the "modern" design of the owner's other project, diskurt (`C:\Projects\diskurt`,
  read-only; extracted spec in `docs/design-spec.md`). **Tokens only:** the chat
  experience must stay WhatsApp's (bubbles, own messages on the right, time and ticks
  inside the bubble). The owner rejected diskurt's flat Slack-like message rows.

## Layout

- `src/main.rs`, `src/ui/`: the GPUI application (shell, chat, message rows, theme).
- `src/backend.rs`, `src/media_ops.rs`: the connection thread; `Command`s in, `UiEvent`s out.
- `src/calls.rs`: voice calls (microphone and speaker for the library's call engine).
  `src/voice.rs`: voice message recording and the Opus encoder (child process).
  `src/tray.rs`: tray icon, taskbar badge, start with Windows.
- `src/guard.rs`, `src/salvage.rs`: opening, checking, backing up and rebuilding the
  chat database.
- `src/ingest.rs`: applies events to the store (has the receive-path tests).
- `src/store.rs`, `src/store_more.rs`, `src/polls.rs`: the SQLite store.
- `src/bin/wa-link.rs`: QR linking tool. `src/bin/wactl.rs`: inspect the store (masked).
  `src/bin/wa-test.rs`: end-to-end checks. `src/bin/wa-audio.rs`: audio playback check.
- `.reference/whatsapp-rust-docs/`: local copy of the library docs (not committed).
- Runtime data: `%LOCALAPPDATA%\whatsapp-gpui\`.

## Working notes

- Toolchain: `stable-x86_64-pc-windows-msvc` (directory override), Visual Studio C++
  tools, Windows 11 SDK.
- GPUI comes from the gpui-kit ecosystem: `gpui-component 0.9.1`, which pins
  `gpui-pre =0.3.12` (imported as `gpui`) and `gpui-pre-platform` (as `gpui_platform`).
  Their sources under `~/.cargo/registry/src` are the API reference.
- `whatsapp-rust` is pinned to a git commit; it bundles SQLite via `libsqlite3-sys 0.38`,
  so `rusqlite` must stay on 0.40.x.
- Only one process may use `session.db` at a time. Close the app before `wa-test`.
- **The chat database is the owner's data. Protect it.** On 2026-11-11 `chats.db` was
  found damaged (SQLite could not read its schema; recent writes had been lost between
  processes sharing the file). It was rebuilt with nothing known to be missing, but the
  cause was not proven: force-killing concurrent writers and the voice codec were both
  stress-tested and neither reproduced it. Rules since then:
  - Interface tests never open the real database. `scripts/uitest.sh` works on a
    snapshot in `%LOCALAPPDATA%\whatsapp-gpui-uitest` (`scripts/uitest.sh refresh`
    takes a new one).
  - `wa-test` (real sends) uses the real database: run it only while the app is closed,
    and let it finish; do not kill it.
  - Never copy or move a database file without its `-wal` and `-shm` files, and never
    put a database file next to another database's `-wal`: SQLite would apply it.
  - The Opus encoder (machine-translated C) runs only in a child process
    (`voice::finish_isolated`), never in a process that holds the database.
  - `src/guard.rs` opens the database for the app: if it is unreadable it is set aside
    and rebuilt (`src/salvage.rs`); a background check and a daily backup follow
    (`backups\`, three kept).
  - Tools: `wa-dbcheck` (integrity per table), `wa-dbscan` (raw pages), `wa-salvage`
    (rebuild), `wa-guard` (the app's open-and-repair path), `wa-sql` (read-only counts),
    `wa-dbstress` + `scripts/dbstress.sh` (try to break a scratch copy).
- **Testing the interface never touches the owner's running app.** `scripts/uitest.sh`
  builds a separate copy (`target/uitest`, run as `wa-uitest.exe`) that is off the
  screen, never connects (`WA_OFFLINE`), and never saves appearance changes.
  `scripts/dev.sh [chat title] [seconds]` rebuilds it, starts it and saves a screenshot
  to `.reference/shot.png`. `scripts/wheel.ps1 -ProcessName wa-uitest` sends wheel
  scrolls to it. Never kill `whatsapp-gpui.exe`: that is the owner's copy. Build
  `target/release` (what `run.bat` starts) only when the owner's copy is not running.
- **Speed is a requirement: at least 120 frames a second everywhere** (the owner's
  screen is 165 Hz). `scripts/perf.sh [chat]` prints frame timings (`WA_PERF=1`).
  Lists must be virtual (`uniform_list` or `list`), nothing may query the database or
  the disk per frame, and a chat reload must splice only the rows that changed.
- GPUI is vendored in `vendor/` with patches (glass backgrounds, smooth wheel
  scrolling, frame timing); see `vendor/README.md`.
- Development aids: `WA_OPEN_CHAT`, `WA_SECTION` (calls, status, starred, settings,
  archived), `WA_INFO`, `WA_HOVER=1` (keeps every message's hover bar visible),
  `WA_STYLE`, `WA_THEME`, `WA_PALETTE`.
- Source files are reformatted after being written, so line-exact text replacement is
  fragile. Insert by anchor line, or append a new `impl` block.
- Icons are Lucide SVGs in `assets/icons` at stroke width 1.75; add new ones there.
- Work directly on `main`; the owner does not want a `dev` branch.
- Releases: push a `v*` tag. `.github/workflows/release.yml` builds the app, the Inno
  Setup installer (`installer/whatsapp-gpui.iss`) and a portable zip, and publishes
  them. `ci.yml` tests and builds every push to `main`. Inno Setup is not installed on
  this PC, so the installer is only built in CI.
- Interface styles (Modern, Glass, Liquid Glass, Futuristic) live in `src/ui/theme.rs`
  as `Style`; `WA_STYLE`, `WA_BACKDROP`, `WA_PALETTE` and `WA_THEME` override the stored
  choice for a dev run. `WA_FAKE_CALL=ringing|calling|connected` shows the call card.
- `whatsapp-rust` is built with its `voip` feature, which compiles the native Opus
  library with CMake. On this PC CMake came from winget and `.cargo/config.toml` (not
  committed) points Cargo at it.
- **Never call, message, add or follow anyone in a test.** Calls and status replies
  cannot be verified without another person; say so instead of trying. A status may be
  test-posted only with `only_me` (the owner's own devices). WhatsApp rate-limits
  group creation: after a few test groups in a day, stop creating more.
- Patches to source files are applied with small Node scripts written with the Write
  tool to `.reference/p_*.js` (exact-text replace, reporting what is missing). Bash
  heredocs containing Rust break, and reusing one script name has re-run an old patch.
