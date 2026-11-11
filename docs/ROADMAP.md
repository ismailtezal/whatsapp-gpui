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
| Receive messages | Partial | The handling is covered by tests (text, own-device messages, groups, reactions, deletes, edits). One real incoming message has since arrived while the app was running: it appeared in the chat list with its preview and unread badge. That chat was not opened, so the message was not seen inside a conversation. |
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
| Events | Partial | Shown with name, time, place and description. No responding, no creating. |
| Location | Partial | Card that opens the place in the browser's maps. No sending, no live location. |
| Contact cards | Partial | Card with a "message" action. No sending. |
| Disappearing messages | Partial | The timer is tracked per chat; it cannot be changed, and expired messages are not removed locally. |
| View-once | Partial | Placeholder, as on any linked device. |
| Phone-only messages | Verified | Shown with the same notice the official client uses. |
| Pin a message | Verified | Pin (for a week) and unpin from the message menu; the newest pin shows under the header. Pins made before this device was linked are not known. |
| Keep in chat | Not done | |
| Search inside a chat | Built | Full-text, with jump to the message. |
| Drafts | Built | Kept per chat when switching away. |

## Media

| Feature | Status | Notes |
| --- | --- | --- |
| Photos: download, show, viewer | Verified | Download on view; in-app viewer. |
| Send photo | Verified | With thumbnail and dimensions. |
| Send document | Verified | |
| Send video, audio | Built | Same path as photos and documents. Videos are sent without a preview image. |
| File picker, drag and drop, paste | Built | Files are staged above the message box with a preview and sent on Enter; what is typed becomes the caption. Pasting a copied image or copied files stages them too. |
| Voice messages and audio: play in app | Verified (player) / Built (control) | The player was checked on this PC with a real voice message. The play button and progress bar in the conversation were not clicked. |
| Voice messages: record | Not done | Needs an Opus encoder. |
| Media that expired on the server | Verified | Re-requested from the phone, then downloaded. |
| Stickers: show | Verified | Animated stickers play on their own. Checked on screen: two captures a second apart showed different frames of the animated sticker in "Dosyalar". |
| Stickers: send | Verified | |
| Sticker picker | Built | Lists stickers already downloaded to this PC. No favourites, no sticker packs. |
| Video and GIF previews | Verified | GIFs and videos up to 16 MB download on view; Windows makes the preview picture. |
| GIFs: play in app | Verified | A click opens the viewer, where the GIF loops. Seen playing on screen. In the conversation itself a GIF is a still preview; it does not autoplay inline. |
| Video: play in app | Built | Same viewer, with the sound track played alongside. Not run, because unattended tests must stay silent, so picture-sound sync is unproven. No pause, seek or volume controls yet; a click closes it. |
| Documents: open | Built | Downloads and opens with the default app. |
| Albums | Built | Photos sent together are shown as a two-column grid. Videos in an album are still listed separately. |
| Camera capture | Not done | |

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
| Block, report | Built / Not done | Block from a person's info panel and unblock from Settings are built but not exercised. No report. |
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
| Taskbar badge, tray icon | Not done | |
| App icon | Verified (build) | `assets/app.ico`, drawn by `scripts/make-icon.ps1` and embedded by `build.rs`. A placeholder mark: an accent square with a speech bubble. |
| Installer, start with Windows | Built | `installer/whatsapp-gpui.iss` (Inno Setup): installs for the current user without administrator rights, Start menu entry, optional desktop shortcut and "Start with Windows", uninstaller. Built by the release workflow; see "Releases" below for whether it has been run. Not signed, so Windows SmartScreen will warn. |
| Build and release workflows | Built | `.github/workflows/ci.yml` tests and builds every push to `main`. `release.yml` runs on a `v*` tag and publishes the installer and a portable zip as a GitHub release. |
| Interface styles | Verified (shown) | Modern, Glass, Liquid Glass and Futuristic, chosen in Settings and remembered. Glass is a translucent window over the Windows blur. Futuristic adds accent glows and sharper borders. Not every style was checked with every palette. |
| Liquid Glass style | Verified (shown) | Real glass drawn by the renderer (a patch to the vendored GPUI, see `vendor/README.md`): the rail, chat list, conversation panel, chat header, message box and menus show a blurred copy of what is beneath them, bent at the rim with a slight colour split and a lit edge. The header and message box float over the messages, which pass beneath them. Checked on screen in dark and light, zoomed in on the edges; holds 165 frames a second while scrolling. Differences from Apple's Liquid Glass: the lighting is fixed (it does not react to window movement or content brightness), and menus were not opened in this style during the checks. |
| Speed | Verified (measured) | Measured with `WA_PERF` on a 165 Hz screen: scrolling a conversation and the chat list holds 165 frames a second (about 2.5 ms of work per frame, down from 10 to 18 ms). Lists build only the rows on screen; wheel scrolling glides; a reload updates only the rows that changed, so the list keeps its place. Not measured: typing, opening the emoji picker, playing video, the first seconds of a large history sync. |
| Reaction bar on hover | Verified (position) / Built (hover) | Sits right next to the bubble, level with its top (left of own messages, right of others), not at the edge of the window. The position was checked on screen with the bar forced visible (`WA_HOVER=1`). Showing it on a real mouse hover was not captured. |
| Colour palettes of the design | Partial | Neutral, Turquoise, Midnight, Mono and Amber, in dark and light, chosen in Settings and remembered. Turquoise dark was checked on screen. The other five palettes of diskurt (ocean, violet, rose, pomegranate, forest) are not included. |

## Design

The interface takes its design tokens from `docs/design-spec.md` (diskurt's modern
design): colours, Figtree, radii, spacing, translucent fills, the frameless window with a
32px title bar, the rail, the floating main panel and roomy menus. The conversation
itself keeps WhatsApp's shape, at the owner's request: bubbles, own messages on the
right in the accent tint, others on the left, time and ticks inside the bubble, names
and pictures on other people's bubbles in groups, centred date and notice pills.
Known differences from the spec:

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
