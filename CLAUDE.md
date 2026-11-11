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
- Runtime data: `%LOCALAPPDATA%\whatsapp-gpui\` (but see "Two data folders" below).

## Working notes

- Toolchain: `stable-x86_64-pc-windows-msvc` (directory override), Visual Studio C++
  tools, Windows 11 SDK.
- GPUI comes from the gpui-kit ecosystem: `gpui-component 0.9.1`, which pins
  `gpui-pre =0.3.12` (imported as `gpui`) and `gpui-pre-platform` (as `gpui_platform`).
  Their sources under `~/.cargo/registry/src` are the API reference.
- `whatsapp-rust` is pinned to a git commit; it bundles SQLite via `libsqlite3-sys 0.38`,
  so `rusqlite` must stay on 0.40.x.
- **Two data folders, two linked devices.** A program started from a packaged app (the
  Claude desktop app's shell: every command an assistant runs, and anything the owner
  starts with a Run button there) sees a redirected `AppData\Local`. Its writes go to
  `AppData\Local\Packages\Claude_…\LocalCache\Local\whatsapp-gpui` (the *container
  copy*), and a file that copy lacks is silently read, written or deleted in the real
  `AppData\Local\whatsapp-gpui` (the *owner's copy*, used when the app is started from
  Explorer, the Start menu or `run.bat`). Each copy is a separately linked WhatsApp
  device with its own `session.db` and `chats.db`.
  - `src/paths.rs` finds the true folder (it writes a marker file and looks where it
    landed) and everything opens files by that true path. Never build a data path from
    `LOCALAPPDATA` yourself; use `paths::data_dir()` or `paths::default_data_dir()`.
  - Tools and tests run from the shell act on the container copy and its device. They
    cannot disturb the owner's running app, and they say nothing about the owner's copy.
  - To read or repair the owner's copy, run the command outside the container, with
    full paths and its output sent to a file in the project:
    `Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{CommandLine='cmd /c "… > C:\Projects\whatsapp-gpui\.reference\out.txt 2>&1"'}`.
    Only while the owner's app is not running. Never delete anything there.
  - A folder link whose target is written with the real path is followed to the owner's
    copy even from inside the container. Do not make such links into the app's data.
- Only one process may use a `session.db` at a time. Close the app that uses the same
  copy before `wa-test`.
- **The chat database is the owner's data. Protect it.** On 2026-11-11 it was damaged
  twice, both times by the redirection above, before that was understood: a tool run
  from the shell opened the container copy's `chats.db` through the merged view while
  that copy had no write-ahead log of its own, so SQLite took the *owner's copy's* log,
  applied it to the wrong database and deleted it; the repair's fallback then deleted
  what it took for the damaged file, which was the owner's copy's `chats.db`. Both
  copies were restored the same evening from their own backups and merged with each
  other and with what could be read from the damaged file (`wa-merge`); each passes
  its integrity check with 73,919 messages. What the owner's copy wrote between 19:53
  and 20:15 that day survives only as far as it could be read back. Rules:
  - Interface tests never open the real database. `scripts/uitest.sh` works on a
    snapshot in `target/uitest-data` (`scripts/uitest.sh refresh` takes a new one from
    the container copy; a failed refresh leaves the previous snapshot standing).
  - `wa-test` (real sends) uses the container copy: run it only while no app is using
    that copy, and let it finish; do not kill it. It leaves half-sent messages found in
    the store alone (`backend::HOLD_PENDING`).
  - Never copy or move a database file without its `-wal` and `-shm` files, and never
    put a database file next to another database's `-wal`: SQLite would apply it.
  - The Opus encoder (machine-translated C) runs only in a child process
    (`voice::finish_isolated`), never in a process that holds the database.
  - `src/guard.rs` opens the database for the app: if it is unreadable it is set aside
    and rebuilt (`src/salvage.rs`); a background check and a daily backup follow
    (`backups\`, three kept).
  - Tools: `wa-dbcheck` (integrity per table), `wa-dbscan` (raw pages), `wa-salvage`
    (rebuild), `wa-guard` (the app's open-and-repair path; `--where` only prints the
    folder it would use), `wa-merge` (add missing rows from other databases, restore a
    backup, refresh pointers), `wa-sql` (read-only counts),
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
  archived), `WA_INFO`, `WA_STYLE`, `WA_THEME`, `WA_PALETTE`.
- More development aids: `WA_STAGE=<file>` stages a file as if attached and `WA_EDIT=1` opens
  the photo editor on it; `WA_SLOWMO=<factor>` stretches every animation. `scripts/type.ps1`
  (text and keys), `scripts/drag.ps1`, `scripts/click.ps1 -Middle` and `scripts/crop.ps1`
  (look at only part of a screenshot) work on the test copy. `wa-video <folder>` reports how
  video files decode. All animations live in `src/ui/motion.rs`; add new ones there.
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
  choice for a dev run. `WA_FAKE_CALL=ringing|calling|connected` shows the call card; `WA_PLAY_VIDEO=1` opens
  the newest downloaded video of the opened chat in the viewer.
- To check the real start-up path (tray, database check, backup, connection) without
  killing anything: `WA_READONLY=1 WA_OFFSCREEN=1 WA_MUTE=1 WA_EXIT_AFTER=35
  target/release/whatsapp-gpui.exe`. It quits by itself; exit code 0 and a fresh file in
  `backups` mean it worked. Only while the owner's copy is not running.
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
