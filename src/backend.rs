//! The live connection to WhatsApp.
//!
//! Runs the connection library on its own thread and async runtime. The interface talks to
//! it through two channels: [`Command`]s go in, [`UiEvent`]s come out. Everything the
//! connection learns is written to the [`Store`] first; events only say what to re-read.

use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use whatsapp_rust::async_channel;
use whatsapp_rust::buffa::MessageField;
use whatsapp_rust::prelude::*;
use whatsapp_rust::proto_helpers::MessageExt;
use whatsapp_rust::wacore::store::DevicePropsOverride;
use whatsapp_rust::wacore::store::device::default_history_sync_config;
use whatsapp_rust::waproto::whatsapp::device_props::{HistorySyncConfig, PlatformType};
use whatsapp_rust::{Jid, MessageId, MessageRef};

use crate::content::bare_jid;
use crate::events::{Activity, Command, ConnState, UiEvent};
use crate::model::{Ack, ChatKind};
use crate::store::{ChatPatch, Store};
use crate::{ingest, wa};

/// Handle the interface keeps: send commands, receive events, read the store.
#[derive(Clone)]
pub struct Backend {
    commands: tokio::sync::mpsc::UnboundedSender<Command>,
    events: async_channel::Receiver<UiEvent>,
    store: Arc<Store>,
}

/// Test runs set `WA_READONLY=1`: nothing that other people can see is sent on its own
/// (no read receipts, no "online", no typing indicator). Explicit sends still work.
pub fn read_only() -> bool {
    std::env::var("WA_READONLY").is_ok_and(|v| v == "1")
}

impl Backend {
    pub fn start(store: Arc<Store>) -> Self {
        let (commands, command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, events) = async_channel::unbounded();
        let thread_store = store.clone();
        std::thread::Builder::new()
            .name("wa-backend".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .worker_threads(2)
                    .thread_name("wa-runtime")
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = event_tx.send_blocking(UiEvent::Error(format!(
                            "could not start the connection runtime: {error}"
                        )));
                        return;
                    }
                };
                let failure_tx = event_tx.clone();
                if let Err(error) = runtime.block_on(run(thread_store, command_rx, event_tx)) {
                    let _ = failure_tx.send_blocking(UiEvent::Connection(ConnState::Offline(
                        format!("{error:#}"),
                    )));
                }
            })
            .expect("spawn connection thread");
        Backend {
            commands,
            events,
            store,
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn events(&self) -> async_channel::Receiver<UiEvent> {
        self.events.clone()
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }
}

fn full_history_config() -> HistorySyncConfig {
    let mut config = default_history_sync_config();
    config.full_sync_days_limit = Some(365);
    config.on_demand_ready = Some(true);
    config.complete_on_demand_ready = Some(true);
    config
}

