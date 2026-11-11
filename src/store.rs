//! The local database of chats, messages and contacts.
//!
//! Every message is kept as its full protocol record (`WebMessageInfo`) next to a few
//! extracted columns for listing, sorting and search, so nothing is lost for message types
//! the interface learns to show later.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use whatsapp_rust::buffa::{Enumeration, Message as _};

use crate::content::{self, bare_jid};
use crate::model::{Ack, ChatKind, ChatRow, ContactRow, MsgKind, MsgRow, Preview, display_number};
use crate::wa;

pub type Result<T> = std::result::Result<T, rusqlite::Error>;

const SCHEMA_VERSION: i32 = 1;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS chats (
    jid             TEXT PRIMARY KEY,
    kind            INTEGER NOT NULL DEFAULT 0,
    name            TEXT,
    last_ts         INTEGER NOT NULL DEFAULT 0,
    last_rowid      INTEGER,
    unread          INTEGER NOT NULL DEFAULT 0,
    unread_mentions INTEGER NOT NULL DEFAULT 0,
    marked_unread   INTEGER NOT NULL DEFAULT 0,
    archived        INTEGER NOT NULL DEFAULT 0,
    pinned          INTEGER NOT NULL DEFAULT 0,
    mute_until      INTEGER NOT NULL DEFAULT 0,
    ephemeral       INTEGER NOT NULL DEFAULT 0,
    read_only       INTEGER NOT NULL DEFAULT 0,
    locked          INTEGER NOT NULL DEFAULT 0,
    is_parent       INTEGER NOT NULL DEFAULT 0,
    parent_group    TEXT,
    description     TEXT,
    created_at      INTEGER,
    created_by      TEXT,
    draft           TEXT,
    hidden          INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS messages (
    chat        TEXT NOT NULL,
    id          TEXT NOT NULL,
    from_me     INTEGER NOT NULL,
    sender      TEXT,
    ts          INTEGER NOT NULL,
    kind        INTEGER NOT NULL,
    text        TEXT,
    status      INTEGER NOT NULL DEFAULT 1,
    starred     INTEGER NOT NULL DEFAULT 0,
    quoted_id   TEXT,
    edited      INTEGER NOT NULL DEFAULT 0,
    forwarded   INTEGER NOT NULL DEFAULT 0,
    media_path  TEXT,
    proto       BLOB NOT NULL,
    UNIQUE (chat, id, from_me)
);
CREATE INDEX IF NOT EXISTS messages_by_chat ON messages (chat, ts);
CREATE INDEX IF NOT EXISTS messages_starred ON messages (starred) WHERE starred = 1;

CREATE TABLE IF NOT EXISTS contacts (
    jid           TEXT PRIMARY KEY,
    lid           TEXT,
    name          TEXT,
    first_name    TEXT,
    push_name     TEXT,
    verified_name TEXT,
    username      TEXT,
    about         TEXT,
    avatar_id     TEXT,
    avatar_path   TEXT
);

CREATE TABLE IF NOT EXISTS lid_map (
    lid TEXT PRIMARY KEY,
    pn  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS lid_map_by_pn ON lid_map (pn);

CREATE TABLE IF NOT EXISTS reactions (
    chat    TEXT NOT NULL,
    msg_id  TEXT NOT NULL,
    sender  TEXT NOT NULL,
    emoji   TEXT NOT NULL,
    ts      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (chat, msg_id, sender)
);

CREATE TABLE IF NOT EXISTS receipts (
    chat      TEXT NOT NULL,
    msg_id    TEXT NOT NULL,
    user      TEXT NOT NULL,
    delivered INTEGER,
    read      INTEGER,
    played    INTEGER,
    PRIMARY KEY (chat, msg_id, user)
);

CREATE TABLE IF NOT EXISTS group_members (
    group_jid TEXT NOT NULL,
    user      TEXT NOT NULL,
    rank      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (group_jid, user)
);

CREATE TABLE IF NOT EXISTS calls (
    id        TEXT PRIMARY KEY,
    peer      TEXT NOT NULL,
    ts        INTEGER NOT NULL,
    duration  INTEGER NOT NULL DEFAULT 0,
    video     INTEGER NOT NULL DEFAULT 0,
    outgoing  INTEGER NOT NULL DEFAULT 0,
    outcome   INTEGER NOT NULL DEFAULT 0,
    group_jid TEXT
);

CREATE TABLE IF NOT EXISTS poll_votes (
    chat    TEXT NOT NULL,
    poll_id TEXT NOT NULL,
    voter   TEXT NOT NULL,
    options TEXT NOT NULL DEFAULT '',
    ts      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (chat, poll_id, voter)
);

CREATE TABLE IF NOT EXISTS pins (
    chat   TEXT NOT NULL,
    msg_id TEXT NOT NULL,
    ts     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (chat, msg_id)
);

CREATE TABLE IF NOT EXISTS kv (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// Fields of a chat that an import or an update may set. `None` leaves a column as it is.
#[derive(Default, Debug, Clone)]
pub struct ChatPatch {
    pub name: Option<String>,
    pub last_ts: Option<i64>,
    pub unread: Option<i64>,
    pub unread_mentions: Option<i64>,
    pub marked_unread: Option<bool>,
    pub archived: Option<bool>,
    pub pinned: Option<i64>,
    pub mute_until: Option<i64>,
    pub ephemeral: Option<i64>,
    pub read_only: Option<bool>,
    pub locked: Option<bool>,
    pub is_parent: Option<bool>,
    pub parent_group: Option<String>,
    pub description: Option<String>,
    pub created_at: Option<i64>,
    pub created_by: Option<String>,
}

pub struct Store {
    conn: Mutex<Connection>,
    has_fts: bool,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        // Full-text search is optional: without it, search falls back to substring matching.
        let has_fts = conn
            .execute_batch(
                r#"
                CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                    text, content='messages', content_rowid='rowid',
                    tokenize='unicode61 remove_diacritics 2'
                );
                CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages BEGIN
                    INSERT INTO messages_fts(rowid, text) VALUES (new.rowid, new.text);
                END;
                CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages BEGIN
                    INSERT INTO messages_fts(messages_fts, rowid, text)
                    VALUES ('delete', old.rowid, old.text);
                END;
                CREATE TRIGGER IF NOT EXISTS messages_fts_update AFTER UPDATE OF text ON messages BEGIN
                    INSERT INTO messages_fts(messages_fts, rowid, text)
                    VALUES ('delete', old.rowid, old.text);
                    INSERT INTO messages_fts(rowid, text) VALUES (new.rowid, new.text);
                END;
                "#,
            )
            .is_ok();
        Ok(Store {
            conn: Mutex::new(conn),
            has_fts,
        })
    }

    /// Reads a little from the main tables. Fails if SQLite cannot follow them, which is
    /// how damage to the file shows itself.
    pub fn probe(&self) -> Result<()> {
        let conn = self.lock();
        conn.query_row("SELECT COUNT(*) FROM chats", [], |row| row.get::<_, i64>(0))?;
        conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get::<_, i64>(0))?;
        conn.query_row("SELECT MAX(rowid) FROM messages", [], |row| row.get::<_, Option<i64>>(0))?;
        Ok(())
    }

    pub fn open_default() -> Result<Self> {
        Self::open(&crate::paths::chat_db())
    }

    pub fn has_full_text_search(&self) -> bool {
        self.has_fts
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs several writes as one transaction (one disk sync instead of one per write).
    pub fn batch<T>(&self, work: impl FnOnce(&Tx<'_>) -> Result<T>) -> Result<T> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let out = work(&Tx { conn: &tx })?;
        tx.commit()?;
        Ok(out)
    }

    /// Runs a read against the connection.
    pub(crate) fn read<T>(&self, work: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.lock();
        work(&conn)
    }

    fn with<T>(&self, work: impl FnOnce(&Tx<'_>) -> Result<T>) -> Result<T> {
        let conn = self.lock();
        work(&Tx { conn: &conn })
    }

    // Single-operation conveniences; each mirrors a `Tx` method.

    pub fn put_message(&self, wmi: &wa::WebMessageInfo) -> Result<Option<i64>> {
        self.with(|tx| tx.put_message(wmi))
    }

    pub fn patch_chat(&self, jid: &str, patch: &ChatPatch) -> Result<()> {
        self.with(|tx| tx.patch_chat(jid, patch))
    }

    pub fn put_contact(&self, contact: &ContactRow) -> Result<()> {
        self.with(|tx| tx.put_contact(contact))
    }

    pub fn learn_lid(&self, lid: &str, pn: &str) -> Result<()> {
        self.with(|tx| tx.learn_lid(lid, pn))
    }

    pub fn canonical(&self, jid: &str) -> Result<String> {
        self.with(|tx| tx.canonical(jid))
    }

    pub fn set_kv(&self, key: &str, value: &str) -> Result<()> {
        self.with(|tx| {
            tx.conn.execute(
                "INSERT INTO kv (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    pub fn kv(&self, key: &str) -> Result<Option<String>> {
        self.with(|tx| {
            tx.conn
                .query_row("SELECT value FROM kv WHERE key = ?1", [key], |row| row.get(0))
                .optional()
        })
    }

    /// The chat list: pinned first, then newest first.
    pub fn chats(&self, archived: bool) -> Result<Vec<ChatRow>> {
        self.with(|tx| {
            let mut stmt = tx.conn.prepare_cached(
                r#"
                SELECT c.jid, c.kind, c.name, c.last_ts, c.unread, c.marked_unread, c.archived,
                       c.pinned, c.mute_until, c.draft,
                       k.name, k.push_name, k.verified_name,
                       m.kind, m.text, m.from_me, m.status, m.sender,
                       s.name, s.push_name
                FROM chats c
                LEFT JOIN contacts k ON k.jid = c.jid
                LEFT JOIN messages m ON m.rowid = c.last_rowid
                LEFT JOIN contacts s ON s.jid = m.sender
                WHERE c.archived = ?1 AND c.hidden = 0 AND c.kind != 4
                ORDER BY (c.pinned > 0) DESC, c.pinned DESC, c.last_ts DESC
                "#,
            )?;
            let rows = stmt.query_map([archived as i64], |row| {
                let jid: String = row.get(0)?;
                let kind = ChatKind::from_i32(row.get(1)?);
                let chat_name: Option<String> = row.get(2)?;
                let contact_name: Option<String> = row.get(10)?;
                let push_name: Option<String> = row.get(11)?;
                let verified_name: Option<String> = row.get(12)?;
                let title = pick_title(kind, &jid, contact_name, chat_name, verified_name, push_name);
                let preview = match row.get::<_, Option<i32>>(13)? {
                    Some(msg_kind) => {
                        let sender: Option<String> = row.get(17)?;
                        let sender_name: Option<String> = row
                            .get::<_, Option<String>>(18)?
                            .or(row.get::<_, Option<String>>(19)?)
                            .or_else(|| sender.as_deref().map(display_number));
                        Some(Preview {
                            kind: MsgKind::from_i32(msg_kind),
                            text: row.get(14)?,
                            from_me: row.get::<_, i64>(15)? != 0,
                            sender_name,
                            ack: Ack::from_i32(row.get(16)?),
                        })
                    }
                    None => None,
                };
                Ok(ChatRow {
                    jid,
                    kind,
                    title,
                    last_ts: row.get(3)?,
                    unread: row.get(4)?,
                    marked_unread: row.get::<_, i64>(5)? != 0,
                    archived: row.get::<_, i64>(6)? != 0,
                    pinned: row.get(7)?,
                    mute_until: row.get(8)?,
                    preview,
                    draft: row.get(9)?,
                })
            })?;
            rows.collect()
        })
    }

    /// A page of messages, oldest first, ending just before `before` (a `(ts, rowid)` cursor).
    pub fn messages(&self, chat: &str, before: Option<(i64, i64)>, limit: usize) -> Result<Vec<MsgRow>> {
        self.with(|tx| {
            let (ts, rowid) = before.unwrap_or((i64::MAX, i64::MAX));
            let mut stmt = tx.conn.prepare_cached(
                r#"
                SELECT chat, id, from_me, sender, ts, kind, text, status, starred, quoted_id,
                       edited, forwarded, media_path, proto, rowid
                FROM messages
                WHERE chat = ?1 AND (ts < ?2 OR (ts = ?2 AND rowid < ?3))
                ORDER BY ts DESC, rowid DESC
                LIMIT ?4
                "#,
            )?;
            let mut rows: Vec<MsgRow> = stmt
                .query_map(params![chat, ts, rowid, limit as i64], msg_row)?
                .collect::<Result<_>>()?;
            rows.reverse();
            Ok(rows)
        })
    }

    pub fn message(&self, chat: &str, id: &str) -> Result<Option<MsgRow>> {
        self.with(|tx| {
            tx.conn
                .query_row(
                    r#"
                    SELECT chat, id, from_me, sender, ts, kind, text, status, starred, quoted_id,
                           edited, forwarded, media_path, proto, rowid
                    FROM messages WHERE chat = ?1 AND id = ?2
                    ORDER BY from_me DESC LIMIT 1
                    "#,
                    params![chat, id],
                    msg_row,
                )
                .optional()
        })
    }

    pub fn message_count(&self, chat: Option<&str>) -> Result<i64> {
        self.with(|tx| match chat {
            Some(chat) => tx.conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE chat = ?1",
                [chat],
                |row| row.get(0),
            ),
            None => tx
                .conn
                .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0)),
        })
    }

    /// Reactions on a set of messages of one chat: `(msg_id, sender, emoji)`.
    pub fn reactions(&self, chat: &str, ids: &[String]) -> Result<Vec<(String, String, String)>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|tx| {
            let marks = vec!["?"; ids.len()].join(",");
            let sql = format!(
                "SELECT msg_id, sender, emoji FROM reactions
                 WHERE chat = ? AND emoji != '' AND msg_id IN ({marks}) ORDER BY ts"
            );
            let mut stmt = tx.conn.prepare(&sql)?;
            let args = std::iter::once(chat.to_string()).chain(ids.iter().cloned());
            let rows = stmt.query_map(rusqlite::params_from_iter(args), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;
            rows.collect()
        })
    }

    pub fn contact(&self, jid: &str) -> Result<Option<ContactRow>> {
        self.with(|tx| {
            tx.conn
                .query_row(
                    "SELECT jid, lid, name, first_name, push_name, verified_name, username, about
                     FROM contacts WHERE jid = ?1",
                    [jid],
                    |row| {
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
                    },
                )
                .optional()
        })
    }

    /// The best name for a person: saved name, verified business name, push name, number.
    pub fn display_name(&self, jid: &str) -> Result<String> {
        let contact = self.contact(jid)?;
        Ok(contact
            .and_then(|c| c.name.or(c.verified_name).or(c.push_name))
            .unwrap_or_else(|| display_number(jid)))
    }

    /// Finds a chat by exact JID, or by a case-insensitive title match.
    pub fn find_chats(&self, needle: &str) -> Result<Vec<ChatRow>> {
        let needle = needle.to_lowercase();
        let mut all = self.chats(false)?;
        all.extend(self.chats(true)?);
        Ok(all
            .into_iter()
            .filter(|c| c.jid == needle || c.title.to_lowercase().contains(&needle))
            .collect())
    }

    /// Full-text search over message text, newest first.
    pub fn search_messages(&self, query: &str, chat: Option<&str>, limit: usize) -> Result<Vec<MsgRow>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|tx| {
            let columns = "m.chat, m.id, m.from_me, m.sender, m.ts, m.kind, m.text, m.status, \
                           m.starred, m.quoted_id, m.edited, m.forwarded, m.media_path, m.proto, m.rowid";
            if self.has_fts {
                // Each word becomes a quoted prefix term, so punctuation cannot break the query.
                let terms: Vec<String> = query
                    .split_whitespace()
                    .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
                    .collect();
                let sql = format!(
                    "SELECT {columns} FROM messages_fts f JOIN messages m ON m.rowid = f.rowid
                     WHERE messages_fts MATCH ?1 AND (?2 IS NULL OR m.chat = ?2)
                     ORDER BY m.ts DESC LIMIT ?3"
                );
                let mut stmt = tx.conn.prepare(&sql)?;
                stmt.query_map(params![terms.join(" "), chat, limit as i64], msg_row)?
                    .collect()
            } else {
                let sql = format!(
                    "SELECT {columns} FROM messages m
                     WHERE m.text LIKE ?1 ESCAPE '\\' AND (?2 IS NULL OR m.chat = ?2)
                     ORDER BY m.ts DESC LIMIT ?3"
                );
                let pattern = format!(
                    "%{}%",
                    query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
                );
                let mut stmt = tx.conn.prepare(&sql)?;
                stmt.query_map(params![pattern, chat, limit as i64], msg_row)?
                    .collect()
            }
        })
    }
}

