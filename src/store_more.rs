//! Store queries for the surfaces beyond a single conversation: calls, starred messages,
//! status updates, contacts.

use rusqlite::{OptionalExtension, params};
use whatsapp_rust::buffa::Enumeration;

use crate::content::bare_jid;
use crate::model::{ChatRow, ContactRow, MsgRow};
use crate::store::{Result, Store, Tx, msg_row};
use crate::wa;

/// One entry of the calls list.
#[derive(Clone, Debug)]
pub struct CallRow {
    pub id: String,
    /// The other party, or the group for a group call.
    pub peer: String,
    pub ts: i64,
    pub duration: i64,
    pub video: bool,
    pub outgoing: bool,
    /// The protocol's call result code (0 connected, 1 rejected, 2 cancelled, 4 missed, …).
    pub outcome: i32,
    pub group: Option<String>,
}

impl CallRow {
    pub fn missed(&self) -> bool {
        !self.outgoing && matches!(self.outcome, 1 | 2 | 4 | 6 | 9)
    }
}

const MSG_COLUMNS: &str = "chat, id, from_me, sender, ts, kind, text, status, starred, quoted_id, \
                           edited, forwarded, media_path, proto, rowid";

impl Tx<'_> {
    /// Stores a call-log record (from history sync or a live update).
    pub fn put_call(&self, record: &wa::CallLogRecord) -> Result<()> {
        let Some(id) = record.call_id.as_deref().filter(|id| !id.is_empty()) else {
            return Ok(());
        };
        let outgoing = !record.is_incoming.unwrap_or(false);
        let group = record.group_jid.as_deref().filter(|g| !g.is_empty()).map(bare_jid);
        // The other party: the creator for an incoming call, else the first participant
        // who is not the creator.
        let creator = record.call_creator_jid.as_deref().filter(|c| !c.is_empty());
        let other = if outgoing {
            record
                .participants
                .iter()
                .filter_map(|p| p.user_jid.as_deref())
                .find(|p| Some(*p) != creator)
                .or(creator)
        } else {
            creator
        };
        let peer = match (&group, other) {
            (Some(group), _) => group.clone(),
            (None, Some(other)) => self.canonical(other)?,
            (None, None) => return Ok(()),
        };
        self.conn.execute(
            "INSERT INTO calls (id, peer, ts, duration, video, outgoing, outcome, group_jid)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (id) DO UPDATE SET
                peer = excluded.peer, ts = excluded.ts, duration = excluded.duration,
                video = excluded.video, outgoing = excluded.outgoing, outcome = excluded.outcome,
                group_jid = excluded.group_jid",
            params![
                id,
                peer,
                record.start_time.unwrap_or(0),
                record.duration.unwrap_or(0),
                record.is_video.unwrap_or(false),
                outgoing,
                record.call_result.map(|r| r.to_i32()).unwrap_or(0),
                group,
            ],
        )?;
        Ok(())
    }
}

impl Store {
    pub fn put_call(&self, record: &wa::CallLogRecord) -> Result<()> {
        self.batch(|tx| tx.put_call(record))
    }

