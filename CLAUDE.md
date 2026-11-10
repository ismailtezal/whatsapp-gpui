# whatsapp-gpui

A native Windows WhatsApp client: GPUI for the interface, `whatsapp-rust` for the
connection. Windows only. Private now, open source later.

## Owner's standing rules

- **Goal is full parity with the official WhatsApp client.** Never drop a feature to
  make the work easier. Report honestly what works, what is built but untested, and
  what is not done. `docs/ROADMAP.md` is the feature checklist and status.
- **Stop working when weekly plan usage reaches 50%** (either the all-models or the
  per-model figure). Check with the usage tool regularly.
- **Testing on the real account:** never send a message to a real person. Send tests go
  only to a group with nobody else affected: the owner's existing "dosyalar" group, or a
  test group created for the purpose. Do not mark the owner's real chats as read from
  test runs (set `WA_READONLY=1`, which disables read receipts and presence).
- **Design:** copy the "modern" design of the owner's other project, diskurt
  (`C:\Projects\diskurt`, read-only). The extracted spec is `docs/design-spec.md`.

## Layout

- `src/bin/wa-link.rs`: QR linking tool; also records every event and history-sync blob.
- `src/main.rs`: the GPUI application.
- `.reference/whatsapp-rust-docs/`: local copy of the library docs (not committed).
- Runtime data: `%LOCALAPPDATA%\whatsapp-gpui\` (`session.db` is the linked session,
  `history\*.zlib` are raw history-sync blobs, `events.jsonl` is the raw event log).

## Build notes

- Toolchain: `stable-x86_64-pc-windows-msvc` (set as a directory override), Visual Studio
  C++ tools plus the Windows 11 SDK.
- GPUI comes from the gpui-kit ecosystem: `gpui-component 0.9.1`, which pins
  `gpui-pre =0.3.12` (imported as `gpui`) and `gpui-pre-platform` (as `gpui_platform`).
- `whatsapp-rust` is pinned to a git commit; it bundles SQLite via `libsqlite3-sys 0.38`,
  so `rusqlite` must stay on 0.40.x.
- Only one process may use `session.db` at a time: two connections with the same linked
  device replace each other.
