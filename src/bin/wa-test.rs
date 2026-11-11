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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let action = args.first().map(String::as_str).unwrap_or("online");
    let store = Arc::new(Store::open_default()?);
    let backend = Backend::start(store.clone());

    wait_for(&backend, "connection", Duration::from_secs(40), |event| {
        if let UiEvent::Identity { pn, lid, .. } = event {
            println!("identity: pn={} lid={}", pn.is_some(), lid.is_some());
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