fn pick_title(
    kind: ChatKind,
    jid: &str,
    contact_name: Option<String>,
    chat_name: Option<String>,
    verified_name: Option<String>,
    push_name: Option<String>,
) -> String {
    let non_empty = |name: Option<String>| name.filter(|n| !n.trim().is_empty());
    let named = match kind {
        ChatKind::User => non_empty(contact_name)
            .or(non_empty(verified_name))
            .or(non_empty(chat_name))
            .or(non_empty(push_name)),
        _ => non_empty(chat_name).or(non_empty(contact_name)),
    };
    named.unwrap_or_else(|| match kind {
        ChatKind::User => display_number(jid),
        ChatKind::Group | ChatKind::Community => "Group".to_string(),
        ChatKind::Newsletter => "Channel".to_string(),
        ChatKind::Broadcast => "Broadcast list".to_string(),
        ChatKind::Status => "Status".to_string(),
    })
}

pub(crate) fn msg_row(row: &rusqlite::Row<'_>) -> Result<MsgRow> {
    Ok(MsgRow {
        chat: row.get(0)?,
        id: row.get(1)?,
        from_me: row.get::<_, i64>(2)? != 0,
        sender: row.get(3)?,
        ts: row.get(4)?,
        kind: MsgKind::from_i32(row.get(5)?),
        text: row.get(6)?,
        ack: Ack::from_i32(row.get(7)?),
        starred: row.get::<_, i64>(8)? != 0,
        quoted_id: row.get(9)?,
        edited: row.get::<_, i64>(10)? != 0,
        forwarded: row.get::<_, i64>(11)? != 0,
        media_path: row.get(12)?,
        proto: row.get(13)?,
        rowid: row.get(14)?,
    })
}

