# Roadmap and status

Goal: everything the official WhatsApp client does, on Windows, in GPUI and Rust.
This file is the honest checklist. Each line is one of:

- **Verified**: exercised against the real account, or covered by an automated test.
- **Built**: implemented and compiles, not yet exercised end to end. Treat as unproven.
- **Partial**: some of it works; the gap is stated.
- **Not done**: nothing useful exists yet.

Real-account checks were run only where nobody else is affected: the owner's "Dosyalar"
group (no other member), groups and a community created for the purpose with nobody
else in them, a status posted to the owner's own devices only, and read-only requests.
`wa-test` refuses to send anywhere but a group. Nothing was ever sent to another person,
which is also why several features below are "Built" and not "Verified": calling,
replying to someone's status, adding people to a group and following a channel cannot be
tried without touching someone else or changing the account.

## Foundation

| Area | Status | Notes |
| --- | --- | --- |
| Link by QR code | Verified | Linked with `wa-link`. The app draws the QR itself when not linked (Built: this path has not been run, since the device is already linked). |
| Session persistence, reconnect | Verified | Reconnected across dozens of runs. |
| History sync import | Verified | 73,754 messages in 131 chats are stored. |
| Local chat database | Verified | SQLite; the full protocol record of every message; full-text index. |
| Data safety | Verified (tested) | The app checks its chat database when it starts. If SQLite cannot read it, the file is set aside and rebuilt from its data pages plus the saved history, and the app starts with a notice; this was run against a really damaged file. A full integrity check and a daily backup (three kept) run in the background. See "The database incident" below. |
| Account state replay | Verified | Saved contact names, pins, mutes, archive flags and stars arrive on first run. |
| LID / phone-number identity merge | Verified | Unit test; mappings also learned from history and group details. |
| Get earlier messages from the phone | Built | Request is sent; the phone did not answer during the night test, so the result is unconfirmed. |

## Messaging

| Feature | Status | Notes |
| --- | --- | --- |
| Receive messages | Partial | The handling is covered by tests (text, own-device messages, groups, reactions, deletes, edits). One real incoming message arrived while the app was running: it appeared in the chat list with its preview and unread badge. It was not watched arriving inside an open conversation. |
| Send text | Verified | |
| Reply (quote), jump to the quoted message | Verified (send) / Built (jump) | |
| Reactions: send, show | Verified | Quick reactions and the full emoji picker. |
| Motion | Verified (menu, in slow motion) / Built (rest) | One small set of animations in `src/ui/motion.rs`, after Emil Kowalski's rules: menus and pickers enter in 140 ms from the side they were asked from, dialogs and the call card settle in 200 ms, viewers and the editor fade in 160 ms, the info panel slides in 240 ms, a newly arrived message rises into place in 180 ms; all on a strong ease-out, all leave at once; buttons and menu items dim while pressed. Nothing animates on key presses, chat switching or hover. GPUI cannot scale an element, so entrances move and fade instead of growing. `WA_SLOWMO=<factor>` stretches them for inspection; the menu entrance was looked at that way. |
| Middle-button auto-scroll | Verified (test copy) | A middle click drops a marker; moving the pointer away from it scrolls what is under it, faster the further away. Any click, key or wheel notch ends it; press-drag-release works too. Checked with an injected middle click in a conversation. |
| Reactions from the right-click menu | Verified (menu) / Built (removing) | Nothing appears on hover any more (the owner found the hover bar distracting). The menu's top row has six quick reactions and a plus for the full picker; the one that is mine is marked and choosing it again takes it back. Clicking a message's reaction chips lists who reacted; my own line removes my reaction. |
| Emoji picker | Verified (shown) | Every emoji the system font draws (Unicode 15 and earlier), in nine groups plus recents, with search by name. Seen on screen. No skin-tone choice. |
| Edit own message | Verified | 15-minute window. |
| Delete for everyone | Verified | Text, photo and document. |
| Delete for me | Built | |
| Star / unstar, starred list | Built | |
| Forward, with a chat picker | Verified (send) / Built (picker) | A message was forwarded within "Dosyalar" and arrived marked as forwarded. The picker was not clicked through. |
| Copy text | Built | |
| Message info (who received and read) | Built | From stored receipts. |
| Read receipts (blue ticks) | Built | Test runs set `WA_READONLY=1`, so none has been sent for real. |
| Delivery ticks | Partial | Sent, delivered, read are stored and drawn; history shows all three, live only "sent" was observed. |
| Typing indicator: send, show | Built | Sent at most once every 8 seconds while typing. Sending is disabled in test runs. |
| Presence (online, last seen) | Built | The account shows as online only while the window is in front. |
| Mentions | Verified (send, picker) | Typing `@` in a group lists the members (search ignores accents); arrows, Tab, Enter or a click pick one, and the mention sits in the message box as one coloured unit. Checked in the test copy by typing and picking with the keyboard. A message mentioning this account itself was sent to "Dosyalar" and carried the mention. Edits and captions carry mentions too (built). In the chat list and in copies that lost their tag (forwards), `@number` is shown as the person's name when it is known. `@all` is not built. |
| Formatting (bold, italic, strike, code), links | Verified | Unit tests. |
| Link previews | Partial | Shown for received and stored messages. Not generated when sending a link. |
| Polls | Verified | Create (`/poll Question \| A \| B` in the composer), show, vote. Gaps: no dialog for creating one. Decoding of other people's votes as they arrive is built but unproven. |
| Events | Partial | Shown with name, time, place and description. No responding, no creating. |
| Location | Partial | Card that opens the place in the browser's maps. No sending, no live location. |
| Contact cards | Partial | Card with a "message" action. No sending. |
| Disappearing messages | Partial | The timer can be set from the info panel: verified on a test group (7 days, then off); for a chat with one person it is built but not exercised. Expired messages are not removed locally. |
| View-once | Partial | Placeholder, as on any linked device. |
| Phone-only messages | Verified | Shown with the same notice the official client uses. |
| Pin a message | Verified | Pin (for a week) and unpin from the message menu; the newest pin shows under the header. Pins made before this device was linked are not known. |
| Keep in chat | Not done | |
| Search inside a chat | Built | Full-text, with jump to the message. |
| Drafts | Built | Kept per chat when switching away. |

