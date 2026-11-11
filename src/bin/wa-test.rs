//! Headless end-to-end checks against the real connection, for development.
//!
//! Connects with the linked session (the GUI must be closed: one connection per device),
//! runs one action, prints what happened and exits. Sends are refused unless the target is
//! a group, so a slip can never message a person.
//!
//! Usage:
//!   wa-test online                        connect and report identity
//!   wa-test send <group> <text>           send a text
//!   wa-test file <group> <path> [caption] send a file
//!   wa-test react <group> <emoji>         react to the newest message
//!   wa-test edit <group> <text>           edit our newest text message
//!   wa-test revoke <group>                delete our newest message for everyone
//!   wa-test download <group> [n]          download media of the newest n messages
//!   wa-test listen [seconds]              stay connected and count events

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use whatsapp_gpui::backend::Backend;
use whatsapp_gpui::events::{Command, ConnState, UiEvent};
use whatsapp_gpui::media_ops::attach_kind_for;
use whatsapp_gpui::model::{Ack, ChatKind};
use whatsapp_gpui::store::Store;

fn wait_for(backend: &Backend, what: &str, limit: Duration, mut hit: impl FnMut(&UiEvent) -> bool) -> Result<()> {
    let events = backend.events();
    let deadline = Instant::now() + limit;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            bail!("timed out waiting for {what}");
        }
        // Poll in short steps so the deadline is honoured.
        match events.try_recv() {
            Ok(event) => {
                if let UiEvent::Error(error) = &event {
                    println!("  error event: {error}");
                }
                if hit(&event) {
                    return Ok(());
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// Resolves a group by (part of) its title. Only groups are accepted as send targets.
fn group(store: &Store, needle: &str) -> Result<String> {
    let matches: Vec<_> = store
        .find_chats(needle)?
        .into_iter()
        .filter(|c| c.kind == ChatKind::Group)
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.jid.clone()),
        [] => Err(anyhow!("no group matches {needle:?}")),
        many => Err(anyhow!("{} groups match {needle:?}; be more specific", many.len())),
    }
}

fn newest(store: &Store, chat: &str, mine: bool) -> Result<whatsapp_gpui::model::MsgRow> {
    store
        .messages(chat, None, 30)?
        .into_iter()
        .rev()
        .find(|m| !mine || m.from_me)
        .ok_or_else(|| anyhow!("no message found"))
}

fn wait_ack(store: &Store, chat: &str, id: &str, limit: Duration) -> Result<Ack> {
    let deadline = Instant::now() + limit;
    loop {
        let ack = store
            .message(chat, id)?
            .map(|m| m.ack)
            .unwrap_or(Ack::Pending);
        if ack >= Ack::Sent || ack == Ack::Error || Instant::now() > deadline {
            return Ok(ack);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn main() -> Result<()> {
    // Started as the voice encoder child: do that one job and leave.
    if whatsapp_gpui::voice::run_child_if_requested() {
        return Ok(());
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let action = args.first().map(String::as_str).unwrap_or("online");
    let store = Arc::new(Store::open_default()?);
    let backend = Backend::start(store.clone());

    let mut own_name: Option<String> = None;
    wait_for(&backend, "connection", Duration::from_secs(40), |event| {
        if let UiEvent::Identity { pn, lid, push_name } = event {
            println!("identity: pn={} lid={}", pn.is_some(), lid.is_some());
            own_name = Some(push_name.clone());
        }
        matches!(event, UiEvent::Connection(ConnState::Online))
    })?;
    println!("online");
    // Let the offline queue drain before acting.
    std::thread::sleep(Duration::from_secs(2));

    match action {
        "online" => {}
        "send" => {
            let chat = group(&store, &args[1])?;
            let text = args[2..].join(" ");
            let before = newest(&store, &chat, true).map(|m| m.id).unwrap_or_default();
            backend.send(Command::SendText {
                chat: chat.clone(),
                text,
                reply_to: None,
                mentions: Vec::new(),
            });
            std::thread::sleep(Duration::from_millis(600));
            let sent = newest(&store, &chat, true)?;
            if sent.id == before {
                bail!("no new outgoing message was stored");
            }
            println!("sent id={} ack={:?}", sent.id, wait_ack(&store, &chat, &sent.id, Duration::from_secs(20))?);
        }
        "file" => {
            let chat = group(&store, &args[1])?;
            let path = std::path::PathBuf::from(&args[2]);
            let before = newest(&store, &chat, true).map(|m| m.id).unwrap_or_default();
            backend.send(Command::SendFile {
                chat: chat.clone(),
                kind: attach_kind_for(&path),
                path,
                caption: args.get(3).cloned(),
                reply_to: None,
            });
            let deadline = Instant::now() + Duration::from_secs(60);
            let sent = loop {
                let newest = newest(&store, &chat, true)?;
                if newest.id != before {
                    break newest;
                }
                if Instant::now() > deadline {
                    bail!("upload did not finish in time");
                }
                std::thread::sleep(Duration::from_millis(200));
            };
            println!(
                "sent file id={} kind={:?} ack={:?}",
                sent.id,
                sent.kind,
                wait_ack(&store, &chat, &sent.id, Duration::from_secs(30))?
            );
        }
        "react" => {
            let chat = group(&store, &args[1])?;
            let target = newest(&store, &chat, false)?;
            backend.send(Command::React {
                chat: chat.clone(),
                id: target.id.clone(),
                emoji: args.get(2).cloned().unwrap_or_default(),
            });
            std::thread::sleep(Duration::from_secs(3));
            let reactions = store.reactions(&chat, &[target.id.clone()])?;
            println!("reactions on {}: {}", target.id, reactions.len());
        }
        "edit" => {
            let chat = group(&store, &args[1])?;
            let target = newest(&store, &chat, true)?;
            backend.send(Command::Edit {
                chat: chat.clone(),
                id: target.id.clone(),
                text: args[2..].join(" "),
            });
            std::thread::sleep(Duration::from_secs(3));
            let after = store.message(&chat, &target.id)?.ok_or_else(|| anyhow!("gone"))?;
            println!("edited={} id={}", after.edited, after.id);
        }
        "revoke" => {
            let chat = group(&store, &args[1])?;
            let target = newest(&store, &chat, true)?;
            backend.send(Command::Revoke {
                chat: chat.clone(),
                id: target.id.clone(),
            });
            std::thread::sleep(Duration::from_secs(3));
            let after = store.message(&chat, &target.id)?.ok_or_else(|| anyhow!("gone"))?;
            println!("kind after revoke: {:?}", after.kind);
        }
        "download" => {
            let chat = group(&store, &args[1])?;
            let count: usize = args.get(2).and_then(|n| n.parse().ok()).unwrap_or(3);
            let targets: Vec<_> = store
                .messages(&chat, None, 60)?
                .into_iter()
                .rev()
                .filter(|m| m.kind.is_media())
                .take(count)
                .collect();
            for target in &targets {
                backend.send(Command::DownloadMedia {
                    chat: chat.clone(),
                    id: target.id.clone(),
                });
            }
            let mut done = 0;
            let total = targets.len();
            let _ = wait_for(&backend, "downloads", Duration::from_secs(60), |event| {
                if let UiEvent::MediaReady { path, error, .. } = event {
                    done += 1;
                    let size = path
                        .as_ref()
                        .and_then(|p| std::fs::metadata(p).ok())
                        .map(|m| m.len());
                    println!("  media ready: bytes={size:?} error={error:?}");
                }
                done >= total
            });
            println!("downloaded {done}/{total}");
        }
        "voice" => {
            // Download-only: fetch the newest voice message from any chat and print its path.
            let mut found = None;
            for chat in store.chats(false)? {
                if let Some(msg) = store
                    .messages(&chat.jid, None, 400)?
                    .into_iter()
                    .rev()
                    .find(|m| m.kind == whatsapp_gpui::model::MsgKind::Voice)
                {
                    found = Some((chat.jid.clone(), msg));
                    break;
                }
            }
            let (chat, msg) = found.ok_or_else(|| anyhow!("no voice message in history"))?;
            backend.send(Command::DownloadMedia {
                chat,
                id: msg.id.clone(),
            });
            let mut result = None;
            let _ = wait_for(&backend, "voice download", Duration::from_secs(40), |event| {
                if let UiEvent::MediaReady { path, error, .. } = event {
                    result = Some((path.clone(), error.clone()));
                    return true;
                }
                false
            });
            match result {
                Some((Some(path), _)) => println!("VOICE_PATH={path}"),
                Some((None, error)) => println!("voice download failed: {error:?}"),
                None => println!("voice download timed out"),
            }
        }
        "older" => {
            // Ask the phone for messages older than the oldest stored one of a group.
            let chat = group(&store, &args[1])?;
            let before = store.message_count(Some(&chat))?;
            backend.send(Command::LoadOlder { chat: chat.clone() });
            let _ = wait_for(&backend, "history from the phone", Duration::from_secs(45), |event| {
                matches!(event, UiEvent::HistoryProgress { .. })
            });
            std::thread::sleep(Duration::from_secs(1));
            println!("messages before={before} after={}", store.message_count(Some(&chat))?);
        }
        "reply" => {
            // Reply to the newest message of a group, quoting it.
            let chat = group(&store, &args[1])?;
            let target = newest(&store, &chat, false)?;
            backend.send(Command::SendText {
                chat: chat.clone(),
                text: args[2..].join(" "),
                reply_to: Some(target.id.clone()),
                mentions: Vec::new(),
            });
            std::thread::sleep(Duration::from_millis(800));
            let sent = newest(&store, &chat, true)?;
            println!(
                "reply id={} quotes_target={} ack={:?}",
                sent.id,
                sent.quoted_id.as_deref() == Some(target.id.as_str()),
                wait_ack(&store, &chat, &sent.id, Duration::from_secs(20))?
            );
        }
        "group" => {
            // Read-only: refresh a group's details from the server and count its members.
            let chat = group(&store, &args[1])?;
            let before = store.group_members(&chat)?.len();
            backend.send(Command::RefreshGroup { chat: chat.clone() });
            std::thread::sleep(Duration::from_secs(4));
            let members = store.group_members(&chat)?;
            let admins = members.iter().filter(|(_, rank)| *rank > 0).count();
            println!("members before={before} after={} admins={admins}", members.len());
        }
        "poll" => {
            // Create a poll: wa-test poll <group> "Question | Option A | Option B"
            let chat = group(&store, &args[1])?;
            let parts: Vec<String> = args[2..]
                .join(" ")
                .split('|')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect();
            let (question, options) = parts.split_first().ok_or_else(|| anyhow!("give a question"))?;
            backend.send(Command::CreatePoll {
                chat: chat.clone(),
                question: question.clone(),
                options: options.to_vec(),
                multiple: false,
            });
            std::thread::sleep(Duration::from_secs(4));
            let sent = newest(&store, &chat, true)?;
            let view = store.poll(&chat, &sent.id, &sent.proto);
            println!(
                "poll id={} kind={:?} options={:?}",
                sent.id,
                sent.kind,
                view.map(|v| v.options.len())
            );
        }
        "vote" => {
            // Vote on the newest poll of a group: wa-test vote <group> <option name>
            let chat = group(&store, &args[1])?;
            let poll = store
                .messages(&chat, None, 60)?
                .into_iter()
                .rev()
                .find(|m| m.kind == whatsapp_gpui::model::MsgKind::Poll)
                .ok_or_else(|| anyhow!("no poll found"))?;
            backend.send(Command::VotePoll {
                chat: chat.clone(),
                id: poll.id.clone(),
                options: vec![args[2..].join(" ")],
            });
            std::thread::sleep(Duration::from_secs(4));
            let view = store.poll(&chat, &poll.id, &poll.proto).ok_or_else(|| anyhow!("not a poll"))?;
            let tally: Vec<(usize, bool)> = view.options.iter().map(|o| (o.votes, o.mine)).collect();
            println!("voters={} tally(votes, mine)={tally:?}", view.voters);
        }
        "mkgroup" => {
            // Create an empty group (nobody else is added): wa-test mkgroup <subject>
            let subject = args[1..].join(" ");
            backend.send(Command::CreateGroup {
                subject: subject.clone(),
                members: Vec::new(),
            });
            std::thread::sleep(Duration::from_secs(5));
            let created = store
                .find_chats(&subject)?
                .into_iter()
                .find(|c| c.kind == ChatKind::Group);
            match created {
                Some(chat) => println!(
                    "group created: jid_suffix={} members={}",
                    chat.jid.rsplit('@').next().unwrap_or_default(),
                    store.group_members(&chat.jid)?.len()
                ),
                None => println!("group was not created"),
            }
        }
        "subject" => {
            let chat = group(&store, &args[1])?;
            let subject = args[2..].join(" ");
            backend.send(Command::SetSubject {
                chat: chat.clone(),
                subject: subject.clone(),
            });
            std::thread::sleep(Duration::from_secs(4));
            backend.send(Command::RefreshGroup { chat: chat.clone() });
            std::thread::sleep(Duration::from_secs(3));
            let title = store.chat(&chat)?.map(|c| c.title).unwrap_or_default();
            println!("subject now matches: {}", title == subject);
        }
        "describe" => {
            let chat = group(&store, &args[1])?;
            let description = args[2..].join(" ");
            backend.send(Command::SetDescription {
                chat: chat.clone(),
                description: description.clone(),
            });
            std::thread::sleep(Duration::from_secs(4));
            backend.send(Command::RefreshGroup { chat: chat.clone() });
            std::thread::sleep(Duration::from_secs(3));
            let stored = store.chat_details(&chat)?.and_then(|(d, _, _)| d).unwrap_or_default();
            println!("description now matches: {}", stored == description);
        }
        "leave" => {
            let chat = group(&store, &args[1])?;
            backend.send(Command::LeaveGroup { chat: chat.clone() });
            std::thread::sleep(Duration::from_secs(4));
            println!("leave sent for group");
        }
        "groupsettings" => {
            // On a group the tests own (nobody else in it): flip each admin switch on and
            // back off, set and clear the message timer, read the invite link, reset it.
            use whatsapp_gpui::events::{GroupSetting, GroupSettings};
            let chat = group(&store, &args[1])?;
            let read = |store: &Store| -> Option<GroupSettings> {
                store
                    .kv(&GroupSettings::key(&chat))
                    .ok()
                    .flatten()
                    .and_then(|text| GroupSettings::decode(&text))
            };
            let settle = |what: &str, want: &dyn Fn(&GroupSettings) -> bool| -> Result<()> {
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    if read(&store).is_some_and(|settings| want(&settings)) {
                        println!("ok: {what}");
                        return Ok(());
                    }
                    if Instant::now() > deadline {
                        bail!("timed out: {what} (now {:?})", read(&store));
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            };
            backend.send(Command::RefreshGroup { chat: chat.clone() });
            settle("settings read", &|_| true)?;
            println!("start: {:?}", read(&store));

            backend.send(Command::SetGroupSetting { chat: chat.clone(), setting: GroupSetting::Locked(true) });
            settle("only admins edit info: on", &|s| s.locked)?;
            backend.send(Command::SetGroupSetting { chat: chat.clone(), setting: GroupSetting::Locked(false) });
            settle("only admins edit info: off", &|s| !s.locked)?;

            backend.send(Command::SetGroupSetting { chat: chat.clone(), setting: GroupSetting::Announce(true) });
            settle("only admins send: on", &|s| s.announce)?;
            backend.send(Command::SetGroupSetting { chat: chat.clone(), setting: GroupSetting::Announce(false) });
            settle("only admins send: off", &|s| !s.announce)?;

            backend.send(Command::SetGroupSetting { chat: chat.clone(), setting: GroupSetting::Approval(true) });
            settle("approve new members: on", &|s| s.approval)?;
            backend.send(Command::JoinRequests { chat: chat.clone() });
            wait_for(&backend, "join requests", Duration::from_secs(30), |event| {
                if let UiEvent::JoinRequests { people, .. } = event {
                    println!("ok: requests to join read ({})", people.len());
                    return true;
                }
                false
            })?;
            backend.send(Command::SetGroupSetting { chat: chat.clone(), setting: GroupSetting::Approval(false) });
            settle("approve new members: off", &|s| !s.approval)?;

            backend.send(Command::SetTimer { chat: chat.clone(), seconds: 604_800 });
            settle("timer: 7 days", &|s| s.timer == 604_800)?;
            backend.send(Command::SetTimer { chat: chat.clone(), seconds: 0 });
            settle("timer: off", &|s| s.timer == 0)?;

            let mut first = String::new();
            backend.send(Command::GroupInvite { chat: chat.clone(), reset: false });
            wait_for(&backend, "invite link", Duration::from_secs(30), |event| {
                if let UiEvent::InviteLink { link, .. } = event {
                    first = link.clone();
                    return true;
                }
                false
            })?;
            let mut second = String::new();
            backend.send(Command::GroupInvite { chat: chat.clone(), reset: true });
            wait_for(&backend, "new invite link", Duration::from_secs(30), |event| {
                if let UiEvent::InviteLink { link, .. } = event {
                    second = link.clone();
                    return true;
                }
                false
            })?;
            println!(
                "ok: invite link read (looks right: {}), reset gave a different one: {}",
                first.starts_with("https://chat.whatsapp.com/"),
                !second.is_empty() && second != first
            );
            // Cancel the link again so no usable invite is left behind.
            backend.send(Command::GroupInvite { chat: chat.clone(), reset: true });
            wait_for(&backend, "final link reset", Duration::from_secs(30), |event| {
                matches!(event, UiEvent::InviteLink { .. })
            })?;
        }
        "account" => {
            // Exercise the profile and privacy calls without changing anything: set the
            // name to the name it already is, and one privacy setting to its current value.
            backend.send(Command::FetchAccountInfo);
            let mut privacy = Vec::new();
            wait_for(&backend, "account info", Duration::from_secs(30), |event| {
                if let UiEvent::AccountInfo { privacy: settings, .. } = event {
                    privacy = settings.clone();
                    return true;
                }
                false
            })?;
            println!("privacy settings read: {}", privacy.len());
            let settable = ["Last", "Profile", "Status", "GroupAdd", "ReadReceipts", "Messages"];
            let simple = ["All", "Contacts", "None"];
            if let Some((category, value)) = privacy
                .iter()
                .find(|(category, value)| settable.contains(&category.as_str()) && simple.contains(&value.as_str()))
                .cloned()
            {
                backend.send(Command::SetPrivacy {
                    category: category.clone(),
                    value: value.clone(),
                });
                let mut after = Vec::new();
                wait_for(&backend, "privacy change", Duration::from_secs(30), |event| match event {
                    UiEvent::AccountInfo { privacy: settings, .. } => {
                        after = settings.clone();
                        true
                    }
                    UiEvent::Error(message) => {
                        println!("privacy: {message}");
                        true
                    }
                    _ => false,
                })?;
                let same = after.iter().any(|(c, v)| *c == category && *v == value);
                println!("privacy setting re-applied with its own value: accepted={same}");
            } else {
                println!("no simple privacy setting to re-apply");
            }
            match own_name.clone().filter(|name| !name.is_empty()) {
                Some(name) => {
                    backend.send(Command::SetProfileName { name });
                    let mut said = String::new();
                    wait_for(&backend, "name change", Duration::from_secs(30), |event| {
                        if let UiEvent::Error(message) = event {
                            said = message.clone();
                            return true;
                        }
                        false
                    })?;
                    println!("name re-applied with its own value: {said}");
                }
                None => println!("the account's name is not known; name call skipped"),
            }
        }
        "status" => {
            // Post a text status that only this account receives (nobody else is sent
            // anything), check the server accepts it, then take it down again.
            let text = format!("whatsapp-gpui status check {}", std::process::id());
            backend.send(Command::PostStatus {
                text: text.clone(),
                background: 0xFF1E6E4F,
                file: None,
                only_me: true,
            });
            let chat = "status@broadcast".to_string();
            let deadline = Instant::now() + Duration::from_secs(40);
            let posted = loop {
                if let Some(row) = store
                    .status_updates(0)?
                    .into_iter()
                    .rev()
                    .find(|row| row.from_me && row.text.as_deref() == Some(text.as_str()))
                    && row.ack != whatsapp_gpui::model::Ack::Pending
                {
                    break row;
                }
                if Instant::now() > deadline {
                    bail!("the status was not accepted in time");
                }
                std::thread::sleep(Duration::from_millis(200));
            };
            println!("posted status id={} ack={:?}", posted.id, posted.ack);
            backend.send(Command::DeleteStatus { id: posted.id.clone() });
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let row = store.message(&chat, &posted.id)?;
                if row.is_some_and(|row| row.kind == whatsapp_gpui::model::MsgKind::Revoked) {
                    println!("status removed again");
                    break;
                }
                if Instant::now() > deadline {
                    bail!("the status was not removed in time");
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        "voicenote" => {
            // Send a voice message made from a two-second tone (no microphone involved)
            // to a group, to check that the server accepts what the encoder produces.
            let chat = group(&store, &args[1])?;
            let rate = 48_000u32;
            let samples: Vec<f32> = (0..rate as usize * 2)
                .map(|i| {
                    let t = i as f32 / rate as f32;
                    // A rising tone that fades in and out, so the waveform has a shape.
                    let envelope = (t * std::f32::consts::PI / 2.).sin();
                    (t * (330. + 110. * t) * std::f32::consts::TAU).sin() * 0.4 * envelope
                })
                .collect();
            let recording = whatsapp_gpui::voice::finish_isolated(&samples, rate)?;
            let folder = whatsapp_gpui::paths::media_dir();
            std::fs::create_dir_all(&folder)?;
            let path = folder.join("voice-test.ogg");
            recording.save(&path)?;
            println!(
                "encoded {} bytes, {} s, waveform bars {}",
                recording.ogg.len(),
                recording.seconds,
                recording.waveform.len()
            );
            let before = newest(&store, &chat, true).map(|m| m.id).unwrap_or_default();
            backend.send(Command::SendFile {
                chat: chat.clone(),
                kind: whatsapp_gpui::events::Attach::Voice,
                path,
                caption: None,
                reply_to: None,
            });
            let deadline = Instant::now() + Duration::from_secs(60);
            let sent = loop {
                let newest = newest(&store, &chat, true)?;
                if newest.id != before {
                    break newest;
                }
                if Instant::now() > deadline {
                    bail!("voice message upload did not finish in time");
                }
                std::thread::sleep(Duration::from_millis(200));
            };
            println!(
                "sent voice message id={} kind={:?} ack={:?}",
                sent.id,
                sent.kind,
                wait_ack(&store, &chat, &sent.id, Duration::from_secs(30))?
            );
        }
        "sticker" => {
            // Fetch a sticker from history (download only), then send it to a group.
            let chat = group(&store, &args[1])?;
            let mut path = store.sticker_paths(1)?.into_iter().next();
            if path.is_none() {
                let mut found = None;
                for candidate in store.chats(false)? {
                    if let Some(msg) = store
                        .messages(&candidate.jid, None, 600)?
                        .into_iter()
                        .rev()
                        .find(|m| m.kind == whatsapp_gpui::model::MsgKind::Sticker)
                    {
                        found = Some((candidate.jid.clone(), msg.id));
                        break;
                    }
                }
                let (from, id) = found.ok_or_else(|| anyhow!("no sticker in history"))?;
                backend.send(Command::DownloadMedia { chat: from, id });
                let _ = wait_for(&backend, "sticker download", Duration::from_secs(40), |event| {
                    if let UiEvent::MediaReady { path: ready, .. } = event {
                        path = ready.clone();
                        return true;
                    }
                    false
                });
            }
            let path = std::path::PathBuf::from(path.ok_or_else(|| anyhow!("could not get a sticker file"))?);
            let before = newest(&store, &chat, true).map(|m| m.id).unwrap_or_default();
            backend.send(Command::SendFile {
                chat: chat.clone(),
                kind: whatsapp_gpui::events::Attach::Sticker,
                path,
                caption: None,
                reply_to: None,
            });
            let deadline = Instant::now() + Duration::from_secs(60);
            let sent = loop {
                let newest = newest(&store, &chat, true)?;
                if newest.id != before {
                    break newest;
                }
                if Instant::now() > deadline {
                    bail!("sticker upload did not finish in time");
                }
                std::thread::sleep(Duration::from_millis(200));
            };
            println!(
                "sent sticker id={} kind={:?} ack={:?}",
                sent.id,
                sent.kind,
                wait_ack(&store, &chat, &sent.id, Duration::from_secs(30))?
            );
        }
        "fetch" => {
            // Download-only: fetch the newest message of a kind (gif, video, sticker, …)
            // from any chat and print where it was saved.
            let wanted = args.get(1).map(String::as_str).unwrap_or("gif").to_lowercase();
            let mut found = None;
            'chats: for chat in store.chats(false)? {
                for msg in store.messages(&chat.jid, None, 400)?.into_iter().rev() {
                    if format!("{:?}", msg.kind).to_lowercase() == wanted {
                        found = Some((chat.jid.clone(), msg));
                        break 'chats;
                    }
                }
            }
            let (chat, msg) = found.ok_or_else(|| anyhow!("no {wanted} message in history"))?;
            backend.send(Command::DownloadMedia {
                chat,
                id: msg.id.clone(),
            });
            let mut result = None;
            let _ = wait_for(&backend, "download", Duration::from_secs(60), |event| {
                if let UiEvent::MediaReady { path, error, .. } = event {
                    result = Some((path.clone(), error.clone()));
                    return true;
                }
                false
            });
            match result {
                Some((Some(path), _)) => println!("FETCHED_PATH={path}"),
                Some((None, error)) => println!("download failed: {error:?}"),
                None => println!("download timed out"),
            }
        }
        "pin" => {
            // Pin the newest own text message of a group, then unpin it again.
            let chat = group(&store, &args[1])?;
            let target = store
                .messages(&chat, None, 40)?
                .into_iter()
                .rev()
                .find(|m| m.from_me && m.kind == whatsapp_gpui::model::MsgKind::Text)
                .ok_or_else(|| anyhow!("no own text message to pin"))?;
            backend.send(Command::PinMessage {
                chat: chat.clone(),
                id: target.id.clone(),
                pinned: true,
            });
            std::thread::sleep(Duration::from_secs(4));
            let pinned = store.pinned(&chat)?.contains(&target.id);
            backend.send(Command::PinMessage {
                chat: chat.clone(),
                id: target.id.clone(),
                pinned: false,
            });
            std::thread::sleep(Duration::from_secs(4));
            let unpinned = !store.pinned(&chat)?.contains(&target.id);
            println!("pinned={pinned} then unpinned={unpinned}");
        }
        "listen" => {
            let seconds: u64 = args.get(1).and_then(|n| n.parse().ok()).unwrap_or(15);
            let mut counts = std::collections::BTreeMap::new();
            let _ = wait_for(&backend, "listen window", Duration::from_secs(seconds), |event| {
                let name = format!("{event:?}");
                let name = name.split([' ', '(', '{']).next().unwrap_or("?").to_string();
                *counts.entry(name).or_insert(0) += 1;
                false
            });
            println!("{counts:?}");
        }
        other => bail!("unknown action {other}"),
    }

    backend.send(Command::Shutdown);
    std::thread::sleep(Duration::from_millis(1500));
    Ok(())
}
