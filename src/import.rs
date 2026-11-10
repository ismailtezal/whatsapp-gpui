//! Loads history-sync data (the chats and messages the phone sends after linking)
//! into the store.

use anyhow::{Context, Result};
use whatsapp_rust::wacore::history_sync::{HistorySyncStream, MAX_DECOMPRESSED};

use crate::model::ContactRow;
use crate::store::{ChatPatch, Store, Tx};
use crate::wa;

#[derive(Debug, Default, Clone, Copy)]
pub struct ImportStats {
    pub conversations: usize,
    pub messages: usize,
    /// Records that are actions or key exchanges, not messages to show.
    pub non_display: usize,
    pub push_names: usize,
    pub lid_mappings: usize,
    pub status_messages: usize,
    pub call_logs: usize,
    pub skipped_conversations: usize,
}

impl ImportStats {
    pub fn add(&mut self, other: ImportStats) {
        self.conversations += other.conversations;
        self.messages += other.messages;
        self.non_display += other.non_display;
        self.push_names += other.push_names;
        self.lid_mappings += other.lid_mappings;
        self.status_messages += other.status_messages;
        self.call_logs += other.call_logs;
        self.skipped_conversations += other.skipped_conversations;
    }
}

/// Imports one history-sync blob (zlib-compressed, exactly as received).
///
/// Importing the same blob twice is harmless: every write is an upsert.
pub fn import_blob(store: &Store, compressed: &[u8]) -> Result<ImportStats> {
    // First pass: everything except conversations. Identity mappings come first so that
    // conversations are filed under their final JID straight away.
    let mut scan = HistorySyncStream::new(compressed, MAX_DECOMPRESSED);
    while scan
        .next_conversation_bytes()
        .context("scanning history sync")?
        .is_some()
    {}
    let rest = scan.remainder().context("decoding history sync")?;

    let mut stats = ImportStats::default();
    store.batch(|tx| {
        for mapping in &rest.phone_number_to_lid_mappings {
            if let (Some(pn), Some(lid)) = (mapping.pn_jid.as_deref(), mapping.lid_jid.as_deref()) {
                tx.learn_lid(lid, pn)?;
                stats.lid_mappings += 1;
            }
        }
        for push_name in &rest.pushnames {
            if let (Some(id), Some(name)) = (push_name.id.as_deref(), push_name.pushname.as_deref())
                && !name.is_empty()
                && name != "-"
            {
                tx.put_contact(&ContactRow {
                    jid: id.to_string(),
                    push_name: Some(name.to_string()),
                    ..Default::default()
                })?;
                stats.push_names += 1;
            }
        }
        for status in &rest.status_v3_messages {
            if tx.put_message(status)?.is_some() {
                stats.status_messages += 1;
            }
        }
        for call in &rest.call_log_records {
            tx.put_call(call)?;
        }
        stats.call_logs += rest.call_log_records.len();
        Ok(())
    })?;

    // Second pass: conversations, one transaction each to keep memory and lock time bounded.
    let mut stream = HistorySyncStream::new(compressed, MAX_DECOMPRESSED);
    let mut conversation = wa::Conversation::default();
    while stream
        .next_conversation_into(&mut conversation)
        .context("reading conversation")?
    {
        let (messages, non_display) = store.batch(|tx| import_conversation(tx, &conversation))?;
        stats.conversations += 1;
        stats.messages += messages;
        stats.non_display += non_display;
    }
    stats.skipped_conversations = stream.skipped_conversations();
    Ok(stats)
}

fn import_conversation(tx: &Tx<'_>, conv: &wa::Conversation) -> rusqlite::Result<(usize, usize)> {
    if conv.id.is_empty() {
        return Ok((0, 0));
    }
    if let (Some(lid), Some(pn)) = (conv.lid_jid.as_deref(), conv.pn_jid.as_deref()) {
        tx.learn_lid(lid, pn)?;
    }
    let name = conv
        .name
        .clone()
        .or_else(|| conv.display_name.clone())
        .filter(|n| !n.is_empty());
    let last_ts = conv
        .conversation_timestamp
        .or(conv.last_msg_timestamp)
        .map(|ts| ts as i64);
    tx.patch_chat(
        &conv.id,
        &ChatPatch {
            name,
            last_ts,
            unread: conv.unread_count.map(i64::from),
            unread_mentions: conv.unread_mention_count.map(i64::from),
            marked_unread: conv.marked_as_unread,
            archived: conv.archived,
            pinned: conv.pinned.map(i64::from),
            mute_until: conv.mute_end_time.map(|ts| ts as i64),
            ephemeral: conv.ephemeral_expiration.map(i64::from),
            read_only: conv.read_only,
            locked: conv.locked,
            is_parent: conv.is_parent_group,
            parent_group: conv.parent_group_id.clone().filter(|g| !g.is_empty()),
            description: conv.description.clone().filter(|d| !d.is_empty()),
            created_at: conv.created_at.map(|ts| ts as i64),
            created_by: conv.created_by.clone().filter(|c| !c.is_empty()),
        },
    )?;
    for member in &conv.participant {
        if !member.user_jid.is_empty() {
            let rank = member.rank.map(|r| r as i32).unwrap_or(0);
            tx.put_group_member(&conv.id, &member.user_jid, rank)?;
        }
    }
    let mut stored = 0;
    let mut non_display = 0;
    for entry in &conv.messages {
        let Some(message) = entry.message.as_option() else {
            continue;
        };
        if tx.put_message(message)?.is_some() {
            stored += 1;
        } else {
            non_display += 1;
        }
    }
    Ok((stored, non_display))
}

/// Imports every blob in the history folder, oldest first.
pub fn import_saved_history(store: &Store) -> Result<ImportStats> {
    let mut files: Vec<_> = std::fs::read_dir(crate::paths::history_dir())?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "zlib"))
        .collect();
    files.sort();
    let mut total = ImportStats::default();
    for file in files {
        let bytes = std::fs::read(&file)?;
        let stats =
            import_blob(store, &bytes).with_context(|| format!("importing {}", file.display()))?;
        total.add(stats);
    }
    Ok(total)
}
