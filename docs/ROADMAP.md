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
| Send text | Verified (earlier) / Built (new delivery) | Messages to one chat now go out one after another, in the order written. A send waits for the connection (up to ten minutes) instead of failing while the app is still connecting, and is tried again under the same ID if the connection drops; only then is it marked failed. A failed message has "Send again" in its menu. Messages left half-sent by a closed app go out again at the next start (the last six hours only). The new delivery path compiles and its parts are tested, but no message has been sent through it yet. |
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
| Events handled in order, none dropped | Built | The connection library by default handled sixteen events at once and dropped events once 256 were waiting, so an edit or a receipt could be applied before its message, and a reconnect after hours offline or a history sync could lose messages silently. Events are now handled one at a time in arrival order with room for 200,000. Not yet watched through a real burst. |
| Problems are shown | Built | A message that could not be sent, or a request the server refused, appears at the bottom of the window for seven seconds (a click dismisses it). Before, it was only in Settings. The notice in a chat's header now clears itself after five seconds. |
| Smaller fixes from a code audit | Built | Messages into a chat with disappearing messages carry the timer; a reply to my own message names me as its author; editing a reply keeps the quote; a timed mute ends; the tray and taskbar badge count every chat whichever section is showing; no notifications for the backlog after a reconnect, and a muted chat stays silent even when another section is showing; "typing…" lapses after 20 seconds without a repeat; a draft is cleared when it is sent; the "New" line stays where it was when the chat was opened; a new message no longer re-measures the whole conversation or throws a scrolled-up view to the top; Escape leaves edit mode; deleting a chat takes two clicks and closes it. Not fixed yet from the same audit: group ticks turn blue on the first reader; a reply to a message this device does not have shows no quote; nothing shows while a file uploads; the message-info panel and a few lists read the database while drawing. |
| "Waiting for this message" | Built | A message this device cannot decrypt yet shows the same placeholder as the official client and is replaced when the sender's phone delivers it again. Not seen live. |
| Read receipts (blue ticks) | Built | Test runs set `WA_READONLY=1`, so none has been sent for real. |
| Delivery ticks | Fixed (tested) | A "delivered" or "read" that arrived before its message (usual for messages typed on the phone) used to be lost, leaving one grey tick for ever: 59 of one day's messages were found that way with their receipts on record. A new message row now takes its ticks from the receipts on record, a receipt that names the chat by an unknown address finds its message by ID, and existing rows were repaired once (checked on a copy: 70 stuck messages became 5). In groups the ticks still turn blue on the first reader instead of the last; not fixed. |
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
| Log out | Verified (on copies) | Logging out (two clicks in Settings) unlinks the device, removes the account's chats, media, pictures, saved history and backups from this PC (the look of the app is kept), replaces the device keys and shows the welcome screen with a new code, without a restart. The same happens when the phone unlinks the device, and at the start of a copy that was logged out by an older version. Checked two ways in the test copy: a simulated log out on a scratch folder, and a start on a copy of a really logged-out session; both ended at a fresh code. |
| Welcome and linking | Verified (shown) / Built (phone number) | A copy with no account shows one screen: what linking is, three steps, and the code to scan. "Link with a phone number instead" asks for the number and shows the eight-character code to type on the phone; that side was drawn in the test copy but no code was requested. After linking, the title bar says chats are being fetched. |
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
| Liquid Glass style | Verified (shown) | Rebuilt after Apple's material, where lensing rather than blur makes the glass. Two layers: content (rail, chat list, conversation) lies on plain translucent sheets; the controls floating above it (chat header, message box, chips, menus, dialogs) are lenses drawn by the renderer (a patch to the vendored GPUI, see `vendor/README.md`): nearly unblurred and undistorted in the flat middle, bent hard in a curved bezel by Snell's law, colours parting there, a bezel that catches the light, hairlines of light on two opposite sides, and a density that adapts to what is underneath so text stays readable (milky glass thickens over dark photos, smoked glass over bright ones). Seen in the test copy with photos scrolled under the bars and a menu over them, dark and light. **Limit:** an app on Windows cannot read the pixels behind its own window, so the desktop behind a clear window is never bent, only what the app itself draws; Windows can blur it (Frosted). Not built: glass that flexes or lights up under a press, shapes that merge, the chat list scrolling under a floating header. |
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

## The database incidents (2026-11-11) and their cause

The chat database was damaged twice on 2026-11-11. The cause, found that evening:

Development runs inside the Claude desktop app, which Windows packages in a container.
A program started from there sees a redirected `AppData\Local`: what it writes goes to
the container's private copy of the folder, and a file that copy lacks is silently
taken from the real folder. So there were two data folders all along, each a separately
linked WhatsApp device: the real one (the app started from Explorer; linked by the owner
at 08:44) and the container's (everything started from the development shell; linked at
01:56). A database opened through the merged view could be given the *other* database's
write-ahead log.

- Morning: the container copy's database was found unreadable and was rebuilt from its
  data pages and the saved history files.
- Evening, 21:45: a snapshot tool opened the container copy's `chats.db` while that copy
  had no log of its own. SQLite took the real folder's log (left by the owner's app,
  which had run from 19:53 to 20:15), applied it to the wrong database and deleted it.
  The container copy was damaged; the owner's copy lost what it had written in those 22
  minutes. The repair that followed could not rebuild in place, fell back to restoring a
  backup, and in doing so deleted what it took for the damaged file: the owner's copy's
  `chats.db`.

What was done the same evening: the owner's copy was restored from its own 19:53 backup
(73,694 messages), then given every row the container copy's intact 19:51 backup had
that it lacked (the whole day's messages, which that device had also received) and
every row that could be read back from the damaged file (which held the lost log's
pages). Both copies now hold 73,919 messages and pass SQLite's integrity check. The
damaged file is kept in the container copy's `damaged-1794423405` folder. What cannot be
ruled out: a few messages or setting changes from 19:53 to 20:15 that were in the lost
log and could not be read back.

What prevents a repeat: `src/paths.rs` finds the folder's true location and every file
is opened by it, so a process works on one copy or the other and never on a mixture;
the snapshot for interface tests lives inside the project; the snapshot tool keeps the
previous snapshot until a new one has succeeded; the app says so when it is running on
a container's private copy. The earlier stress tests could not reproduce the damage
because they ran on scratch folders that have no twin in the real location.

