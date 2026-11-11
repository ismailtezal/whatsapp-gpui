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
                        let mut raised = tx.set_status(&chat, &id, status)?;
                        if !raised
                            && !chat.ends_with("@g.us")
                            && let Some(home) = tx.set_status_by_id(&id, status)?
                        {
                            // The receipt named the person by an address this device
                            // had not tied to them; the message says where it lives.
                            out.push(UiEvent::ChatChanged { chat: home });
                            raised = true;
                        }
                        changed |= raised;
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

        Event::UndecryptableMessage(lost) => {
            // A message arrived that this device cannot read yet. The connection has
            // already asked the sender's phone for it again; until that copy arrives
            // the chat shows "Waiting for this message", as the official client does.
            // The real message has the same ID and takes the placeholder's place.
            use whatsapp_rust::wacore::types::events::DecryptFailMode;
            let info = &lost.info;
            let id = info.id.to_string();
            let chat = store.canonical(&info.source.chat.to_non_ad_string())?;
            let shown = lost.decrypt_fail_mode == DecryptFailMode::Show && !lost.is_unavailable;
            if shown && !info.source.is_from_me && store.message(&chat, &id)?.is_none() {
                let mut record = to_record(info, &wa::Message::default());
                record.message = MessageField::none();
                record.message_stub_type = Some(wa::web_message_info::StubType::CIPHERTEXT.into());
                store.put_message(&record)?;
                out.push(UiEvent::ChatsChanged);
                out.push(UiEvent::ChatChanged { chat });
            }
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

        Event::CallLogHistory(history) => {
            store.put_call(&history.record)?;
        }

        Event::IncomingCall(call) => {
            if let whatsapp_rust::wacore::types::call::CallAction::Offer {
                call_id,
                is_video,
                caller_pn,
                ..
            } = &call.action
            {
                let from = caller_pn.as_ref().unwrap_or(&call.from).to_non_ad_string();
                out.push(UiEvent::IncomingCall {
                    from: store.canonical(&from)?,
                    call_id: call_id.clone(),
                    video: *is_video,
                });
            }
        }

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

    // Someone pinned or unpinned a message for everyone.
    if let Some(pin) = base.pin_in_chat_message.as_option() {
        if let Some(target) = pin.key.as_option().and_then(|k| k.id.as_deref()) {
            let pinned = !format!("{:?}", pin.r#type).to_uppercase().contains("UNPIN");
            tx.set_pinned(&chat, target, pinned, ts)?;
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
                backlog: info.is_offline,
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


#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use whatsapp_rust::wacore::types::events::{BatchOrigin, MessageBatch};
    use whatsapp_rust::wacore::types::message::MessageSource;
    use whatsapp_rust::Jid;

    use super::*;
    use crate::model::MsgKind;

    const ADA: &str = "15550001111@s.whatsapp.net";
    const GROUP: &str = "120363000000000001@g.us";

    fn temp_store(name: &str) -> Store {
        let path = std::env::temp_dir().join(format!(
            "whatsapp-gpui-test-{name}-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        Store::open(&path).expect("open test store")
    }

    fn text(body: &str) -> wa::Message {
        let mut message = wa::Message::default();
        message.conversation = Some(body.to_string());
        message
    }

    /// An event carrying one message, as the connection delivers it.
    fn inbound(chat: &str, sender: &str, from_me: bool, id: &str, message: wa::Message) -> Event {
        let mut info = MessageInfo::default();
        info.source = MessageSource {
            chat: chat.parse::<Jid>().expect("chat jid"),
            sender: sender.parse::<Jid>().expect("sender jid"),
            is_from_me: from_me,
            is_group: chat.ends_with("@g.us"),
            ..Default::default()
        };
        info.id = id.into();
        info.push_name = "Ada".into();
        info.timestamp = whatsapp_rust::chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("time");
        let message = InboundMessage::builder()
            .message(Arc::new(message))
            .info(Arc::new(info))
            .build();
        Event::Messages(
            MessageBatch::builder()
                .messages(Arc::from(vec![message]))
                .origin(BatchOrigin::Live)
                .build(),
        )
    }

    #[test]
    fn an_incoming_text_is_stored_counted_and_announced() {
        let store = temp_store("incoming");
        let updates = apply(&store, &inbound(ADA, ADA, false, "MSG1", text("hello there")));

        let rows = store.messages(ADA, None, 10).expect("read messages");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text.as_deref(), Some("hello there"));
        assert!(!rows[0].from_me);
        assert_eq!(store.unread_count(ADA).expect("unread"), 1);
        // The sender's display name comes from the message.
        assert_eq!(store.display_name(ADA).expect("name"), "Ada");

        assert!(updates.iter().any(|u| matches!(u, UiEvent::ChatsChanged)));
        assert!(updates.iter().any(|u| matches!(u, UiEvent::ChatChanged { chat } if chat == ADA)));
        assert!(updates.iter().any(
            |u| matches!(u, UiEvent::Incoming { chat, preview, .. } if chat == ADA && preview == "hello there")
        ));
        let chats = store.chats(false).expect("chat list");
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].unread, 1);
    }

    #[test]
    fn a_receipt_that_arrives_before_its_message_still_ticks_it() {
        let store = temp_store("early-receipt");
        // The other person's "read" for a message sent from the phone gets here first.
        store
            .batch(|tx| tx.put_receipt(ADA, "MSG9", ADA, Some(1_700_000_001), Some(1_700_000_002), None))
            .expect("receipt");
        apply(&store, &inbound(ADA, ADA, true, "MSG9", text("typed on my phone")));
        let rows = store.messages(ADA, None, 10).expect("read messages");
        assert_eq!(rows[0].ack, Ack::Read);
    }

    #[test]
    fn old_rows_are_repaired_from_the_receipts_on_record() {
        let store = temp_store("repair-receipts");
        apply(&store, &inbound(ADA, ADA, true, "MSG7", text("one tick for ever")));
        apply(&store, &inbound(ADA, ADA, true, "MSG8", text("no receipt")));
        // Put the row back to how the old code left it, then record a receipt for it.
        store.batch(|tx| tx.force_status(ADA, "MSG7", Ack::Sent)).expect("reset");
        store
            .batch(|tx| tx.put_receipt(ADA, "MSG7", ADA, Some(1_700_000_001), None, None))
            .expect("receipt");
        assert_eq!(store.batch(|tx| tx.reapply_receipts()).expect("repair"), 1);
        let rows = store.messages(ADA, None, 10).expect("read messages");
        let status = |id: &str| rows.iter().find(|row| row.id == id).map(|row| row.ack);
        assert_eq!(status("MSG7"), Some(Ack::Delivered));
        assert_eq!(status("MSG8"), Some(Ack::Sent));
    }

    #[test]
    fn logging_out_forgets_the_account_but_not_the_look() {
        let store = temp_store("forget");
        apply(&store, &inbound(ADA, ADA, false, "MSG1", text("hello there")));
        store.set_kv("style", "liquid").expect("style");
        store.set_kv("account_state_replayed", "1").expect("flag");
        store.forget_account().expect("forget");
        assert!(store.messages(ADA, None, 10).expect("messages").is_empty());
        assert!(store.chats(false).expect("chats").is_empty());
        assert_eq!(store.kv("style").expect("style").as_deref(), Some("liquid"));
        assert_eq!(store.kv("account_state_replayed").expect("flag"), None);
        // The store still works for the next account.
        apply(&store, &inbound(ADA, ADA, false, "MSG2", text("after linking again")));
        assert_eq!(store.messages(ADA, None, 10).expect("messages").len(), 1);
    }

    #[test]
    fn a_message_from_our_other_device_does_not_count_as_unread() {
        let store = temp_store("own");
        let updates = apply(&store, &inbound(ADA, ADA, true, "MSG1", text("sent from my phone")));
        assert_eq!(store.unread_count(ADA).expect("unread"), 0);
        assert!(!updates.iter().any(|u| matches!(u, UiEvent::Incoming { .. })));
        assert!(store.messages(ADA, None, 10).expect("read")[0].from_me);
    }

    #[test]
    fn a_group_message_keeps_its_author() {
        let store = temp_store("group");
        apply(&store, &inbound(GROUP, ADA, false, "MSG1", text("hi all")));
        let rows = store.messages(GROUP, None, 10).expect("read");
        assert_eq!(rows[0].sender.as_deref(), Some(ADA));
        let chats = store.chats(false).expect("chat list");
        assert_eq!(chats[0].kind, crate::model::ChatKind::Group);
    }

    #[test]
    fn a_reaction_attaches_to_its_message_instead_of_becoming_one() {
        let store = temp_store("reaction");
        apply(&store, &inbound(ADA, ADA, false, "MSG1", text("nice")));

        let mut key = wa::MessageKey::default();
        key.id = Some("MSG1".to_string());
        let mut reaction = wa::message::ReactionMessage::default();
        reaction.key = MessageField::some(key);
        reaction.text = Some("👍".to_string());
        let mut message = wa::Message::default();
        message.reaction_message = MessageField::some(reaction);
        apply(&store, &inbound(ADA, ADA, false, "REACT1", message));

        assert_eq!(store.messages(ADA, None, 10).expect("read").len(), 1);
        let reactions = store.reactions(ADA, &["MSG1".to_string()]).expect("reactions");
        assert_eq!(reactions, vec![("MSG1".to_string(), ADA.to_string(), "👍".to_string())]);
    }

    #[test]
    fn deleting_for_everyone_leaves_a_placeholder() {
        let store = temp_store("revoke");
        apply(&store, &inbound(ADA, ADA, false, "MSG1", text("oops")));

        let mut key = wa::MessageKey::default();
        key.id = Some("MSG1".to_string());
        let mut protocol = wa::message::ProtocolMessage::default();
        protocol.key = MessageField::some(key);
        protocol.r#type = Some(wa::message::protocol_message::Type::REVOKE);
        let mut message = wa::Message::default();
        message.protocol_message = MessageField::some(protocol);
        apply(&store, &inbound(ADA, ADA, false, "REVOKE1", message));

        let rows = store.messages(ADA, None, 10).expect("read");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, MsgKind::Revoked);
        assert_eq!(rows[0].text, None);
    }

    #[test]
    fn an_edit_replaces_the_text_and_marks_the_message() {
        let store = temp_store("edit");
        apply(&store, &inbound(ADA, ADA, false, "MSG1", text("teh")));

        let mut key = wa::MessageKey::default();
        key.id = Some("MSG1".to_string());
        let mut protocol = wa::message::ProtocolMessage::default();
        protocol.key = MessageField::some(key);
        protocol.r#type = Some(wa::message::protocol_message::Type::MESSAGE_EDIT);
        protocol.edited_message = MessageField::some(text("the"));
        let mut message = wa::Message::default();
        message.protocol_message = MessageField::some(protocol);
        apply(&store, &inbound(ADA, ADA, false, "EDIT1", message));

        let rows = store.messages(ADA, None, 10).expect("read");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text.as_deref(), Some("the"));
        assert!(rows[0].edited);
    }

    #[test]
    fn a_lid_and_phone_number_fold_into_one_chat() {
        let store = temp_store("lid");
        let lid = "99990000111122@lid";
        apply(&store, &inbound(lid, lid, false, "MSG1", text("from my hidden id")));
        assert_eq!(store.messages(lid, None, 10).expect("read").len(), 1);

        store.learn_lid(lid, ADA).expect("learn mapping");
        // The message moved to the phone-number chat, and new LID traffic lands there too.
        assert_eq!(store.messages(ADA, None, 10).expect("read").len(), 1);
        apply(&store, &inbound(lid, lid, false, "MSG2", text("again")));
        assert_eq!(store.messages(ADA, None, 10).expect("read").len(), 2);
        assert_eq!(store.chats(false).expect("chats").len(), 1);
    }
}
