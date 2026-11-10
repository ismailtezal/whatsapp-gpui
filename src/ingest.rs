//! Applies what the connection reports (messages, receipts, chat and contact changes)
//! to the store, and says which parts of the interface need to refresh.

use whatsapp_rust::buffa::{Message as _, MessageField};
use whatsapp_rust::prelude::{Event, InboundMessage};
use whatsapp_rust::wacore::types::message::MessageInfo;
use whatsapp_rust::wacore::types::presence::{ChatActivity, PresenceStatus, ReceiptType};

use crate::content::{self, preview_label};
use crate::events::{Activity, ConnState, UiEvent};
use crate::model::{Ack, ContactRow};
use crate::store::{ChatPatch, Store, Tx};
use crate::wa;

type DbResult<T> = rusqlite::Result<T>;

/// Builds the stored record for a message received live (history gives these ready-made).
pub fn to_record(info: &MessageInfo, message: &wa::Message) -> wa::WebMessageInfo {
    let chat = info.source.chat.to_non_ad_string();
    let sender = info.source.sender.to_non_ad_string();
    let grouped = info.source.is_group || chat.ends_with("@broadcast") || chat.ends_with("@newsletter");

    let mut key = wa::MessageKey::default();
    key.remote_jid = Some(chat);
    key.from_me = Some(info.source.is_from_me);
    key.id = Some(info.id.to_string());
    if grouped && !info.source.is_from_me {
        key.participant = Some(sender.clone());
    }

    let mut record = wa::WebMessageInfo::default();
    record.key = MessageField::some(key);
    record.message = MessageField::some(message.clone());
    record.message_timestamp = Some(info.timestamp.timestamp().max(0) as u64);
    if grouped && !info.source.is_from_me {
        record.participant = Some(sender);
    }
    if !info.push_name.is_empty() {
        record.push_name = Some(info.push_name.to_string());
    }
    if info.source.is_from_me {
        record.status = Some(wa::web_message_info::Status::SERVER_ACK);
    }
    record
}

/// Applies one event. Returns what the interface should refresh.
pub fn apply(store: &Store, event: &Event) -> Vec<UiEvent> {
    let mut out = Vec::new();
    if let Err(error) = apply_inner(store, event, &mut out) {
        out.push(UiEvent::Error(format!("could not store an update: {error}")));
    }
    out
}