/// Write operations, usable alone or inside [`Store::batch`].
pub struct Tx<'a> {
    pub(crate) conn: &'a Connection,
}

impl Tx<'_> {
    /// The JID a chat or person is filed under: device part removed, and a LID replaced by
    /// its phone-number JID once the two are known to be the same person.
    pub fn canonical(&self, jid: &str) -> Result<String> {
        let bare = bare_jid(jid);
        if bare.ends_with("@lid") {
            let pn: Option<String> = self
                .conn
                .prepare_cached("SELECT pn FROM lid_map WHERE lid = ?1")?
                .query_row([&bare], |row| row.get(0))
                .optional()?;
            if let Some(pn) = pn {
                return Ok(pn);
            }
        }
        Ok(bare)
    }

    /// Records that a LID and a phone-number JID are the same person, and folds anything
    /// already filed under the LID into the phone-number JID.
    pub fn learn_lid(&self, lid: &str, pn: &str) -> Result<()> {
        let lid = bare_jid(lid);
        let pn = bare_jid(pn);
        if !lid.ends_with("@lid") || pn.ends_with("@lid") || pn.is_empty() {
            return Ok(());
        }
        let changed = self.conn.execute(
            "INSERT INTO lid_map (lid, pn) VALUES (?1, ?2)
             ON CONFLICT (lid) DO UPDATE SET pn = excluded.pn WHERE pn != excluded.pn",
            params![lid, pn],
        )?;
        if changed == 0 {
            return Ok(());
        }
        self.conn.execute(
            "UPDATE contacts SET lid = ?1 WHERE jid = ?2 AND (lid IS NULL OR lid != ?1)",
            params![lid, pn],
        )?;
        // Messages and senders filed under the LID move to the phone-number JID.
        self.conn.execute(
            "UPDATE OR IGNORE messages SET chat = ?2 WHERE chat = ?1",
            params![lid, pn],
        )?;
        self.conn.execute("DELETE FROM messages WHERE chat = ?1", [&lid])?;
        self.conn.execute(
            "UPDATE messages SET sender = ?2 WHERE sender = ?1",
            params![lid, pn],
        )?;
        self.conn.execute(
            "UPDATE OR IGNORE reactions SET chat = ?2 WHERE chat = ?1",
            params![lid, pn],
        )?;
        self.conn.execute(
            "UPDATE OR IGNORE reactions SET sender = ?2 WHERE sender = ?1",
            params![lid, pn],
        )?;
        self.conn.execute(
            "UPDATE OR IGNORE group_members SET user = ?2 WHERE user = ?1",
            params![lid, pn],
        )?;
        let lid_chat: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT last_ts, unread FROM chats WHERE jid = ?1",
                [&lid],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((last_ts, unread)) = lid_chat {
            self.ensure_chat(&pn)?;
            self.conn.execute(
                "UPDATE chats SET unread = unread + ?2 WHERE jid = ?1",
                params![pn, unread],
            )?;
            self.conn.execute("DELETE FROM chats WHERE jid = ?1", [&lid])?;
            self.refresh_last(&pn)?;
            let _ = last_ts;
        }
        // A contact row keyed by the LID folds into the phone-number row.
        let lid_contact: Option<(Option<String>, Option<String>)> = self
            .conn
            .query_row(
                "SELECT name, push_name FROM contacts WHERE jid = ?1",
                [&lid],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((name, push_name)) = lid_contact {
            self.put_contact(&ContactRow {
                jid: pn.clone(),
                lid: Some(lid.clone()),
                name,
                push_name,
                ..Default::default()
            })?;
            self.conn.execute("DELETE FROM contacts WHERE jid = ?1", [&lid])?;
        }
        Ok(())
    }

    fn ensure_chat(&self, jid: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO chats (jid, kind) VALUES (?1, ?2)",
            params![jid, ChatKind::of_jid(jid) as i32],
        )?;
        Ok(())
    }

    /// Points the chat at its newest message again (after a merge or a delete).
    pub fn refresh_last(&self, jid: &str) -> Result<()> {
        self.conn.execute(
            r#"
            UPDATE chats SET
                last_rowid = (SELECT rowid FROM messages WHERE chat = ?1
                              ORDER BY ts DESC, rowid DESC LIMIT 1),
                last_ts = MAX(last_ts, COALESCE(
                              (SELECT MAX(ts) FROM messages WHERE chat = ?1), 0))
            WHERE jid = ?1
            "#,
            [jid],
        )?;
        Ok(())
    }

    pub fn patch_chat(&self, jid: &str, patch: &ChatPatch) -> Result<()> {
        let jid = self.canonical(jid)?;
        self.ensure_chat(&jid)?;
        self.conn.execute(
            r#"
            UPDATE chats SET
                name            = COALESCE(?2, name),
                last_ts         = MAX(last_ts, COALESCE(?3, 0)),
                unread          = COALESCE(?4, unread),
                unread_mentions = COALESCE(?5, unread_mentions),
                marked_unread   = COALESCE(?6, marked_unread),
                archived        = COALESCE(?7, archived),
                pinned          = COALESCE(?8, pinned),
                mute_until      = COALESCE(?9, mute_until),
                ephemeral       = COALESCE(?10, ephemeral),
                read_only       = COALESCE(?11, read_only),
                locked          = COALESCE(?12, locked),
                is_parent       = COALESCE(?13, is_parent),
                parent_group    = COALESCE(?14, parent_group),
                description     = COALESCE(?15, description),
                created_at      = COALESCE(?16, created_at),
                created_by      = COALESCE(?17, created_by)
            WHERE jid = ?1
            "#,
            params![
                jid,
                patch.name.as_deref().filter(|n| !n.is_empty()),
                patch.last_ts,
                patch.unread,
                patch.unread_mentions,
                patch.marked_unread,
                patch.archived,
                patch.pinned,
                patch.mute_until,
                patch.ephemeral,
                patch.read_only,
                patch.locked,
                patch.is_parent,
                patch.parent_group,
                patch.description,
                patch.created_at,
                patch.created_by,
            ],
        )?;
        if patch.is_parent == Some(true) {
            self.conn.execute(
                "UPDATE chats SET kind = ?2 WHERE jid = ?1",
                params![jid, ChatKind::Community as i32],
            )?;
        }
        Ok(())
    }

    /// Adds what is known about a person; fields left `None` keep their stored value.
    pub fn put_contact(&self, contact: &ContactRow) -> Result<()> {
        let jid = self.canonical(&contact.jid)?;
        let clean = |value: &Option<String>| value.clone().filter(|v| !v.trim().is_empty());
        self.conn.execute(
            r#"
            INSERT INTO contacts (jid, lid, name, first_name, push_name, verified_name, username, about)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT (jid) DO UPDATE SET
                lid           = COALESCE(excluded.lid, lid),
                name          = COALESCE(excluded.name, name),
                first_name    = COALESCE(excluded.first_name, first_name),
                push_name     = COALESCE(excluded.push_name, push_name),
                verified_name = COALESCE(excluded.verified_name, verified_name),
                username      = COALESCE(excluded.username, username),
                about         = COALESCE(excluded.about, about)
            "#,
            params![
                jid,
                clean(&contact.lid),
                clean(&contact.name),
                clean(&contact.first_name),
                clean(&contact.push_name),
                clean(&contact.verified_name),
                clean(&contact.username),
                clean(&contact.about),
            ],
        )?;
        Ok(())
    }

    pub fn put_group_member(&self, group: &str, user: &str, rank: i32) -> Result<()> {
        let user = self.canonical(user)?;
        self.conn.execute(
            "INSERT INTO group_members (group_jid, user, rank) VALUES (?1, ?2, ?3)
             ON CONFLICT (group_jid, user) DO UPDATE SET rank = excluded.rank",
            params![bare_jid(group), user, rank],
        )?;
        Ok(())
    }

    pub fn put_reaction(&self, chat: &str, msg_id: &str, sender: &str, emoji: &str, ts: i64) -> Result<()> {
        let chat = self.canonical(chat)?;
        let sender = self.canonical(sender)?;
        self.conn.execute(
            "INSERT INTO reactions (chat, msg_id, sender, emoji, ts) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (chat, msg_id, sender) DO UPDATE
             SET emoji = excluded.emoji, ts = excluded.ts WHERE excluded.ts >= ts",
            params![chat, msg_id, sender, emoji, ts],
        )?;
        Ok(())
    }

    pub fn put_receipt(
        &self,
        chat: &str,
        msg_id: &str,
        user: &str,
        delivered: Option<i64>,
        read: Option<i64>,
        played: Option<i64>,
    ) -> Result<()> {
        let chat = self.canonical(chat)?;
        let user = self.canonical(user)?;
        self.conn.execute(
            "INSERT INTO receipts (chat, msg_id, user, delivered, read, played)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (chat, msg_id, user) DO UPDATE SET
                delivered = COALESCE(excluded.delivered, delivered),
                read      = COALESCE(excluded.read, read),
                played    = COALESCE(excluded.played, played)",
            params![chat, msg_id, user, delivered, read, played],
        )?;
        Ok(())
    }

    /// Stores a message record. Returns its row id, or `None` when the record is not a
    /// displayable message (an action or key exchange, handled elsewhere).
    pub fn put_message(&self, wmi: &wa::WebMessageInfo) -> Result<Option<i64>> {
        let Some(key) = wmi.key.as_option() else {
            return Ok(None);
        };
        let (Some(remote), Some(id)) = (key.remote_jid.as_deref(), key.id.as_deref()) else {
            return Ok(None);
        };
        let chat = self.canonical(remote)?;
        let from_me = key.from_me.unwrap_or(false);
        let kind_of_chat = ChatKind::of_jid(&chat);

        let digest = wmi.message.as_option().and_then(content::digest);
        let is_stub = wmi.message_stub_type.is_some();
        let (kind, text, quoted_id, edited, forwarded) = match &digest {
            Some(d) => (d.kind, d.text.clone(), d.quoted_id.clone(), d.edited, d.forwarded),
            None if is_stub => (MsgKind::System, None, None, false, false),
            None => return Ok(None),
        };

        let participant = wmi
            .participant
            .as_deref()
            .or(key.participant.as_deref())
            .filter(|p| !p.is_empty());
        let sender = match participant {
            Some(participant) => Some(self.canonical(participant)?),
            None if !from_me && kind_of_chat == ChatKind::User => Some(chat.clone()),
            None => None,
        };
        let ts = wmi.message_timestamp.unwrap_or(0) as i64;
        let status = wmi.status.map(|s| s.to_i32()).unwrap_or(if from_me { 2 } else { 3 });

        self.ensure_chat(&chat)?;
        self.conn
            .prepare_cached(
                r#"
                INSERT INTO messages
                    (chat, id, from_me, sender, ts, kind, text, status, starred, quoted_id,
                     edited, forwarded, proto)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                ON CONFLICT (chat, id, from_me) DO UPDATE SET
                    sender    = COALESCE(excluded.sender, sender),
                    ts        = CASE WHEN excluded.ts > 0 THEN excluded.ts ELSE ts END,
                    kind      = excluded.kind,
                    text      = excluded.text,
                    status    = MAX(status, excluded.status),
                    starred   = MAX(starred, excluded.starred),
                    quoted_id = excluded.quoted_id,
                    edited    = MAX(edited, excluded.edited),
                    forwarded = excluded.forwarded,
                    proto     = excluded.proto
                "#,
            )?
            .execute(params![
                chat,
                id,
                from_me,
                sender,
                ts,
                kind as i32,
                text,
                status,
                wmi.starred.unwrap_or(false),
                quoted_id,
                edited,
                forwarded,
                wmi.encode_to_vec(),
            ])?;
        let rowid: i64 = self
            .conn
            .prepare_cached("SELECT rowid FROM messages WHERE chat = ?1 AND id = ?2 AND from_me = ?3")?
            .query_row(params![chat, id, from_me], |row| row.get(0))?;

        self.conn
            .prepare_cached(
                "UPDATE chats SET last_ts = ?2, last_rowid = ?3
                 WHERE jid = ?1 AND (last_rowid IS NULL OR last_ts <= ?2)",
            )?
            .execute(params![chat, ts, rowid])?;

        if !from_me
            && let Some(sender) = &sender
            && let Some(push_name) = wmi.push_name.as_deref().filter(|n| !n.is_empty())
        {
            self.put_contact(&ContactRow {
                jid: sender.clone(),
                push_name: Some(push_name.to_string()),
                ..Default::default()
            })?;
        }

        for reaction in &wmi.reactions {
            let Some(reaction_key) = reaction.key.as_option() else {
                continue;
            };
            let reactor = if reaction_key.from_me.unwrap_or(false) {
                "me".to_string()
            } else {
                reaction_key
                    .participant
                    .clone()
                    .filter(|p| !p.is_empty())
                    .or_else(|| reaction_key.remote_jid.clone())
                    .unwrap_or_default()
            };
            if reactor.is_empty() {
                continue;
            }
            self.put_reaction(
                &chat,
                id,
                &reactor,
                reaction.text.as_deref().unwrap_or(""),
                reaction.sender_timestamp_ms.unwrap_or(0) / 1000,
            )?;
        }

        for receipt in &wmi.user_receipt {
            self.put_receipt(
                &chat,
                id,
                &receipt.user_jid,
                receipt.receipt_timestamp,
                receipt.read_timestamp,
                receipt.played_timestamp,
            )?;
        }

        if !wmi.poll_updates.is_empty() {
            self.put_poll_votes_of(&chat, id, wmi)?;
        }

        Ok(Some(rowid))
    }

    pub fn set_status(&self, chat: &str, id: &str, status: Ack) -> Result<bool> {
        let chat = self.canonical(chat)?;
        let changed = self.conn.execute(
            "UPDATE messages SET status = ?3 WHERE chat = ?1 AND id = ?2 AND from_me = 1 AND status < ?3",
            params![chat, id, status as i32],
        )?;
        Ok(changed > 0)
    }

    pub fn set_starred(&self, chat: &str, id: &str, starred: bool) -> Result<()> {
        let chat = self.canonical(chat)?;
        self.conn.execute(
            "UPDATE messages SET starred = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, starred],
        )?;
        Ok(())
    }

    /// Replaces a message with the "This message was deleted" placeholder.
    pub fn mark_revoked(&self, chat: &str, id: &str) -> Result<bool> {
        let chat = self.canonical(chat)?;
        let changed = self.conn.execute(
            "UPDATE messages SET kind = ?3, text = NULL, media_path = NULL WHERE chat = ?1 AND id = ?2",
            params![chat, id, MsgKind::Revoked as i32],
        )?;
        Ok(changed > 0)
    }

    pub fn delete_message(&self, chat: &str, id: &str) -> Result<()> {
        let chat = self.canonical(chat)?;
        self.conn.execute(
            "DELETE FROM messages WHERE chat = ?1 AND id = ?2",
            params![chat, id],
        )?;
        self.refresh_last(&chat)
    }

    pub fn set_media_path(&self, chat: &str, id: &str, path: &str) -> Result<()> {
        let chat = self.canonical(chat)?;
        self.conn.execute(
            "UPDATE messages SET media_path = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, path],
        )?;
        Ok(())
    }

    pub fn set_unread(&self, chat: &str, unread: i64) -> Result<()> {
        let chat = self.canonical(chat)?;
        self.conn.execute(
            "UPDATE chats SET unread = ?2, marked_unread = 0 WHERE jid = ?1",
            params![chat, unread],
        )?;
        Ok(())
    }

    pub fn bump_unread(&self, chat: &str) -> Result<()> {
        let chat = self.canonical(chat)?;
        self.conn
            .execute("UPDATE chats SET unread = unread + 1 WHERE jid = ?1", [chat])?;
        Ok(())
    }
}

