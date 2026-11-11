# whatsapp-gpui

A native Windows WhatsApp client: GPUI for the interface, `whatsapp-rust` for the
connection. Windows only. Private now, open source later.

## Owner's standing rules

- **Goal is full parity with the official WhatsApp client.** Never drop a feature to
  make the work easier. Report honestly what works, what is built but untested, and
  what is not done. `docs/ROADMAP.md` is the feature checklist and status; keep it true.
- **Stop working when weekly plan usage reaches 50%** (either the all-models or the
  per-model figure). Check with the usage tool regularly.
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
- `scripts/dev.sh [chat title] [seconds]` stops the app, rebuilds, relaunches in
  read-only mode and saves a screenshot to `.reference/shot.png`. Development aids:
  `WA_OPEN_CHAT`, `WA_SECTION` (calls, status, starred, settings, archived), `WA_INFO`.
- Source files are reformatted after being written, so line-exact text replacement is
  fragile. Insert by anchor line, or append a new `impl` block.
- Icons are Lucide SVGs in `assets/icons` at stroke width 1.75; add new ones there.
- Work directly on `main`; the owner does not want a `dev` branch.
- Releases: push a `v*` tag. `.github/workflows/release.yml` builds the app, the Inno
  Setup installer (`installer/whatsapp-gpui.iss`) and a portable zip, and publishes
  them. `ci.yml` tests and builds every push to `main`. Inno Setup is not installed on
  this PC, so the installer is only built in CI.
- Interface styles (Modern, Glass, Futuristic) live in `src/ui/theme.rs` as `Style`;
  `WA_STYLE`, `WA_PALETTE` and `WA_THEME` override the stored choice for a dev run.