## Media

| Feature | Status | Notes |
| --- | --- | --- |
| Photos: download, show, viewer | Verified | Download on view. The viewer covers the window: wheel, buttons or a click zoom, a drag moves the zoomed photo, arrows (on screen and on the keyboard) page through the chat's photos, and a copy can be saved. Opening, zooming and paging were checked in the test copy; "Save as" was not clicked. In a conversation a photo is shown from a small copy made in the background, so scrolling past large photos does not stall. |
| Edit a photo before sending | Verified (test copy) | Click a staged picture: crop (free or square, with thirds), turn, draw (8 colours, 3 widths), add text (same colours, 3 sizes, movable), undo, Done. The result replaces the staged file. Checked in the test copy on a generated picture: the saved file matched what the editor showed. Not built: emoji and stickers on a photo, blur, a "view once" switch, editing videos. Text is drawn with Segoe UI, so emoji typed into a label are not drawn. |
| Send photo | Verified | With thumbnail and dimensions. |
| Send document | Verified | |
| Send video, audio | Built | Same path as photos and documents. Videos are sent without a preview image. |
| File picker, drag and drop, paste | Built | Files are staged above the message box with a preview and sent on Enter; what is typed becomes the caption. Pasting a copied image or copied files stages them too. |
| Voice messages and audio: play in app | Verified (player, waveform) / Built (controls) | The player was checked on this PC with a real voice message. A voice message shows its sender's waveform (seen on screen); clicking a bar jumps there and a chip steps the speed through 1, 1.5 and 2 times. Jumping and the speed chip were not clicked. |
| Voice messages: record | Verified (parts) / Built (whole) | The mic button records: a level meter, a timer, discard and send. Checked separately: clicking the mic button in the test copy started a recording and showed the row; the microphone opens and delivers audio on this PC; the encoder (run in a child process) produces Opus in OGG that decodes back to the exact length; a synthetic two-second voice note was sent to "Dosyalar" and accepted by the server. The recording row (discard, timer, level meter, send) was seen on screen, and discard brings the message box back. Not done as one piece: speaking a message and sending it from the app. No pause, no slide-to-lock, no playback before sending. |
| Media that expired on the server | Verified | Re-requested from the phone, then downloaded. |
| Stickers: show | Verified | Animated stickers play on their own, seen on screen. |
| Stickers: send | Verified | |
| Sticker picker | Built | Lists stickers already downloaded to this PC. No favourites, no sticker packs. |
| Video and GIF previews | Verified | GIFs and videos up to 16 MB download on view; Windows makes the preview picture. |
| GIFs: play in app | Verified | A click opens the viewer, where the GIF loops. In the conversation itself a GIF is a still preview; it does not autoplay inline. |
| Video: play in app | Verified (picture) / Built (sound) | The viewer covers the window and has play and pause, a bar to jump with, the time and mute. The pictures follow the sound's clock, so the two stay together; they are handed to the screen once per frame (165 a second measured while playing); large videos are averaged down while decoding instead of being squeezed on screen; videos recorded upright are turned upright (checked with test files carrying each rotation). Sound was only ever played muted in tests, so lip-sync was not judged by ear. |
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
| Chat info panel: about, photos, members | Verified (shown) | The first ten members are listed, with "View all" for the rest. |
| Group details from the server (members, admins, description) | Verified | |
| Create a group | Verified | Tested with nobody else in it. |
| Rename a group, change its description | Verified | |
| Leave a group | Verified | |
| Add, remove, promote, demote members | Built | Deliberately not exercised: it would affect real people. |
| Group settings | Verified (calls) / Built (screen) | On a test group, through the same commands the screen sends: only admins edit info, only admins send, admins approve new members (on and off each), the message timer, reading the requests to join, the invite link and resetting it. Built but not exercised: "members can add people", changing the group photo, answering a request to join. The switches in the info panel were not seen on screen, because the off-screen test copy has no account identity and so does not show admin controls. |
| Join a group from an invite link | Built | New chat panel. Not exercised: it would join a real group. |
| Communities | Verified (create, close) / Built (rest) | A Communities section lists each community with its groups. Verified live: reading the list (the account is in none), creating a community with nobody else in it, closing it. Adding a group to it was refused by WhatsApp's rate limit after the day's test groups, so it is unverified. Joining a community's group, and putting an existing group into a community or taking it out (from the group's info panel, for admins), are built only. The section was not seen with real data. |
| Channels | Verified (read) / Built (rest) | Verified live, read-only: the list of followed channels (the account follows none), looking a public channel up by its link (name, followers, verified mark), and reading its posts with their reaction totals without following. Built but not exercised, because each would change the account: follow, unfollow, mute, react, create a channel, post as an admin. The channel screens were not seen with real data. No channel directory to browse, no live update of reaction totals. |
| Block, report | Built / Not done | Block from a person's info panel and unblock from Settings are built but not exercised. No report. |
| Labels (business) | Not done | |