impl Tx<'_> {
    /// The stored protocol record of a message, undecoded.
    pub fn raw_record(&self, chat: &str, id: &str) -> Result<Option<Vec<u8>>> {
        self.conn
            .query_row(
                "SELECT proto FROM messages WHERE chat = ?1 AND id = ?2 ORDER BY from_me DESC LIMIT 1",
                params![chat, id],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn mark_edited(&self, chat: &str, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET edited = 1 WHERE chat = ?1 AND id = ?2",
            params![chat, id],
        )?;
        Ok(())
    }

    /// Removes a chat and everything in it.
    pub fn delete_chat(&self, chat: &str) -> Result<()> {
        let chat = self.canonical(chat)?;
        self.conn.execute("DELETE FROM messages WHERE chat = ?1", [&chat])?;
        self.conn.execute("DELETE FROM reactions WHERE chat = ?1", [&chat])?;
        self.conn.execute("DELETE FROM receipts WHERE chat = ?1", [&chat])?;
        self.conn.execute("DELETE FROM chats WHERE jid = ?1", [&chat])?;
        Ok(())
    }

    /// Empties a chat but keeps it in the list.
    pub fn clear_chat(&self, chat: &str) -> Result<()> {
        let chat = self.canonical(chat)?;
        self.conn.execute("DELETE FROM messages WHERE chat = ?1", [&chat])?;
        self.conn.execute("DELETE FROM reactions WHERE chat = ?1", [&chat])?;
        self.conn.execute("DELETE FROM receipts WHERE chat = ?1", [&chat])?;
        self.conn.execute(
            "UPDATE chats SET last_rowid = NULL, unread = 0 WHERE jid = ?1",
            [&chat],
        )?;
        Ok(())
    }

    pub fn set_draft(&self, chat: &str, draft: Option<&str>) -> Result<()> {
        let chat = self.canonical(chat)?;
        self.conn.execute(
            "UPDATE chats SET draft = ?2 WHERE jid = ?1",
            params![chat, draft.filter(|d| !d.trim().is_empty())],
        )?;
        Ok(())
    }
}

