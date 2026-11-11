# whatsapp-gpui

A native WhatsApp client for Windows, written in Rust: [GPUI](https://www.gpui.rs/) for the
interface and [`whatsapp-rust`](https://github.com/oxidezap/whatsapp-rust) for the
connection. It links to your account as one more device, the same way WhatsApp Desktop does.

> **Unofficial.** This project is not affiliated with WhatsApp or Meta. Third-party clients
> are against WhatsApp's terms of service and an account that uses one can be restricted.
> Use it at your own risk.

What works and what does not is tracked honestly, feature by feature, in
[docs/ROADMAP.md](docs/ROADMAP.md).

## Install

Download `whatsapp-gpui-setup-<version>.exe` from the repository's Releases page and run
it. It installs for the current user only (no administrator rights), adds a Start menu
entry and an uninstaller, and can add a desktop shortcut and start with Windows. The
installer is not signed, so Windows SmartScreen asks for confirmation. The portable zip
on the same page holds just the program.

A release is made by pushing a version tag; the workflow in
`.github/workflows/release.yml` builds and publishes it:

```bash
git tag v0.1.0
```

```bash
git push origin v0.1.0
```

## Run it from source

```bash
run.bat
```

`run.bat` starts the optimized build (`target\release\whatsapp-gpui.exe`), building it
first if it is missing. The first start shows a QR code: on your phone open WhatsApp,
then Settings, Linked devices, Link a device, and scan it.

Only one copy may run at a time. Two connections with the same linked device replace
each other.

## Build

Requirements: Rust 1.95 or newer with the `x86_64-pc-windows-msvc` toolchain, Visual
Studio with the C++ tools, and the Windows 11 SDK.

```bash
cargo build --release --bin whatsapp-gpui
```

```bash
cargo test --lib
```

## Where your data lives

Everything is kept on this PC under `%LOCALAPPDATA%\whatsapp-gpui`:

| Path | What it is |
| --- | --- |
| `session.db` | The linked-device session and encryption keys. |
| `chats.db` | Chats, messages and contacts. |
| `history\` | The raw history the phone sent, kept so `chats.db` can be rebuilt. |
| `media\`, `avatars\` | Downloaded media and profile pictures. |

Deleting the folder unlinks nothing by itself; use Settings, Log out, to unlink the device.

## Layout

| Path | Role |
| --- | --- |
| `src/backend.rs`, `src/media_ops.rs` | The live connection: commands in, events out. |
| `src/ingest.rs` | Applies what the connection reports to the store. |
| `src/store.rs`, `src/store_more.rs`, `src/polls.rs` | The local database. |
| `src/content.rs`, `src/richtext.rs`, `src/system.rs` | Turning protocol messages into what is shown. |
| `src/ui/` | The interface: shell, conversation, message rows, theme. |
| `src/bin/wa-link.rs` | Command-line linking tool. |
| `src/bin/wactl.rs` | Inspect the local database (content is masked by default). |
| `src/bin/wa-test.rs` | End-to-end checks against the real connection; sends only to groups. |
| `docs/design-spec.md` | The design the interface follows. |

## Design

The look uses the design tokens of diskurt's "modern" design: a frameless window with a
slim title bar, a rail, a floating main panel, the Figtree typeface and translucent
fills. The conversation keeps WhatsApp's shape: bubbles, with your messages on the right.

## Credits

- Figtree by Erik Kennedy, under the SIL Open Font License (`assets/fonts/Figtree-OFL.txt`).
- Icons from [Lucide](https://lucide.dev), under the ISC license.
- `whatsapp-rust` by João Lucas de Oliveira Lopes, under the MIT license.
- GPUI by Zed Industries and `gpui-component` by Longbridge, under the Apache 2.0 license.