## Calls

| Feature | Status | Notes |
| --- | --- | --- |
| Calls list | Verified | 197 calls from history, with direction, duration and missed state. |
| Voice calls with one person | Built | Place a call (phone button in a chat), answer or decline one that rings, mute, hang up. The connection library carries the call; the app supplies the microphone and speaker. Checked: the sound devices open the way a call uses them and deliver sound in the right form; the call card in its ringing and connected states, in two styles. **Never used in a real call**: that needs another person. Whether sound flows both ways with a real phone is unknown. |
| Incoming call | Built | A card with the caller, Answer and Decline, plus a Windows notification; the window comes back from the tray. A video call can only be answered with voice. |
| Video calls | Not done | The library carries video but needs the app to capture the camera and to encode and decode H.264; none of that is written. The video button says so. |
| Group calls, call links | Not done | The library supports them. |

## Status

| Feature | Status | Notes |
| --- | --- | --- |
| View | Verified (shown) | Statuses of the last 24 hours. A text status is shown on the colour its author chose. Photos show in the app; videos open in the default player. |
| Tell the poster it was seen | Built | Sent once per status when it is opened; never in test runs. |
| Post | Verified (text) / Built (photo, video) | A composer with eight background colours, or a photo or video with a caption. A text status was posted live to this account's own devices only and accepted by the server. Posting to contacts (the real audience, following the privacy setting synced from the phone) was not exercised, nor was a photo or video. |
| Delete my status | Verified | The test status was taken down again. |
| Reply, like | Built | A reply box and a heart under someone's status. Not exercised: it would message a real person. |
| Who saw my status | Built | From stored receipts. |

## Settings and system

