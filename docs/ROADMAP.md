# Roadmap and status

Goal: everything the official WhatsApp client does, on Windows, in GPUI and Rust.
This file is the honest checklist. Each line is one of:

- **Verified**: exercised against the real account, or covered by an automated test.
- **Built**: implemented and compiles, not yet exercised end to end. Treat as unproven.
- **Partial**: some of it works; the gap is stated.
- **Not done**: nothing useful exists yet.

Real-account checks were run only in the owner's "Dosyalar" group (which has no other
member) and in an empty test group created for the purpose, never with another person.
`wa-test` refuses to send anywhere but a group.

## Foundation

| Area | Status | Notes |
| --- | --- | --- |
| Link by QR code | Verified | Linked with `wa-link`. The app draws the QR itself when not linked (Built: this path has not been run, since the device is already linked). |
| Session persistence, reconnect | Verified | Reconnected across dozens of runs. |
| History sync import | Verified | About 73,700 messages in 130 chats arrived from the phone and are stored. |
| Local chat database | Verified | SQLite; the full protocol record of every message; full-text index. |
| Account state replay | Verified | Saved contact names, pins, mutes, archive flags and stars arrive on first run. |
| LID / phone-number identity merge | Verified | Unit test; mappings also learned from history and group details. |
| Get earlier messages from the phone | Built | Request is sent; the phone did not answer during the night test, so the result is unconfirmed. |

## Messaging

| Feature | Status | Notes |
| --- | --- | --- |
| Receive messages | Partial | The handling is covered by tests (text, own-device messages, groups, reactions, deletes, edits). No real incoming message arrived during the night, so it has not been watched working live. |
| Send text | Verified | |
| Reply (quote), jump to the quoted message | Verified (send) / Built (jump) | |
| Reactions: send, show | Verified | Quick reactions and a picker of about 190 emoji. |
| Edit own message | Verified | 15-minute window. |
| Delete for everyone | Verified | Text, photo and document. |
| Delete for me | Built | |
| Star / unstar, starred list | Built | |
| Forward, with a chat picker | Built | |
| Copy text | Built | |
| Message info (who received and read) | Built | From stored receipts. |
| Read receipts (blue ticks) | Built | Test runs set `WA_READONLY=1`, so none has been sent for real. |
| Delivery ticks | Partial | Sent, delivered, read are stored and drawn; history shows all three, live only "sent" was observed. |
| Typing indicator: send, show | Built | Sent at most once every 8 seconds while typing. Sending is disabled in test runs. |
| Presence (online, last seen) | Built | The account shows as online only while the window is in front. |
| Mentions | Built | Shown as name pills. In a group, typing `@` lists the members; picking one sends a real mention. The send has not been exercised (the test groups have no other member to mention). |
| Formatting (bold, italic, strike, code), links | Verified | Unit tests. |
| Link previews | Partial | Shown for received and stored messages. Not generated when sending a link. |
| Polls | Verified | Create (`/poll Question \| A \| B` in the composer), show, vote. Gaps: no dialog for creating one. Decoding of other people's votes as they arrive is built but unproven, since nobody else could vote in the test group (votes in history are read). |
| Events | Not done | Shown as a card with the name only. |
| Location | Partial | Card that opens the place in the browser's maps. No sending, no live location. |
| Contact cards | Partial | Card with a "message" action. No sending. |
| Disappearing messages | Partial | The timer is tracked per chat; it cannot be changed, and expired messages are not removed locally. |
| View-once | Partial | Placeholder, as on any linked device. |
| Phone-only messages | Verified | Shown with the same notice the official client uses. |
| Pin a message, keep in chat | Not done | |
| Search inside a chat | Built | Full-text, with jump to the message. |
| Drafts | Built | Kept per chat when switching away. |

## Media