    /// The calls list, newest first.
    pub fn calls(&self, limit: usize) -> Result<Vec<CallRow>> {
        self.read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id, peer, ts, duration, video, outgoing, outcome, group_jid
                 FROM calls ORDER BY ts DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map([limit as i64], |row| {
                Ok(CallRow {
                    id: row.get(0)?,
                    peer: row.get(1)?,
                    ts: row.get(2)?,
                    duration: row.get(3)?,
                    video: row.get::<_, i64>(4)? != 0,
                    outgoing: row.get::<_, i64>(5)? != 0,
                    outcome: row.get(6)?,
                    group: row.get(7)?,
                })
            })?;
            rows.collect()
        })
    }

    /// Starred messages across all chats, newest first.
    pub fn starred(&self, limit: usize) -> Result<Vec<MsgRow>> {
        self.read(|conn| {
            let sql = format!(
                "SELECT {MSG_COLUMNS} FROM messages WHERE starred = 1 ORDER BY ts DESC LIMIT ?1"
            );
            let mut stmt = conn.prepare(&sql)?;
            stmt.query_map([limit as i64], msg_row)?.collect()
        })
    }

    /// Status updates newer than `since`, oldest first.
    pub fn status_updates(&self, since: i64) -> Result<Vec<MsgRow>> {
        self.read(|conn| {
            let sql = format!(
                "SELECT {MSG_COLUMNS} FROM messages
                 WHERE chat = 'status@broadcast' AND ts >= ?1 ORDER BY ts"
            );
            let mut stmt = conn.prepare(&sql)?;
            stmt.query_map([since], msg_row)?.collect()
        })
    }

    /// Saved contacts (people with an address-book name), by name.
    pub fn contacts(&self) -> Result<Vec<ContactRow>> {
        self.read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT jid, lid, name, first_name, push_name, verified_name, username, about
                 FROM contacts
                 WHERE name IS NOT NULL AND jid LIKE '%@s.whatsapp.net'
                 ORDER BY name COLLATE NOCASE",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(ContactRow {
                    jid: row.get(0)?,
                    lid: row.get(1)?,
                    name: row.get(2)?,
                    first_name: row.get(3)?,
                    push_name: row.get(4)?,
                    verified_name: row.get(5)?,
                    username: row.get(6)?,
                    about: row.get(7)?,
                })
            })?;
            rows.collect()
        })
    }

    /// One chat by JID, whether archived or not.
    pub fn chat(&self, jid: &str) -> Result<Option<ChatRow>> {
        let mut all = self.chats(false)?;
        all.extend(self.chats(true)?);
        Ok(all.into_iter().find(|c| c.jid == jid))
    }

    /// Makes sure a chat row exists (for starting a conversation with a contact).
    pub fn ensure_chat(&self, jid: &str) -> Result<()> {
        self.patch_chat(jid, &Default::default())
    }

    /// Members of a group with their rank (0 member, 1 admin, 2 owner).
    pub fn group_members(&self, group: &str) -> Result<Vec<(String, i32)>> {
        self.read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT user, rank FROM group_members WHERE group_jid = ?1 ORDER BY rank DESC, user",
            )?;
            let rows = stmt.query_map([group], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect()
        })
    }

    /// Description and creation details of a group chat.
    pub fn chat_details(&self, jid: &str) -> Result<Option<(Option<String>, Option<i64>, i64)>> {
        self.read(|conn| {
            conn.query_row(
                "SELECT description, created_at, ephemeral FROM chats WHERE jid = ?1",
                [jid],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
        })
    }
}

impl Store {
    /// Paths of the newest downloaded photos of a chat, newest first.
    pub fn photo_paths(&self, chat: &str, limit: usize) -> Result<Vec<String>> {
        self.read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT media_path FROM messages
                 WHERE chat = ?1 AND kind = 1 AND media_path IS NOT NULL
                 ORDER BY ts DESC, rowid DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![chat, limit as i64], |row| row.get(0))?;
            rows.collect()
        })
    }

    /// How many messages of a chat are newer than the given one (its distance from the end).
    pub fn newer_than(&self, chat: &str, id: &str) -> Result<Option<i64>> {
        self.read(|conn| {
            let at: Option<(i64, i64)> = conn
                .query_row(
                    "SELECT ts, rowid FROM messages WHERE chat = ?1 AND id = ?2 LIMIT 1",
                    params![chat, id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((ts, rowid)) = at else {
                return Ok(None);
            };
            conn.query_row(
                "SELECT COUNT(*) FROM messages
                 WHERE chat = ?1 AND (ts > ?2 OR (ts = ?2 AND rowid > ?3))",
                params![chat, ts, rowid],
                |row| row.get(0),
            )
            .map(Some)
        })
    }
}