async fn run(
    store: Arc<Store>,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<Command>,
    events: async_channel::Sender<UiEvent>,
) -> Result<()> {
    let _ = events.send(UiEvent::Connection(ConnState::Starting)).await;
    let session = SqliteStore::open(&crate::paths::session_db().to_string_lossy())
        .await
        .context("opening the session store")?;

    let handler_store = store.clone();
    let handler_events = events.clone();
    let bot = Bot::builder()
        .with_backend(session)
        // Same pairing shape as the Windows desktop client, so the phone sends the full
        // chat history (up to a year) instead of only recent messages.
        .with_device_props(
            DevicePropsOverride::new()
                .with_os("Windows")
                .with_platform_type(PlatformType::UWP)
                .with_require_full_sync(true)
                .with_history_sync_config(full_history_config()),
        )
        .on_event(move |event, client| {
            let store = handler_store.clone();
            let events = handler_events.clone();
            async move {
                if matches!(&*event, Event::Connected(_)) {
                    on_connected(&client, &events).await;
                    replay_account_state_once(client.clone(), store.clone());
                }
                for update in ingest::apply(&store, &event) {
                    let _ = events.send(update).await;
                }
                for chat in decode_poll_votes(&client, &store, &event).await {
                    let _ = events.send(UiEvent::ChatChanged { chat }).await;
                }
            }
        })
        .build()
        .await
        .context("building the client")?;

    let _ = events.send(UiEvent::Connection(ConnState::Connecting)).await;
    let handle = bot.spawn();
    let client = handle.client();

    // Profile pictures are fetched one at a time with a pause, so a full chat list does not
    // turn into a burst of requests the server would throttle.
    let (avatar_tx, mut avatar_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    {
        let client = client.clone();
        let events = events.clone();
        tokio::spawn(async move {
            while let Some(who) = avatar_rx.recv().await {
                let outcome = crate::avatars::fetch(&client, &who).await;
                if let Err(error) = &outcome
                    && format!("{error:#}").contains("429")
                {
                    tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                }
                let _ = events.send(UiEvent::AvatarReady { jid: who }).await;
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        });
    }

    while let Some(command) = commands.recv().await {
        if matches!(command, Command::Shutdown) {
            break;
        }
        if let Command::FetchAvatar { jid } = command {
            let _ = avatar_tx.send(jid);
            continue;
        }
        let client = client.clone();
        let store = store.clone();
        let events = events.clone();
        tokio::spawn(async move {
            let label = format!("{command:?}");
            if let Err(error) = execute(&client, &store, &events, command).await {
                let short: String = label.chars().take(40).collect();
                let _ = events
                    .send(UiEvent::Error(format!("{short}: {error:#}")))
                    .await;
            }
        });
    }
    let _ = handle.shutdown().await;
    Ok(())
}

async fn on_connected(client: &Arc<Client>, events: &async_channel::Sender<UiEvent>) {
    let _ = events
        .send(UiEvent::Identity {
            pn: client.pn().map(|j| j.to_non_ad_string()),
            lid: client.lid().map(|j| j.to_non_ad_string()),
            push_name: client.push_name(),
        })
        .await;
    if read_only() {
        let _ = client.presence().set_unavailable().await;
    }
}

/// Once per installation, replays the account's synced state (saved contact names, pins,
/// mutes, archive flags, stars) so it lands in our store, not only in the session store.
fn replay_account_state_once(client: Arc<Client>, store: Arc<Store>) {
    const DONE: &str = "account_state_replayed";
    if store.kv(DONE).ok().flatten().is_some() {
        return;
    }
    tokio::spawn(async move {
        use whatsapp_rust::{AppStateResyncMode, WAPatchName};
        let collections = [
            WAPatchName::CriticalBlock,
            WAPatchName::CriticalUnblockLow,
            WAPatchName::RegularHigh,
            WAPatchName::RegularLow,
            WAPatchName::Regular,
        ];
        if let Ok(report) = client
            .resync_app_state(collections, AppStateResyncMode::Snapshot)
            .await
            && report.all_synced()
        {
            let _ = store.set_kv(DONE, "1");
        }
    });
}

/// The addressing a protocol operation on a stored message needs: its chat, its original
/// author (for messages by others in groups), and whether it is ours.
pub(crate) fn target_parts(record: &wa::WebMessageInfo, chat: &str) -> Result<(Jid, Option<Jid>, bool)> {
    let key = record
        .key
        .as_option()
        .ok_or_else(|| anyhow!("message has no key"))?;
    let chat_jid = jid(key.remote_jid.as_deref().unwrap_or(chat))?;
    let sender = original_sender(record).map(|s| jid(&s)).transpose()?;
    Ok((chat_jid, sender, key.from_me.unwrap_or(false)))
}

pub(crate) fn jid(text: &str) -> Result<Jid> {
    Jid::from_str(text).map_err(|error| anyhow!("bad address {text}: {error}"))
}

pub(crate) async fn notify(events: &async_channel::Sender<UiEvent>, chat: &str) {
    let _ = events.send(UiEvent::ChatsChanged).await;
    let _ = events
        .send(UiEvent::ChatChanged {
            chat: chat.to_string(),
        })
        .await;
}

/// The original (un-canonicalised) author of a stored message, as the protocol needs it.
fn original_sender(record: &wa::WebMessageInfo) -> Option<String> {
    record
        .participant
        .clone()
        .or_else(|| record.key.as_option().and_then(|k| k.participant.clone()))
        .filter(|p| !p.is_empty())
}

/// Reply context quoting a stored message.
pub(crate) fn quote_context(record: &wa::WebMessageInfo, chat: &str) -> Option<wa::ContextInfo> {
    let key = record.key.as_option()?;
    let message = record.message.as_option()?;
    let mut context = wa::ContextInfo::default();
    context.stanza_id = key.id.clone();
    let from_me = key.from_me.unwrap_or(false);
    context.participant = if from_me {
        None
    } else {
        original_sender(record).or_else(|| {
            (ChatKind::of_jid(chat) == ChatKind::User).then(|| key.remote_jid.clone().unwrap_or_default())
        })
    };
    context.quoted_message = MessageField::from_box(message.prepare_for_quote());
    Some(context)
}

fn text_message(text: &str, context: Option<wa::ContextInfo>, mentions: &[String]) -> wa::Message {
    let mut message = wa::Message::default();
    if context.is_none() && mentions.is_empty() {
        message.conversation = Some(text.to_string());
        return message;
    }
    let mut extended = wa::message::ExtendedTextMessage::default();
    extended.text = Some(text.to_string());
    let mut context = context.unwrap_or_default();
    context.mentioned_jid = mentions.to_vec();
    extended.context_info = MessageField::some(context);
    message.extended_text_message = MessageField::some(extended);
    message
}

/// Stores our own outgoing message before it is sent, so it shows at once with a clock.
fn store_outgoing(store: &Store, chat: &str, id: &str, message: &wa::Message) -> Result<()> {
    let mut key = wa::MessageKey::default();
    key.remote_jid = Some(chat.to_string());
    key.from_me = Some(true);
    key.id = Some(id.to_string());
    let mut record = wa::WebMessageInfo::default();
    record.key = MessageField::some(key);
    record.message = MessageField::some(message.clone());
    record.message_timestamp = Some(now_secs() as u64);
    record.status = Some(wa::web_message_info::Status::PENDING);
    store.put_message(&record)?;
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Sends a prepared message with a local echo, and records the outcome.
pub(crate) async fn send_prepared(
    client: &Arc<Client>,
    store: &Arc<Store>,
    events: &async_channel::Sender<UiEvent>,
    chat: &str,
    message: wa::Message,
) -> Result<String> {
    let to = jid(chat)?;
    let id = client.generate_message_id();
    let id_text = id.as_str().to_string();
    store_outgoing(store, chat, &id_text, &message)?;
    store.batch(|tx| tx.set_unread(chat, 0))?;
    notify(events, chat).await;

    let options = SendOptions::default().with_message_id(id);
    let outcome = client
        .send(SendRequest::new(&to, message).with_options(options))
        .await;
    match outcome {
        Ok(_) => {
            store.batch(|tx| tx.set_status(chat, &id_text, Ack::Sent))?;
            notify(events, chat).await;
            Ok(id_text)
        }
        Err(error) => {
            store.batch(|tx| tx.force_status(chat, &id_text, Ack::Error))?;
            notify(events, chat).await;
            Err(anyhow!("send failed: {error}"))
        }
    }
}

async fn execute(
    client: &Arc<Client>,
    store: &Arc<Store>,
    events: &async_channel::Sender<UiEvent>,
    command: Command,
) -> Result<()> {
    match command {
        Command::SendText {
            chat,
            text,
            reply_to,
            mentions,
        } => {
            let context = match reply_to {
                Some(id) => store.record(&chat, &id)?.and_then(|r| quote_context(&r, &chat)),
                None => None,
            };
            let message = text_message(&text, context, &mentions);
            send_prepared(client, store, events, &chat, message).await?;
        }

        Command::React { chat, id, emoji } => {
            let record = store
                .record(&chat, &id)?
                .ok_or_else(|| anyhow!("message not found"))?;
            let key = record.key.as_option().ok_or_else(|| anyhow!("message has no key"))?;
            let from_me = key.from_me.unwrap_or(false);
            let chat_jid = jid(key.remote_jid.as_deref().unwrap_or(&chat))?;
            let sender = original_sender(&record).map(|s| jid(&s)).transpose()?;
            let target = MessageRef::new(&chat_jid, MessageId::new(&id)?, sender.as_ref(), from_me)?;
            client.send_reaction(&target, &emoji).await?;
            store.batch(|tx| tx.put_reaction(&chat, &id, "me", &emoji, now_secs()))?;
            notify(events, &chat).await;
        }

        Command::Revoke { chat, id } => {
            let record = store
                .record(&chat, &id)?
                .ok_or_else(|| anyhow!("message not found"))?;
            let key = record.key.as_option().ok_or_else(|| anyhow!("message has no key"))?;
            let from_me = key.from_me.unwrap_or(false);
            let chat_jid = jid(key.remote_jid.as_deref().unwrap_or(&chat))?;
            let sender = original_sender(&record).map(|s| jid(&s)).transpose()?;
            let target = MessageRef::new(&chat_jid, MessageId::new(&id)?, sender.as_ref(), from_me)?;
            client.revoke_message(&target).await?;
            store.batch(|tx| {
                tx.mark_revoked(&chat, &id)?;
                tx.refresh_last(&chat)
            })?;
            notify(events, &chat).await;
        }

        Command::Edit { chat, id, text } => {
            let to = jid(&chat)?;
            let mut replacement = wa::Message::default();
            replacement.conversation = Some(text);
            client
                .edit_message_raw(to, id.clone(), replacement.clone(), EditOptions::default())
                .await?;
            store.batch(|tx| tx.apply_edit(&chat, &id, &replacement))?;
            notify(events, &chat).await;
        }

        Command::MarkRead { chat } => {
            let unread = store.unread_count(&chat)?;
            if unread > 0 && !read_only() {
                let to = jid(&chat)?;
                let pending = store.unread_incoming(&chat, unread.clamp(1, 200) as usize)?;
                // Group receipts are per author, so batch the IDs by who wrote them.
                let mut by_sender: Vec<(Option<String>, Vec<String>)> = Vec::new();
                for (id, sender) in pending {
                    match by_sender.iter_mut().find(|(s, _)| *s == sender) {
                        Some((_, ids)) => ids.push(id),
                        None => by_sender.push((sender, vec![id])),
                    }
                }
                for (sender, ids) in by_sender {
                    let sender = sender.as_deref().map(jid).transpose()?;
                    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
                    client.mark_as_read(&to, sender.as_ref(), &ids).await?;
                }
            }
            if !read_only() {
                store.batch(|tx| tx.set_unread(&chat, 0))?;
                let _ = events.send(UiEvent::ChatsChanged).await;
            }
        }

        Command::MarkUnread { chat } => {
            client
                .chat_actions()
                .mark_chat_as_read(&jid(&chat)?, false, None)
                .await?;
            store.patch_chat(
                &chat,
                &ChatPatch {
                    marked_unread: Some(true),
                    ..Default::default()
                },
            )?;
            let _ = events.send(UiEvent::ChatsChanged).await;
        }

        Command::Typing { chat, activity } => {
            if !read_only() {
                let to = jid(&chat)?;
                let state = client.chatstate();
                match activity {
                    Activity::Typing => state.send_composing(&to).await?,
                    Activity::Recording => state.send_recording(&to).await?,
                    Activity::Idle => state.send_paused(&to).await?,
                }
            }
        }

        Command::WatchPresence { jid: who } => {
            if ChatKind::of_jid(&who) == ChatKind::User {
                client.presence().subscribe(&jid(&who)?).await?;
            }
        }

        Command::SetPresence { online } => {
            if online && !read_only() {
                client.presence().set_available().await?;
            } else {
                client.presence().set_unavailable().await?;
            }
        }

        Command::Archive { chat, archived } => {
            let to = jid(&chat)?;
            if archived {
                client.chat_actions().archive_chat(&to, None).await?;
            } else {
                client.chat_actions().unarchive_chat(&to, None).await?;
            }
            store.patch_chat(
                &chat,
                &ChatPatch {
                    archived: Some(archived),
                    ..Default::default()
                },
            )?;
            let _ = events.send(UiEvent::ChatsChanged).await;
        }

        Command::Pin { chat, pinned } => {
            let to = jid(&chat)?;
            if pinned {
                client.chat_actions().pin_chat(&to).await?;
            } else {
                client.chat_actions().unpin_chat(&to).await?;
            }
            store.patch_chat(
                &chat,
                &ChatPatch {
                    pinned: Some(if pinned { now_secs() } else { 0 }),
                    ..Default::default()
                },
            )?;
            let _ = events.send(UiEvent::ChatsChanged).await;
        }

        Command::Mute { chat, until } => {
            let to = jid(&chat)?;
            let stored = match until {
                Some(until) if until > 0 => {
                    client.chat_actions().mute_chat_until(&to, until * 1000).await?;
                    until
                }
                Some(_) => {
                    client.chat_actions().mute_chat(&to).await?;
                    -1
                }
                None => {
                    client.chat_actions().unmute_chat(&to).await?;
                    0
                }
            };
            store.patch_chat(
                &chat,
                &ChatPatch {
                    mute_until: Some(stored),
                    ..Default::default()
                },
            )?;
            let _ = events.send(UiEvent::ChatsChanged).await;
        }

        Command::DeleteChat { chat } => {
            client
                .chat_actions()
                .delete_chat(&jid(&chat)?, true, None)
                .await?;
            store.batch(|tx| tx.delete_chat(&chat))?;
            let _ = events.send(UiEvent::ChatsChanged).await;
        }

        Command::Logout => {
            let _ = client.logout().await;
            let _ = events.send(UiEvent::Connection(ConnState::LoggedOut)).await;
        }

        Command::Star { chat, id, starred } => {
            let record = store
                .record(&chat, &id)?
                .ok_or_else(|| anyhow!("message not found"))?;
            let (chat_jid, sender, from_me) = target_parts(&record, &chat)?;
            let target = MessageRef::new(&chat_jid, MessageId::new(&id)?, sender.as_ref(), from_me)?;
            if starred {
                client.chat_actions().star_message(&target).await?;
            } else {
                client.chat_actions().unstar_message(&target).await?;
            }
            store.batch(|tx| tx.set_starred(&chat, &id, starred))?;
            notify(events, &chat).await;
        }

        Command::DeleteForMe { chat, id } => {
            let record = store
                .record(&chat, &id)?
                .ok_or_else(|| anyhow!("message not found"))?;
            let (chat_jid, sender, from_me) = target_parts(&record, &chat)?;
            let target = MessageRef::new(&chat_jid, MessageId::new(&id)?, sender.as_ref(), from_me)?;
            let sent_at = record.message_timestamp.map(|ts| ts as i64);
            client
                .chat_actions()
                .delete_message_for_me(&target, true, sent_at)
                .await?;
            store.batch(|tx| tx.delete_message(&chat, &id))?;
            notify(events, &chat).await;
        }

        Command::Forward {
            from_chat,
            id,
            to_chats,
        } => {
            let record = store
                .record(&from_chat, &id)?
                .ok_or_else(|| anyhow!("message not found"))?;
            let message = record
                .message
                .as_option()
                .ok_or_else(|| anyhow!("message has no content"))?;
            for to in to_chats {
                let prepared = message.prepare_for_forward();
                send_prepared(client, store, events, &to, prepared).await?;
            }
        }

        command @ (Command::DownloadMedia { .. } | Command::SendFile { .. }) => {
            crate::media_ops::execute(client, store, events, command).await?;
        }

        Command::RefreshGroup { chat } => {
            let meta = client
                .groups()
                .fetch_metadata(&jid(&chat)?)
                .await
                .map_err(|error| anyhow!("could not read the group details: {error}"))?;
            store.batch(|tx| {
                tx.patch_chat(
                    &chat,
                    &ChatPatch {
                        name: meta.subject.clone(),
                        description: meta.description.clone(),
                        created_at: meta.creation_time.map(|t| t as i64),
                        ..Default::default()
                    },
                )?;
                tx.clear_group_members(&chat)?;
                for member in &meta.participants {
                    let address = member.jid.to_non_ad_string();
                    // Prefer the phone-number identity, and remember that the two are one person.
                    let person = match &member.phone_number {
                        Some(pn) => {
                            let pn = pn.to_non_ad_string();
                            if address.ends_with("@lid") {
                                tx.learn_lid(&address, &pn)?;
                            }
                            pn
                        }
                        None => address,
                    };
                    let rank = if member.is_super_admin() {
                        2
                    } else if member.is_admin() {
                        1
                    } else {
                        0
                    };
                    tx.put_group_member(&chat, &person, rank)?;
                }
                Ok(())
            })?;
            notify(events, &chat).await;
        }

        Command::LeaveGroup { chat } => {
            client
                .groups()
                .leave(jid(&chat)?)
                .await
                .map_err(|error| anyhow!("could not leave the group: {error}"))?;
            store.patch_chat(
                &chat,
                &ChatPatch {
                    read_only: Some(true),
                    ..Default::default()
                },
            )?;
            notify(events, &chat).await;
        }

        Command::VotePoll { chat, id, options } => {
            let record = store
                .record(&chat, &id)?
                .ok_or_else(|| anyhow!("poll not found"))?;
            let secret = crate::polls::secret(&record)
                .ok_or_else(|| anyhow!("this poll's key is not on this device, so it cannot be voted on here"))?;
            let (chat_jid, sender, from_me) = target_parts(&record, &chat)?;
            // Votes are encrypted to the poll's creator.
            let creator = if from_me {
                client.pn().ok_or_else(|| anyhow!("not logged in"))?
            } else {
                sender.unwrap_or_else(|| chat_jid.clone())
            };
            client
                .polls()
                .vote_raw(chat_jid, &id, &creator, &secret, &options)
                .await
                .map_err(|error| anyhow!("vote failed: {error}"))?;
            let hashes: Vec<String> = options.iter().map(|o| crate::polls::option_hash(o)).collect();
            store.batch(|tx| tx.put_poll_vote(&chat, &id, "me", &hashes, now_secs()))?;
            notify(events, &chat).await;
        }

        Command::CreatePoll {
            chat,
            question,
            options,
            multiple,
        } => {
            let created = client
                .polls()
                .create(jid(&chat)?, &question, &options, if multiple { 0 } else { 1 })
                .await
                .map_err(|error| anyhow!("could not create the poll: {error}"))?;
            // Keep our own copy, with the secret that later votes need.
            let result = created.send_result();
            let mut message = (*result.message).clone();
            let mut context = message.message_context_info.as_option().cloned().unwrap_or_default();
            if context.message_secret.is_none() {
                context.message_secret = Some(created.secret().as_bytes().to_vec().into());
            }
            message.message_context_info = MessageField::some(context);
            let id = result.message_id.as_str().to_string();
            store_outgoing(store, &chat, &id, &message)?;
            store.batch(|tx| tx.set_status(&chat, &id, Ack::Sent))?;
            notify(events, &chat).await;
        }

        Command::CreateGroup { subject, members } => {
            use whatsapp_rust::{GroupCreateOptions, GroupParticipantOptions};
            let mut options = GroupCreateOptions::new(subject.clone());
            for member in &members {
                options = options.with_participant(GroupParticipantOptions::from_phone(jid(member)?));
            }
            let created = client
                .groups()
                .create_group(options)
                .await
                .map_err(|error| anyhow!("could not create the group: {error}"))?;
            let group = created.metadata.id.to_non_ad_string();
            store.batch(|tx| {
                tx.patch_chat(
                    &group,
                    &ChatPatch {
                        name: Some(subject.clone()),
                        last_ts: Some(now_secs()),
                        created_at: Some(now_secs()),
                        ..Default::default()
                    },
                )?;
                for member in &created.metadata.participants {
                    let person = member
                        .phone_number
                        .as_ref()
                        .unwrap_or(&member.jid)
                        .to_non_ad_string();
                    tx.put_group_member(&group, &person, if member.is_admin() { 1 } else { 0 })?;
                }
                Ok(())
            })?;
            notify(events, &group).await;
        }

        Command::SetSubject { chat, subject } => {
            use whatsapp_rust::GroupSubject;
            let validated = GroupSubject::new(subject.clone()).map_err(|error| anyhow!("{error}"))?;
            client
                .groups()
                .set_subject(jid(&chat)?, validated)
                .await
                .map_err(|error| anyhow!("could not rename the group: {error}"))?;
            store.patch_chat(
                &chat,
                &ChatPatch {
                    name: Some(subject),
                    ..Default::default()
                },
            )?;
            notify(events, &chat).await;
        }

        Command::SetDescription { chat, description } => {
            use whatsapp_rust::{GroupDescription, PreviousDescription};
            let validated = match description.trim() {
                "" => None,
                text => Some(GroupDescription::new(text.to_string()).map_err(|error| anyhow!("{error}"))?),
            };
            client
                .groups()
                .set_description(jid(&chat)?, validated, PreviousDescription::Resolve)
                .await
                .map_err(|error| anyhow!("could not change the description: {error}"))?;
            store.patch_chat(
                &chat,
                &ChatPatch {
                    description: Some(description),
                    ..Default::default()
                },
            )?;
            notify(events, &chat).await;
        }

        Command::ChangeMembers { chat, members, change } => {
            use crate::events::MemberChange;
            let group = jid(&chat)?;
            let people: Vec<Jid> = members.iter().map(|m| jid(m)).collect::<Result<_>>()?;
            let groups = client.groups();
            let outcome = match change {
                MemberChange::Add => groups.add_participants(group, &people).await.map(|_| ()),
                MemberChange::Remove => groups.remove_participants(group, &people).await.map(|_| ()),
                MemberChange::Promote => groups.promote_participants(group, &people).await.map(|_| ()),
                MemberChange::Demote => groups.demote_participants(group, &people).await.map(|_| ()),
            };
            outcome.map_err(|error| anyhow!("could not change the members: {error}"))?;
            // Read the result back from the server rather than guessing it.
            Box::pin(execute(client, store, events, Command::RefreshGroup { chat })).await?;
        }

        Command::FetchAccountInfo => {
            // Read-only: who can see what, and who is blocked.
            let privacy = match client.fetch_privacy_settings().await {
                Ok(response) => response
                    .settings
                    .iter()
                    .map(|setting| (format!("{:?}", setting.category), format!("{:?}", setting.value)))
                    .collect(),
                Err(_) => Vec::new(),
            };
            let blocked = match client.blocking().get_blocklist().await {
                Ok(entries) => {
                    let mut people = Vec::with_capacity(entries.len());
                    for entry in entries {
                        people.push(store.canonical(&entry.jid.to_non_ad_string())?);
                    }
                    people
                }
                Err(_) => Vec::new(),
            };
            let _ = events.send(UiEvent::AccountInfo { privacy, blocked }).await;
        }

        Command::Block { jid: who, blocked } => {
            let person = jid(&who)?;
            let outcome = if blocked {
                client.blocking().block(&person).await
            } else {
                client.blocking().unblock(&person).await
            };
            outcome.map_err(|error| anyhow!("could not change the block: {error}"))?;
            Box::pin(execute(client, store, events, Command::FetchAccountInfo)).await?;
        }

        Command::LoadOlder { chat } => {
            // The phone answers with a history-sync blob, which arrives as an event and is
            // imported like any other.
            let oldest = store
                .oldest_message(&chat)?
                .ok_or_else(|| anyhow!("this chat has no message to load from"))?;
            client
                .fetch_message_history(&jid(&chat)?, &oldest.id, oldest.from_me, oldest.ts * 1000, 50)
                .await?;
        }

        Command::ClearChat { chat } => {
            client
                .chat_actions()
                .clear_chat_with_options(&jid(&chat)?, Default::default(), None)
                .await?;
            store.batch(|tx| tx.clear_chat(&chat))?;
            notify(events, &chat).await;
        }

        other => {
            return Err(anyhow!(
                "not implemented yet: {}",
                format!("{other:?}").split([' ', '{']).next().unwrap_or("command")
            ));
        }
    }
    let _ = bare_jid;
    Ok(())
}

/// Decodes poll votes that arrive live (they are encrypted to the poll's creator and
/// voters) and stores them. Returns the chats whose polls changed.
async fn decode_poll_votes(client: &Arc<Client>, store: &Arc<Store>, event: &Event) -> Vec<String> {
    use whatsapp_rust::wacore::poll::PollVoteCiphertext;
    let Event::Messages(batch) = event else {
        return Vec::new();
    };
    let mut touched = Vec::new();
    for inbound in batch.iter() {
        let (base, _, _) = crate::content::unwrap(&inbound.message);
        let Some(update) = base.poll_update_message.as_option() else {
            continue;
        };
        let (Some(key), Some(vote)) = (
            update.poll_creation_message_key.as_option(),
            update.vote.as_option(),
        ) else {
            continue;
        };
        let Some(poll_id) = key.id.as_deref() else {
            continue;
        };
        let Ok(chat) = store.canonical(&inbound.info.source.chat.to_non_ad_string()) else {
            continue;
        };
        let Ok(Some(record)) = store.record(&chat, poll_id) else {
            continue;
        };
        let Some(secret) = crate::polls::secret(&record) else {
            continue;
        };
        let Ok(secret_bytes) = <[u8; 32]>::try_from(secret.as_slice()) else {
            continue;
        };
        let Ok((chat_jid, sender, from_me)) = target_parts(&record, &chat) else {
            continue;
        };
        let creator = if from_me {
            match client.pn() {
                Some(pn) => pn,
                None => continue,
            }
        } else {
            sender.clone().unwrap_or_else(|| chat_jid.clone())
        };
        let Ok(message_id) = MessageId::new(poll_id) else {
            continue;
        };
        let Ok(target) = MessageRef::new(&chat_jid, message_id, sender.as_ref(), from_me) else {
            continue;
        };
        let secret = whatsapp_rust::MessageSecret::from_bytes(secret_bytes);
        let Ok(poll) = whatsapp_rust::PollRef::new(target, &creator, &secret) else {
            continue;
        };
        let voter_jid = inbound.info.source.sender.to_non_ad();
        let ciphertext = PollVoteCiphertext {
            enc_payload: vote.enc_payload.as_ref().map(|bytes| &bytes[..]).unwrap_or(&[]),
            enc_iv: vote.enc_iv.as_ref().map(|bytes| &bytes[..]).unwrap_or(&[]),
        };
        if let Ok(picked) = client.polls().decrypt_vote(ciphertext, &poll, &voter_jid).await {
            let voter = if inbound.info.source.is_from_me {
                "me".to_string()
            } else {
                voter_jid.to_string()
            };
            let hashes: Vec<String> = picked.iter().map(|hash| crate::polls::hex(hash)).collect();
            let at = inbound.info.timestamp.timestamp();
            if store
                .batch(|tx| tx.put_poll_vote(&chat, poll_id, &voter, &hashes, at))
                .is_ok()
            {
                touched.push(chat);
            }
        }
    }
    touched
}