| Feature | Status | Notes |
| --- | --- | --- |
| Photos: download, show, viewer | Verified | Download on view; in-app viewer. |
| Send photo | Verified | With thumbnail and dimensions. |
| Send document | Verified | |
| Send video, audio | Built | Same path as photos and documents. Videos are sent without a preview image. |
| File picker, drag and drop | Built | |
| Voice messages and audio: play in app | Verified (player) / Built (control) | The player was checked on this PC with a real voice message. The play button and progress bar in the conversation were not clicked. |
| Voice messages: record | Not done | Needs an Opus encoder. |
| Media that expired on the server | Verified | Re-requested from the phone, then downloaded. |
| Stickers: show | Verified | First frame only for animated ones. |
| Stickers: send | Verified | |
| Sticker picker | Built | Lists stickers already downloaded to this PC. No favourites, no sticker packs. |
| Video and GIF previews | Verified | GIFs and videos up to 16 MB download on view; Windows makes the preview picture. |
| Video: play in app | Not done | A click opens it in the default player. |
| GIFs: play in app | Not done | Shown as a still preview; a click opens the default player. |
| Documents: open | Built | Downloads and opens with the default app. |
| Albums | Built | Photos sent together are shown as a two-column grid. Videos in an album are still listed separately. |
| Paste an image, camera | Not done | |

## Chats, contacts, groups

| Feature | Status | Notes |
| --- | --- | --- |
| Chat list: previews, unread, pins, mutes, ticks | Verified | |
| Filters: all, unread, groups; archived list | Built | |
| Search chats and messages | Built | |
| Right-click a chat: read/unread, pin, mute, archive, delete | Built | |
| New chat from saved contacts | Built | |
| Profile pictures | Verified | Fetched one at a time, cached on disk. |
| Chat info panel: about, photos, members | Built | |
| Group details from the server (members, admins, description) | Verified | |
| Create a group | Verified | Tested with nobody else in it. |
| Rename a group, change its description | Verified | |
| Leave a group | Verified | |
| Add, remove, promote, demote members | Built | Deliberately not exercised: it would affect real people. |
| Group settings (who can send, approval, invite links) | Not done | The library supports them. |
| Communities, channels | Not done | Shown as plain chats; the Channels section only filters. |
| Block, report | Partial | Unblock from Settings is built; there is no "block" button on a chat and no report. |
| Labels (business) | Not done | |

## Other surfaces

| Feature | Status | Notes |
| --- | --- | --- |
| Calls list | Verified | 197 calls from history, with direction, duration and missed state. |
| Voice and video calls | Not done | The library handles the call protocol. Microphone, camera, playback and video coding are the app's job and are not started. The call buttons say so. |
| Incoming call notice | Built | A Windows notification says who is calling and to answer on the phone. No ringing screen, no decline button. |
| Status: view | Built | Text and photo statuses of the last 24 hours; other media open in the default player. Viewing does not send a "seen" receipt. |
| Status: post, reply | Not done | |
| Settings: profile, dark / light, data folder, log out | Verified (shown) / Built (log out) | Dark and light were both checked on screen. Log out was not pressed. |
| Settings: privacy and blocked contacts | Partial | Read from the account and shown. Privacy cannot be changed here; unblock is built but not exercised. |
| Settings: notifications, edit name / about / photo | Not done | |
| Notifications | Built | Windows notification for a new message when its chat is not in front; clicking opens the chat. No real incoming message has triggered one yet. |
| Taskbar badge, tray icon, start with Windows, installer | Not done | |
| Other colour palettes of the design | Not done | Neutral only. |

## Design

The interface follows `docs/design-spec.md` (diskurt's modern design): frameless window
with a 32px title bar, rail, floating main panel, Figtree, translucent fills, flat message
rows, roomy menus. Known differences:

- The chat column is 320px, not 248px, to fit two-line rows with previews.
- Chat rows show a preview and a time; diskurt's rows have neither, so this follows the
  spec's suggested member-row shape.
- No open / close motion on menus and dialogs.
- Presence dots (the cut-out status dot on avatars) are not drawn.
- The spec was extracted from diskurt's source; there was no screenshot of the modern
  design to compare against, so small visual differences are likely.

## Test leftovers on the account

Left behind by the checks, all clearly labelled:

- In "Dosyalar": one edited test message, one test reply, one test poll, one sticker,
  and four "This message was deleted" markers.
- A group named "wagpui test group renamed", which the account has left. It can be
  deleted from the chat list.
