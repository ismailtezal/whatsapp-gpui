# Roadmap and status

Goal: everything the official WhatsApp client does, on Windows, in GPUI and Rust.
This file is the honest checklist. Each line is one of:

- **Verified**: exercised against the real account or covered by a test.
- **Built**: implemented and compiles, not yet exercised end to end.
- **Partial**: some of it works; the gap is stated.
- **Not done**: nothing useful exists yet.

Real-account checks were run in the owner's "Dosyalar" group only (see `CLAUDE.md`).

## Foundation

| Area | Status | Notes |
| --- | --- | --- |
| Link by QR code | Verified | `wa-link`; the app itself shows the pairing state but does not draw the QR yet. |
| Session persistence, reconnect | Verified | Reconnected across many runs of `wa-test` and the app. |
| History sync import | Verified | 151 conversations, 10,514 messages imported from the phone's sync. |
| Full (one-year) history | Partial | Requested at pairing; the phone sent the recent set only. Older messages need the on-demand request (`LoadOlder`), which is not implemented. |
| Local chat database | Verified | SQLite, full protocol record per message, full-text index. |
| Account state replay | Verified | Saved contact names, pins, mutes, archive, stars arrive on first run. |
| LID / phone-number identity merge | Built | Mappings from history and live messages fold chats together. |

## Messaging

| Feature | Status | Notes |
| --- | --- | --- |
| Receive text, live | Built | Stored and shown; not observed live yet (no incoming message during testing). |
| Send text | Verified | Server acknowledged. |
| Reply (quote) | Built | Composer reply bar and quote context; not sent to the server in a test yet. |
| Reactions: send, show | Verified | Sent 👍; chips show with own-reaction styling. |
| Edit own message | Verified | Server accepted; "(edited)" shown. 15-minute window enforced in the menu. |
| Delete for everyone | Verified | Text and photo revoked. |
| Delete for me | Built | |
| Star / unstar | Built | |
| Forward | Built (backend) | No chat picker in the interface yet. |
| Copy text | Built | |
| Read receipts (blue ticks) | Built | Disabled in test runs (`WA_READONLY=1`), so never sent for real yet. |
| Delivery ticks | Partial | Sent / delivered / read states stored and drawn; only "sent" observed. |
| Typing indicator: send, show | Built | Sending is disabled in test runs. |
| Presence (online / last seen) | Built | |
| Mentions | Partial | Shown as name pills; cannot be inserted from the composer yet. |
| Formatting (bold, italic, strike, code), links | Verified | Unit tests; links open in the browser. |
| Disappearing messages | Partial | Timer is tracked per chat; expired messages are not removed locally. |
| View-once | Partial | Shown as a placeholder, as on linked devices. |
| Polls: show, vote, create | Not done | Shown as a card with the question only. |
| Events: show, respond, create | Not done | Card only. |
| Location, contacts cards | Partial | Card with the name; no map, no "add contact". |
| Pin message, keep in chat | Not done | |
| Message info (who read it) | Not done | Per-user receipts are stored. |
| Search inside a chat | Not done | Store has full-text search; no interface. |
| Drafts | Not done | Column exists. |

## Media

| Feature | Status | Notes |
| --- | --- | --- |
| Photos: download, show, viewer | Verified | Auto-download, in-app viewer. |
| Send photo | Verified | Uploaded and sent with thumbnail and dimensions. |
| Send video, audio, document | Built | Same path as photos; not sent in a test yet. |
| Drag and drop files, file picker | Built | |
| Stickers: show | Built | Auto-download; animated stickers show their first frame at best. |
| Stickers: send, sticker picker | Not done | |
| Video: play in app | Not done | Downloads, then opens in the default player. |
| GIFs: play in app | Not done | Shown as a still. |
| Voice messages: play in app | Not done | Downloads, then opens in the default player. |
| Voice messages: record | Not done | |
| Documents: open | Built | Downloads and opens with the default app. |
| Albums | Partial | Photos show one below another; no grid. |
| Link previews | Not done | |
| Media expired on the server (re-request from phone) | Not done | |
| Paste image from clipboard | Not done | |
| Camera capture | Not done | |

## Chats and contacts

| Feature | Status | Notes |
| --- | --- | --- |
| Chat list with previews, unread, pins, mutes | Verified | |
| Filters: all, unread, groups | Built | |
| Search chats by name | Built | |
| Global message search | Not done | Store supports it. |
| Archive, pin, mute, mark unread, delete chat | Built (backend) | No context menu on chat rows yet. |
| Archived list | Built | Rail button. |
| New chat / contact list | Not done | |
| Profile pictures | Verified | Fetched one at a time and cached on disk. |
| Contact / group info panel | Not done | |
| Group management (create, add, remove, admins, settings, invite links) | Not done | Library supports it. |
| Communities | Not done | Shown as chats. |
| Block / report | Not done | |
| Labels (business) | Not done | |

## Other surfaces

| Feature | Status | Notes |
| --- | --- | --- |
| Status (stories): view, post, reply | Not done | Status messages are stored; no viewer. |
| Channels (newsletters) | Not done | |
| Calls list | Not done | Call log records arrive but are not stored yet. |
| Voice and video calls | Not done | The library handles the call protocol; microphone, camera, playback and H.264 are the app's job and are not started. |
| Incoming call notice (ring, decline) | Not done | |
| Starred messages list | Not done | |
| Settings (profile, privacy, notifications, theme, storage) | Not done | |
| Notifications (Windows toasts, taskbar badge) | Not done | |
| Light theme | Built | Tokens exist; no switch in the interface. |
| Other colour palettes of the design | Not done | Neutral only. |
| Log out / unlink | Built (backend) | No button yet. |
| Auto-start, tray icon, installer | Not done | |

## Design

The interface follows `docs/design-spec.md` (diskurt's modern design): frameless window
with a 32px title bar, rail, floating main panel, Figtree, translucent fills, flat message
rows. Known gaps: no open/close motion on menus, no cut-out presence dots, chat column is
320px instead of 248px to fit previews, and the modern design was reproduced from source
code without a rendered reference to compare against.