fn apply_inner(store: &Store, event: &Event, out: &mut Vec<UiEvent>) -> DbResult<()> {
    match event {
        Event::Connected(_) => out.push(UiEvent::Connection(ConnState::Online)),
        Event::Disconnected(d) => {
            out.push(UiEvent::Connection(ConnState::Offline(format!("{:?}", d.reason))))
        }
        Event::LoggedOut(_) => out.push(UiEvent::Connection(ConnState::LoggedOut)),
        Event::PairingQrCode(qr) => {
            out.push(UiEvent::Connection(ConnState::Pairing));
            out.push(UiEvent::Qr {
                code: qr.code.to_string(),
                valid_secs: qr.timeout.as_secs(),
            });
        }
        Event::PairingCode(code) => out.push(UiEvent::PairCode(format!("{:?}", code))),
        Event::PairSuccess(_) => out.push(UiEvent::Connection(ConnState::Connecting)),

        Event::Messages(batch) => {
            let mut touched: Vec<String> = Vec::new();
            store.batch(|tx| {
                for inbound in batch.iter() {
                    if let Some(chat) = apply_message(tx, inbound, out)? {
                        if !touched.contains(&chat) {
                            touched.push(chat);
                        }
                    }
                }
                Ok(())
            })?;
            if !touched.is_empty() {
                out.push(UiEvent::ChatsChanged);
            }
            for chat in touched {
                out.push(UiEvent::ChatChanged { chat });
            }
        }

        Event::Receipt(receipt) => {
            let status = match receipt.r#type {
                ReceiptType::Delivered => Some(Ack::Delivered),
                ReceiptType::Read => Some(Ack::Read),
                ReceiptType::Played => Some(Ack::Played),
                _ => None,
            };
            let chat = receipt.source.chat.to_non_ad_string();
            let sender = receipt.source.sender.to_non_ad_string();
            let ts = receipt.timestamp.timestamp();
            let mut changed = false;
            store.batch(|tx| {
                let chat = tx.canonical(&chat)?;
                match receipt.r#type {
                    // Our own other device read this chat.
                    ReceiptType::ReadSelf => {
                        tx.set_unread(&chat, 0)?;
                        changed = true;
                    }
                    _ => {}
                }
                if let Some(status) = status
                    && !receipt.source.is_from_me
                {
                    for id in &receipt.message_ids {
                        let id = id.to_string();
                        changed |= tx.set_status(&chat, &id, status)?;
                        let (delivered, read, played) = match status {
                            Ack::Delivered => (Some(ts), None, None),
                            Ack::Read => (None, Some(ts), None),
                            Ack::Played => (None, None, Some(ts)),
                            _ => (None, None, None),
                        };
                        tx.put_receipt(&chat, &id, &sender, delivered, read, played)?;
                    }
                }
                if changed {
                    out.push(UiEvent::ChatsChanged);
                    out.push(UiEvent::ChatChanged { chat });
                }
                Ok(())
            })?;
        }

        Event::ServerAck(ack) => {
            // The server accepted a message we sent: one grey tick.
            if ack.error.is_none()
                && let Some(to) = &ack.from
            {
                let chat = store.canonical(&to.to_non_ad_string())?;
                let changed = store.batch(|tx| tx.set_status(&chat, &ack.id, Ack::Sent))?;
                if changed {
                    out.push(UiEvent::ChatsChanged);
                    out.push(UiEvent::ChatChanged { chat });
                }
            }
        }

        Event::HistorySync(sync) => {
            // Keep the raw blob so the database can always be rebuilt.
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let name = format!(
                "{stamp}-type{}-chunk{}.zlib",
                sync.sync_type(),
                sync.chunk_order().unwrap_or(0)
            );
            let bytes = sync.compressed_bytes();
            let _ = std::fs::write(crate::paths::history_dir().join(name), &bytes[..]);
            match crate::import::import_blob(store, &bytes[..]) {
                Ok(_) => {
                    out.push(UiEvent::HistoryProgress {
                        progress: sync.progress(),
                    });
                    out.push(UiEvent::ChatsChanged);
                }
                Err(error) => out.push(UiEvent::Error(format!("history import failed: {error:#}"))),
            }
        }

        Event::ContactUpdate(update) => {
            let jid = update.jid.to_non_ad_string();
            store.batch(|tx| {
                if let Some(lid) = update.action.lid_jid.as_deref().filter(|l| !l.is_empty()) {
                    tx.learn_lid(lid, &jid)?;
                }
                tx.put_contact(&ContactRow {
                    jid: jid.clone(),
                    lid: update.action.lid_jid.clone(),
                    name: update.action.full_name.clone(),
                    first_name: update.action.first_name.clone(),
                    username: update.action.username.clone(),
                    ..Default::default()
                })
            })?;
            out.push(UiEvent::ChatsChanged);
        }

        Event::PinUpdate(update) => {
            let pinned = update.action.pinned.unwrap_or(false);
            store.patch_chat(
                &update.jid.to_non_ad_string(),
                &ChatPatch {
                    pinned: Some(if pinned { update.timestamp.timestamp().max(1) } else { 0 }),
                    ..Default::default()
                },
            )?;
            out.push(UiEvent::ChatsChanged);
        }

        Event::MuteUpdate(update) => {
            let muted = update.action.muted.unwrap_or(false);
            // -1 means "muted until I say otherwise".
            let until = if muted {
                match update.action.mute_end_timestamp {
                    Some(ts) if ts > 0 => ts / 1000,
                    _ => -1,
                }
            } else {
                0
            };
            store.patch_chat(
                &update.jid.to_non_ad_string(),
                &ChatPatch {
                    mute_until: Some(until),
                    ..Default::default()
                },
            )?;
            out.push(UiEvent::ChatsChanged);
        }

        Event::ArchiveUpdate(update) => {
            store.patch_chat(
                &update.jid.to_non_ad_string(),
                &ChatPatch {
                    archived: Some(update.action.archived.unwrap_or(false)),
                    ..Default::default()
                },
            )?;
            out.push(UiEvent::ChatsChanged);
        }

        Event::MarkChatAsReadUpdate(update) => {
            let read = update.action.read.unwrap_or(true);
            let chat = update.jid.to_non_ad_string();
            store.batch(|tx| {
                if read {
                    tx.set_unread(&chat, 0)
                } else {
                    tx.patch_chat(
                        &chat,
                        &ChatPatch {
                            marked_unread: Some(true),
                            ..Default::default()
                        },
                    )
                }
            })?;
            out.push(UiEvent::ChatsChanged);
        }

        Event::StarUpdate(update) => {
            let chat = update.chat_jid.to_non_ad_string();
            let starred = update.action.starred.unwrap_or(false);
            store.batch(|tx| tx.set_starred(&chat, &update.message_id, starred))?;
            out.push(UiEvent::ChatChanged {
                chat: store.canonical(&chat)?,
            });
        }

        Event::DeleteMessageForMeUpdate(update) => {
            let chat = update.chat_jid.to_non_ad_string();
            store.batch(|tx| tx.delete_message(&chat, &update.message_id))?;
            out.push(UiEvent::ChatsChanged);
            out.push(UiEvent::ChatChanged {
                chat: store.canonical(&chat)?,
            });
        }

        Event::DeleteChatUpdate(update) => {
            let chat = update.jid.to_non_ad_string();
            store.batch(|tx| tx.delete_chat(&chat))?;
            out.push(UiEvent::ChatsChanged);
        }

        Event::ClearChatUpdate(update) => {
            let chat = update.jid.to_non_ad_string();
            store.batch(|tx| tx.clear_chat(&chat))?;
            out.push(UiEvent::ChatsChanged);
            out.push(UiEvent::ChatChanged {
                chat: store.canonical(&chat)?,
            });
        }

        Event::ChatPresence(presence) => {
            let activity = match presence.state {
                ChatActivity::Typing => Activity::Typing,
                ChatActivity::RecordingAudio => Activity::Recording,
                _ => Activity::Idle,
            };
            out.push(UiEvent::Activity {
                chat: store.canonical(&presence.source.chat.to_non_ad_string())?,
                sender: store.canonical(&presence.source.sender.to_non_ad_string())?,
                activity,
            });
        }

        Event::Presence(presence) => out.push(UiEvent::Presence {
            jid: store.canonical(&presence.from.to_non_ad_string())?,
            online: matches!(presence.status, PresenceStatus::Available),
            last_seen: presence.last_seen.map(|t| t.timestamp()),
        }),

        Event::UserAboutUpdate(update) => {
            store.put_contact(&ContactRow {
                jid: update.jid.to_non_ad_string(),
                about: Some(update.status.clone()),
                ..Default::default()
            })?;
        }

        Event::PictureUpdate(update) => out.push(UiEvent::AvatarReady {
            jid: store.canonical(&update.jid.to_non_ad_string())?,
        }),

        Event::SelfPushNameUpdated(_) => {}

        _ => {}
    }
    Ok(())
}