| Feature | Status | Notes |
| --- | --- | --- |
| Profile: name, About, photo | Verified (name) / Built (About, photo) | The name call was exercised live by re-applying the current name. About and photo are built, not exercised: they would change the account. |
| Privacy | Verified (call) / Built (screen) | Each setting is a row of choices. The change call was exercised live by re-applying a current value. A custom "my contacts except…" list made on the phone is shown but cannot be edited here. Default message timer for new chats: built. |
| Blocked contacts | Partial | Listed; unblock is built but not exercised. |
| Dark / light, data folder | Verified (shown) | |
| Log out | Built | Not pressed. |
| Notification settings | Built | One switch for message notifications; chats can also be muted one by one. No choice of sound, no previews on or off. |
| Notifications | Built | Windows notification for a new message when its chat is not in front; clicking opens the chat. No real incoming message has triggered one yet. |
| Tray icon | Verified (icon) / Built (behaviour) | The icon was added, updated and removed on this PC. Built and not watched: a click brings the window back; its menu has Open and Quit; closing the window keeps the app running behind it (a setting, on by default). |
| Taskbar unread badge | Built | The badge picture is covered by a test; it was not seen on the taskbar. |
| Start with Windows | Built | A setting that adds or removes the app from the sign-in programs. The switch was not flipped in a test. |
| App icon | Verified (build) | `assets/app.ico`, drawn by `scripts/make-icon.ps1` and embedded by `build.rs`. A placeholder mark: an accent square with a speech bubble. |
| Installer | Verified (built) | `installer/whatsapp-gpui.iss` (Inno Setup): installs for the current user without administrator rights, Start menu entry, optional desktop shortcut and "Start with Windows", uninstaller. The release workflow built and published it for v0.1.0. It was not installed and run from that file. Not signed, so Windows SmartScreen will warn. |
| Build and release workflows | Verified | `.github/workflows/ci.yml` tests and builds every push to `main`. `release.yml` runs on a `v*` tag and publishes the installer and a portable zip as a GitHub release. |

## Look and speed

| Feature | Status | Notes |
| --- | --- | --- |
| Interface styles | Verified (shown) | Modern, Glass, Liquid Glass and Futuristic, chosen in Settings and remembered. Glass is a translucent window over the Windows blur. Futuristic adds accent glows and sharper borders. Not every style was checked with every palette. |
| Liquid Glass style | Verified (shown) | Real glass drawn by the renderer (a patch to the vendored GPUI, see `vendor/README.md`): the rail, chat list, conversation panel, chat header, message box and menus show a blurred copy of what is beneath them, bent at the rim with a slight colour split and a lit edge. The header and message box float over the messages, which pass beneath them. Checked on screen in dark and light, zoomed in on the edges. Differences from Apple's Liquid Glass: the lighting is fixed (it does not react to window movement or content brightness). |
| Liquid Glass background | Verified (shown) | Settings has Clear (the default: the desktop shows through the window), Frosted (blurred by Windows; over a dark window it looks almost solid) and Solid (the app's own soft colours). Clear and Frosted were captured on the real screen. The glass bends what is inside the app; the desktop behind the window is shown as Windows composes it, not bent. |
| Colour palettes | Verified (values) | All ten of the design: neutral, turquoise, midnight, mono, amber, ocean, violet, rose, pomegranate, forest, in dark and light. The last five are worked out the way the design's own script does it and are tested against the design's table. Only neutral and turquoise were looked at on screen as a whole interface. |
| Speed | Verified (measured) | Measured with `WA_PERF` on a 165 Hz screen: scrolling a conversation and the chat list holds 165 frames a second (about 2.5 ms of work per frame, down from 10 to 18 ms), in the Modern and Liquid Glass styles, and still does after everything added since. Lists build only the rows on screen; wheel scrolling glides; a reload updates only the rows that changed, so the list keeps its place. Not measured: typing, playing video, the first seconds of a large history sync. |

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

## The database incident (2026-11-11)

During development the chat database was found damaged: SQLite could not read its
schema, and the newest pages of the file were blank. Recent writes had been recorded in
SQLite's write-ahead log and never reached the main file. It was rebuilt from its intact
data pages and the saved history files: 73,754 messages and 131 chats, with messages up
to the minute before the damage and no gap seen in the hours before it. It cannot be
ruled out that a few messages of that morning are missing. The damaged file is kept in
`%LOCALAPPDATA%\whatsapp-gpui\backup-20261111-111714`.

The cause was not proven. Development test processes were sharing the file with each
other at the time. Two stress tests on scratch copies (several writers with 33 random
force-kills; 120 rounds of the voice codec next to a warm database cache) both left the
database intact. Every plausible cause was therefore removed: interface tests use a
throwaway snapshot, the voice encoder runs in a child process, and the app repairs,
checks and backs up its database by itself (see "Data safety").

## Test leftovers on the account

Left behind by the checks, all clearly labelled:

- In "Dosyalar": one edited test message, one test reply, one test poll, a sticker,
  a two-second test voice note (a tone), a "mention check" message, a forwarded copy of
  it, and several "This message was deleted" markers.
- Groups the account has left, which can be deleted from the chat list: "wagpui test
  group renamed" and "wagpui settings test".
- A community named "wagpui community" followed by a number was created and closed
  again. Its announcement and general groups may still be listed.
- A status posted to this account only, removed again after a few seconds.

On this PC: CMake was installed with winget (the native Opus library used for calls is
built with it).