impl Store {
    /// The decoded protocol record of a message.
    pub fn record(&self, chat: &str, id: &str) -> Result<Option<wa::WebMessageInfo>> {
        let chat = self.canonical(chat)?;
        let raw = self.with(|tx| tx.raw_record(&chat, id))?;
        Ok(raw.and_then(|bytes| wa::WebMessageInfo::decode_from_slice(&bytes).ok()))
    }

    /// IDs of the newest incoming messages of a chat, with their original sender JIDs,
    /// for sending read receipts.
    pub fn unread_incoming(&self, chat: &str, limit: usize) -> Result<Vec<(String, Option<String>)>> {
        self.with(|tx| {
            let mut stmt = tx.conn.prepare_cached(
                "SELECT id, proto FROM messages WHERE chat = ?1 AND from_me = 0
                 ORDER BY ts DESC, rowid DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![chat, limit as i64], |row| {
                let id: String = row.get(0)?;
                let proto: Vec<u8> = row.get(1)?;
                Ok((id, proto))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (id, proto) = row?;
                let participant = wa::WebMessageInfo::decode_from_slice(&proto).ok().and_then(|r| {
                    r.participant
                        .clone()
                        .or_else(|| r.key.as_option().and_then(|k| k.participant.clone()))
                });
                out.push((id, participant.filter(|p| !p.is_empty())));
            }
            Ok(out)
        })
    }

    pub fn unread_count(&self, chat: &str) -> Result<i64> {
        self.with(|tx| {
            tx.conn
                .query_row("SELECT unread FROM chats WHERE jid = ?1", [chat], |row| row.get(0))
                .optional()
                .map(|v| v.unwrap_or(0))
        })
    }
}

impl Tx<'_> {
    /// Sets a delivery state even when it goes backwards (a failed send).
    pub fn force_status(&self, chat: &str, id: &str, status: Ack) -> Result<()> {
        let chat = self.canonical(chat)?;
        self.conn.execute(
            "UPDATE messages SET status = ?3 WHERE chat = ?1 AND id = ?2 AND from_me = 1",
            params![chat, id, status as i32],
        )?;
        Ok(())
    }
}