/// Stores one live message or applies the action it carries. Returns the chat it touched.
fn apply_message(tx: &Tx<'_>, inbound: &InboundMessage, out: &mut Vec<UiEvent>) -> DbResult<Option<String>> {
    let info = &inbound.info;
    let message = &inbound.message;

    // A sender seen under both a LID and a phone number is one person.
    if let Some(alt) = &info.source.sender_alt {
        let sender = info.source.sender.to_non_ad_string();
        let alt = alt.to_non_ad_string();
        if sender.ends_with("@lid") {
            tx.learn_lid(&sender, &alt)?;
        } else if alt.ends_with("@lid") {
            tx.learn_lid(&alt, &sender)?;
        }
    }

    let chat = tx.canonical(&info.source.chat.to_non_ad_string())?;
    let sender = if info.source.is_from_me {
        "me".to_string()
    } else {
        tx.canonical(&info.source.sender.to_non_ad_string())?
    };
    let ts = info.timestamp.timestamp();
    let (base, _, _) = content::unwrap(message);

    if let Some(reaction) = base.reaction_message.as_option() {
        if let Some(target) = reaction.key.as_option().and_then(|k| k.id.as_deref()) {
            tx.put_reaction(&chat, target, &sender, reaction.text.as_deref().unwrap_or(""), ts)?;
        }
        return Ok(Some(chat));
    }

    if let Some(protocol) = base.protocol_message.as_option() {
        use wa::message::protocol_message::Type;
        let target = protocol.key.as_option().and_then(|k| k.id.clone());
        match protocol.r#type {
            Some(Type::REVOKE) => {
                if let Some(target) = target {
                    tx.mark_revoked(&chat, &target)?;
                    tx.refresh_last(&chat)?;
                }
                return Ok(Some(chat));
            }
            Some(Type::MESSAGE_EDIT) => {
                if let (Some(target), Some(edited)) = (target, protocol.edited_message.as_option()) {
                    tx.apply_edit(&chat, &target, edited)?;
                }
                return Ok(Some(chat));
            }
            Some(Type::EPHEMERAL_SETTING) => {
                tx.patch_chat(
                    &chat,
                    &ChatPatch {
                        ephemeral: Some(protocol.ephemeral_expiration.unwrap_or(0) as i64),
                        ..Default::default()
                    },
                )?;
                return Ok(Some(chat));
            }
            _ => return Ok(None),
        }
    }

    let record = to_record(info, message);
    let Some(_rowid) = tx.put_message(&record)? else {
        return Ok(None);
    };

    if let Some(seconds) = inbound.ephemeral_expiration {
        tx.patch_chat(
            &chat,
            &ChatPatch {
                ephemeral: Some(seconds as i64),
                ..Default::default()
            },
        )?;
    }

    if !info.source.is_from_me && chat != "status@broadcast" {
        tx.bump_unread(&chat)?;
        if let Some(digest) = content::digest(message) {
            out.push(UiEvent::Incoming {
                chat: chat.clone(),
                id: info.id.to_string(),
                sender: Some(sender),
                preview: preview_label(digest.kind, digest.text.as_deref()),
            });
        }
    } else if info.source.is_from_me {
        // Sent from another of our devices: we have evidently read the chat.
        tx.set_unread(&chat, 0)?;
    }
    Ok(Some(chat))
}

impl Tx<'_> {
    /// Replaces the content of a stored message with its edited version.
    pub fn apply_edit(&self, chat: &str, id: &str, edited: &wa::Message) -> DbResult<()> {
        let chat = self.canonical(chat)?;
        let Some(stored) = self.raw_record(&chat, id)? else {
            return Ok(());
        };
        let Ok(mut record) = wa::WebMessageInfo::decode_from_slice(&stored) else {
            return Ok(());
        };
        record.message = MessageField::some(edited.clone());
        self.put_message(&record)?;
        self.mark_edited(&chat, id)
    }
}

