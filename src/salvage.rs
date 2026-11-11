//! Rebuilds the chat database from a damaged copy.
//!
//! [`rebuild`] makes a fresh database and fills it with everything that can still be
//! read from a damaged one:
//!
//! - tables SQLite can still read are copied through SQLite;
//! - for the others (chats, messages, the LID map, receipts) the file is read page by
//!   page, without SQLite: every data page whose rows have the right shape for the table
//!   is used, whether or not the damaged index tree still points to it;
//! - the history files saved when the device was linked are imported again, which
//!   brings back anything older that was lost.
//!
//! The damaged file is only read. The search index is rebuilt as rows are inserted.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, params_from_iter};

struct Raw {
    bytes: Vec<u8>,
    page_size: usize,
}

fn varint(data: &[u8], mut at: usize) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    for index in 0..9 {
        let byte = *data.get(at)?;
        at += 1;
        if index == 8 {
            return Some(((value << 8) | byte as u64, at));
        }
        value = (value << 7) | (byte & 0x7f) as u64;
        if byte & 0x80 == 0 {
            return Some((value, at));
        }
    }
    None
}

impl Raw {
    fn open(path: &str) -> Result<Self> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {path}"))?;
        if bytes.len() < 100 || &bytes[..15] != b"SQLite format 3" {
            bail!("{path} is not a SQLite file");
        }
        let page_size = match u16::from_be_bytes([bytes[16], bytes[17]]) {
            1 => 65536,
            size => size as usize,
        };
        Ok(Raw { bytes, page_size })
    }

    fn pages(&self) -> usize {
        self.bytes.len() / self.page_size
    }

    fn page(&self, number: usize) -> Option<&[u8]> {
        if number == 0 || number > self.pages() {
            return None;
        }
        let start = (number - 1) * self.page_size;
        self.bytes.get(start..start + self.page_size)
    }

    /// A table leaf cell: its rowid and its whole payload, following overflow pages.
    fn cell(&self, page: &[u8], cell: usize) -> Option<(i64, Vec<u8>)> {
        let (size, at) = varint(page, cell)?;
        let (rowid, at) = varint(page, at)?;
        let size = size as usize;
        if size > 64 * 1024 * 1024 {
            return None;
        }
        let usable = self.page_size;
        let max_local = usable - 35;
        let min_local = (usable - 12) * 32 / 255 - 23;
        let local = if size <= max_local {
            size
        } else {
            let k = min_local + (size - min_local) % (usable - 4);
            if k <= max_local { k } else { min_local }
        };
        let mut data = page.get(at..at + local)?.to_vec();
        if local < size {
            let mut next = u32::from_be_bytes(page.get(at + local..at + local + 4)?.try_into().ok()?) as usize;
            let mut hops = 0;
            while data.len() < size {
                let overflow = self.page(next)?;
                next = u32::from_be_bytes(overflow[..4].try_into().ok()?) as usize;
                let take = (size - data.len()).min(usable - 4);
                data.extend_from_slice(overflow.get(4..4 + take)?);
                hops += 1;
                if hops > 100_000 {
                    return None;
                }
            }
        }
        Some((rowid as i64, data))
    }

    /// Every row of every table leaf page in the file, with its page number.
    fn rows(&self) -> Vec<(usize, i64, Vec<Value>)> {
        let mut out = Vec::new();
        for number in 2..=self.pages() {
            let Some(page) = self.page(number) else {
                continue;
            };
            if page[0] != 0x0d {
                continue;
            }
            let cells = u16::from_be_bytes([page[3], page[4]]) as usize;
            if cells > self.page_size / 4 {
                continue;
            }
            for index in 0..cells {
                let pointer = 8 + index * 2;
                let Some(bytes) = page.get(pointer..pointer + 2) else {
                    break;
                };
                let cell = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
                if let Some((rowid, data)) = self.cell(page, cell)
                    && let Some(values) = record(&data)
                {
                    out.push((number, rowid, values));
                }
            }
        }
        out
    }
}

/// Decodes a record into values. `None` when the bytes are not a well-formed record.
fn record(data: &[u8]) -> Option<Vec<Value>> {
    let (header_len, mut at) = varint(data, 0)?;
    let header_len = header_len as usize;
    if header_len > data.len() || header_len == 0 {
        return None;
    }
    let mut body = header_len;
    let mut out = Vec::new();
    while at < header_len {
        let (kind, next) = varint(data, at)?;
        at = next;
        let value = match kind {
            0 => Value::Null,
            1..=6 => {
                let len = [0usize, 1, 2, 3, 4, 6, 8][kind as usize];
                let bytes = data.get(body..body + len)?;
                // Big-endian, sign-extended from the first byte.
                let mut value: i64 = if bytes[0] & 0x80 != 0 { -1 } else { 0 };
                for byte in bytes {
                    value = (value << 8) | *byte as i64;
                }
                body += len;
                Value::Integer(value)
            }
            7 => {
                let bytes: [u8; 8] = data.get(body..body + 8)?.try_into().ok()?;
                body += 8;
                Value::Real(f64::from_be_bytes(bytes))
            }
            8 => Value::Integer(0),
            9 => Value::Integer(1),
            10 | 11 => return None,
            kind if kind % 2 == 0 => {
                let len = ((kind - 12) / 2) as usize;
                let bytes = data.get(body..body + len)?.to_vec();
                body += len;
                Value::Blob(bytes)
            }
            kind => {
                let len = ((kind - 13) / 2) as usize;
                let text = std::str::from_utf8(data.get(body..body + len)?).ok()?.to_string();
                body += len;
                Value::Text(text)
            }
        };
        out.push(value);
        if out.len() > 64 {
            return None;
        }
    }
    (body == data.len()).then_some(out)
}

fn is_text(value: &Value) -> bool {
    matches!(value, Value::Text(_))
}
fn is_int(value: &Value) -> bool {
    matches!(value, Value::Integer(_))
}
fn text_or_null(value: &Value) -> bool {
    matches!(value, Value::Text(_) | Value::Null)
}
fn int_or_null(value: &Value) -> bool {
    matches!(value, Value::Integer(_) | Value::Null)
}
fn text(value: &Value) -> &str {
    match value {
        Value::Text(text) => text,
        _ => "",
    }
}
fn int(value: &Value) -> i64 {
    match value {
        Value::Integer(value) => *value,
        _ => 0,
    }
}

/// A chat address: something@something.
fn is_jid(value: &Value) -> bool {
    let text = text(value);
    text.contains('@') && !text.contains(' ') && text.len() < 120
}

fn looks_like_message(row: &[Value]) -> bool {
    row.len() == 14
        && is_jid(&row[0])
        && is_text(&row[1])
        && is_int(&row[2])
        && text_or_null(&row[3])
        && is_int(&row[4])
        && is_int(&row[5])
        && text_or_null(&row[6])
        && is_int(&row[7])
        && is_int(&row[8])
        && text_or_null(&row[9])
        && is_int(&row[10])
        && is_int(&row[11])
        && text_or_null(&row[12])
        && matches!(row[13], Value::Blob(_))
}

/// Chats gained columns over time; older rows are shorter and SQLite fills the rest
/// with the column defaults.
fn looks_like_chat(row: &[Value]) -> bool {
    (16..=21).contains(&row.len())
        && is_jid(&row[0])
        && is_int(&row[1])
        && text_or_null(&row[2])
        && is_int(&row[3])
        && int_or_null(&row[4])
        && row[5..15].iter().all(is_int)
        && text_or_null(&row[15])
}

/// Who received or read a message: chat, message, person, then three optional times.
fn looks_like_receipt(row: &[Value]) -> bool {
    row.len() == 6
        && is_jid(&row[0])
        && is_text(&row[1])
        && person(&row[2])
        && row[3..6].iter().all(int_or_null)
}

/// A small table that can be recognised by the shape of its rows when SQLite cannot
/// read it: its columns in order, how many leading columns make its key, and the test.
struct Shape {
    table: &'static str,
    columns: &'static [&'static str],
    key: usize,
    matches: fn(&[Value]) -> bool,
}

/// A person as these tables store one: an address, or "me" for the account itself.
fn person(value: &Value) -> bool {
    is_jid(value) || text(value) == "me"
}

fn no_at(value: &Value) -> bool {
    is_text(value) && !text(value).contains('@')
}

const SHAPES: [Shape; 8] = [
    Shape {
        table: "contacts",
        columns: &[
            "jid", "lid", "name", "first_name", "push_name", "verified_name", "username", "about",
            "avatar_id", "avatar_path",
        ],
        key: 1,
        matches: |row| row.len() == 10 && is_jid(&row[0]) && row[1..].iter().all(text_or_null),
    },
    Shape {
        table: "reactions",
        columns: &["chat", "msg_id", "sender", "emoji", "ts"],
        key: 3,
        // An emoji is never plain ASCII; that tells a reaction from a poll vote, whose
        // options are hexadecimal. A row with neither (a withdrawn reaction or vote)
        // says nothing and is left out.
        matches: |row| {
            row.len() == 5
                && is_jid(&row[0])
                && no_at(&row[1])
                && person(&row[2])
                && is_text(&row[3])
                && !text(&row[3]).is_ascii()
                && is_int(&row[4])
        },
    },
    Shape {
        table: "receipts",
        columns: &["chat", "msg_id", "user", "delivered", "read", "played"],
        key: 3,
        matches: looks_like_receipt,
    },
    Shape {
        table: "group_members",
        columns: &["group_jid", "user", "rank"],
        key: 2,
        matches: |row| row.len() == 3 && text(&row[0]).ends_with("@g.us") && is_jid(&row[1]) && is_int(&row[2]),
    },
    Shape {
        table: "calls",
        columns: &["id", "peer", "ts", "duration", "video", "outgoing", "outcome", "group_jid"],
        key: 1,
        matches: |row| {
            row.len() == 8
                && no_at(&row[0])
                && is_jid(&row[1])
                && row[2..7].iter().all(is_int)
                && text_or_null(&row[7])
        },
    },
    Shape {
        table: "poll_votes",
        columns: &["chat", "poll_id", "voter", "options", "ts"],
        key: 3,
        matches: |row| {
            row.len() == 5
                && is_jid(&row[0])
                && no_at(&row[1])
                && person(&row[2])
                && is_text(&row[3])
                && !text(&row[3]).is_empty()
                && text(&row[3]).is_ascii()
                && is_int(&row[4])
        },
    },
    Shape {
        table: "pins",
        columns: &["chat", "msg_id", "ts"],
        key: 2,
        matches: |row| row.len() == 3 && is_jid(&row[0]) && no_at(&row[1]) && is_int(&row[2]),
    },
    Shape {
        table: "kv",
        columns: &["key", "value"],
        key: 1,
        // Setting names are short, lower-case words joined by underscores.
        matches: |row| {
            row.len() == 2
                && is_text(&row[1])
                && !text(&row[0]).is_empty()
                && text(&row[0]).len() <= 40
                && text(&row[0])
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_' || byte.is_ascii_digit())
        },
    },
];

fn looks_like_lid(row: &[Value]) -> bool {
    row.len() == 2 && text(&row[0]).ends_with("@lid") && is_jid(&row[1])
}

const CHAT_COLUMNS: [&str; 21] = [
    "jid",
    "kind",
    "name",
    "last_ts",
    "last_rowid",
    "unread",
    "unread_mentions",
    "marked_unread",
    "archived",
    "pinned",
    "mute_until",
    "ephemeral",
    "read_only",
    "locked",
    "is_parent",
    "parent_group",
    "description",
    "created_at",
    "created_by",
    "draft",
    "hidden",
];

/// Tables copied through SQLite when it can still read them.
const SQL_TABLES: [&str; 8] = [
    "contacts",
    "reactions",
    "receipts",
    "group_members",
    "calls",
    "poll_votes",
    "pins",
    "kv",
];

/// Rebuilds `fresh` (which must not exist) from `damaged`. Each step is reported
/// through `say`.
pub fn rebuild(damaged: &str, fresh: &str, mut say: impl FnMut(String)) -> Result<()> {
    if std::path::Path::new(fresh).exists() {
        bail!("{fresh} already exists; choose a new file name");
    }
    // A fresh database with the app's schema, search index and triggers.
    drop(crate::store::Store::open(std::path::Path::new(fresh))?);
    let mut out = Connection::open(fresh)?;
    out.pragma_update(None, "synchronous", "OFF")?;

    // 1. Whatever SQLite can still read, table by table.
    let mut unreadable = Vec::new();
    match Connection::open_with_flags(damaged, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(old) => {
            for table in SQL_TABLES {
                match copy_table(&old, &mut out, table) {
                    Ok(count) => say(format!("{table}: {count} rows copied through SQLite")),
                    Err(error) => {
                        say(format!("{table}: SQLite cannot read it ({error})"));
                        unreadable.push(table);
                    }
                }
            }
        }
        Err(error) => {
            say(format!("SQLite cannot open the damaged file at all ({error})"));
            unreadable.extend(SQL_TABLES);
        }
    }

    // 2. Chats, messages and the LID map, read from the pages directly.
    let raw = Raw::open(damaged)?;
    let rows = raw.rows();
    say(format!("{} rows found on {} pages", rows.len(), raw.pages()));

    // Messages: one per (chat, id, from_me). Where the damage left two versions of a
    // row, keep the further-along one (higher status, then a downloaded file, then the
    // later page).
    let mut messages: HashMap<(String, String, i64), (usize, i64, Vec<Value>)> = HashMap::new();
    let mut chats: HashMap<String, (usize, Vec<Value>)> = HashMap::new();
    let mut lids: HashMap<String, String> = HashMap::new();
    // Rows of the small tables SQLite could not read, by table and key.
    let mut shaped: HashMap<&'static str, HashMap<Vec<String>, Vec<Value>>> = HashMap::new();
    for (page, rowid, row) in rows {
        if looks_like_message(&row) {
            let key = (text(&row[0]).to_string(), text(&row[1]).to_string(), int(&row[2]));
            let rank = |row: &[Value], page: usize| (int(&row[7]), is_text(&row[12]), page);
            match messages.get(&key) {
                Some((old_page, _, old)) if rank(old, *old_page) >= rank(&row, page) => {}
                _ => {
                    messages.insert(key, (page, rowid, row));
                }
            }
        } else if looks_like_chat(&row) {
            let key = text(&row[0]).to_string();
            // The later page wins: a chat row is rewritten whenever a message arrives.
            let newer = |old: &(usize, Vec<Value>)| (int(&old.1[3]), old.0) < (int(&row[3]), page);
            if chats.get(&key).is_none_or(newer) {
                chats.insert(key, (page, row));
            }
        } else if looks_like_lid(&row) {
            lids.insert(text(&row[0]).to_string(), text(&row[1]).to_string());
        } else if let Some(shape) = SHAPES
            .iter()
            .find(|shape| unreadable.contains(&shape.table) && (shape.matches)(&row))
        {
            let key: Vec<String> = row[..shape.key].iter().map(|value| text(value).to_string()).collect();
            // Where two versions of a row survive, keep the one with more filled in
            // (values are only ever added to these rows); a tie goes to the later page.
            let filled = |row: &[Value]| row.iter().filter(|value| !matches!(value, Value::Null)).count();
            let rows = shaped.entry(shape.table).or_default();
            if rows.get(&key).is_none_or(|old| filled(old) <= filled(&row)) {
                rows.insert(key, row);
            }
        }
    }

    let tx = out.transaction()?;
    {
        // Messages keep their row numbers (chats point at them); a number claimed by two
        // different messages goes to the first and the other gets a new one.
        let mut ordered: Vec<(i64, Vec<Value>)> = messages.into_values().map(|(_, rowid, row)| (rowid, row)).collect();
        ordered.sort_by_key(|(rowid, _)| *rowid);
        let mut with_rowid = tx.prepare(
            "INSERT OR IGNORE INTO messages (rowid, chat, id, from_me, sender, ts, kind, text, status, starred, quoted_id, edited, forwarded, media_path, proto) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )?;
        let mut without = tx.prepare(
            "INSERT OR IGNORE INTO messages (chat, id, from_me, sender, ts, kind, text, status, starred, quoted_id, edited, forwarded, media_path, proto) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )?;
        let (mut kept, mut renumbered, mut failed) = (0, 0, 0);
        for (rowid, row) in ordered {
            let mut values = vec![Value::Integer(rowid)];
            values.extend(row.iter().cloned());
            match with_rowid.execute(params_from_iter(values.iter())) {
                Ok(1) => kept += 1,
                Ok(_) => match without.execute(params_from_iter(row.iter())) {
                    Ok(1) => renumbered += 1,
                    _ => failed += 1,
                },
                Err(_) => failed += 1,
            }
        }
        say(format!("messages: {kept} kept, {renumbered} given a new row number, {failed} could not be inserted"));

        let mut chat_count = 0;
        for (_, row) in chats.into_values() {
            let columns = &CHAT_COLUMNS[..row.len()];
            let sql = format!(
                "INSERT OR REPLACE INTO chats ({}) VALUES ({})",
                columns.join(", "),
                vec!["?"; columns.len()].join(", ")
            );
            if tx.execute(&sql, params_from_iter(row.iter())).is_ok() {
                chat_count += 1;
            }
        }
        say(format!("chats: {chat_count}"));

        let mut lid_count = 0;
        for (lid, pn) in lids {
            if tx
                .execute("INSERT OR REPLACE INTO lid_map (lid, pn) VALUES (?, ?)", [lid, pn])
                .is_ok()
            {
                lid_count += 1;
            }
        }
        say(format!("lid_map: {lid_count}"));

        for shape in &SHAPES {
            let Some(rows) = shaped.remove(shape.table) else {
                if unreadable.contains(&shape.table) {
                    say(format!("{}: no rows found on the pages", shape.table));
                    unreadable.retain(|table| *table != shape.table);
                }
                continue;
            };
            let sql = format!(
                "INSERT OR REPLACE INTO {} ({}) VALUES ({})",
                shape.table,
                shape.columns.join(", "),
                vec!["?"; shape.columns.len()].join(", ")
            );
            let mut count = 0;
            for row in rows.into_values() {
                if tx.execute(&sql, params_from_iter(row.iter())).is_ok() {
                    count += 1;
                }
            }
            say(format!("{}: {count} read from the pages", shape.table));
            unreadable.retain(|table| *table != shape.table);
        }
    }
    // Each chat's newest message, by the row numbers as they are now.
    tx.execute_batch(
        "UPDATE chats SET last_rowid = (
            SELECT m.rowid FROM messages m WHERE m.chat = chats.jid ORDER BY m.ts DESC, m.rowid DESC LIMIT 1
         );",
    )?;
    tx.commit()?;
    out.pragma_update(None, "synchronous", "NORMAL")?;

    // Fill any holes from the history files saved when the device was linked.
    let count = |db: &Connection| -> i64 {
        db.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0)).unwrap_or(-1)
    };
    let before = count(&out);
    drop(out);
    let store = crate::store::Store::open(std::path::Path::new(fresh))?;
    let imported = crate::import::import_saved_history(&store);
    drop(store);
    let out = Connection::open(fresh)?;
    match imported {
        Ok(_) => say(format!(
            "saved history re-imported: {} messages were missing and are back; {} in total",
            count(&out) - before,
            count(&out)
        )),
        Err(error) => say(format!("saved history could not be re-imported: {error:#}")),
    }

    let check: Vec<String> = out
        .prepare("PRAGMA integrity_check(10)")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    say(format!("new database check: {}", check.join("; ")));
    if !unreadable.is_empty() {
        say(format!("NOT RECOVERED (SQLite could not read them): {}", unreadable.join(", ")));
    }
    Ok(())
}

/// Copies one table through SQLite. Fails, copying nothing, if any row is unreadable.
fn copy_table(old: &Connection, out: &mut Connection, table: &str) -> Result<usize> {
    let mut statement = old.prepare(&format!("SELECT * FROM \"{table}\""))?;
    let columns: Vec<String> = statement.column_names().iter().map(|name| name.to_string()).collect();
    let mut rows = statement.query([])?;
    let mut read: Vec<Vec<Value>> = Vec::new();
    while let Some(row) = rows.next()? {
        read.push((0..columns.len()).map(|index| row.get::<_, Value>(index)).collect::<Result<_, _>>()?);
    }
    let sql = format!(
        "INSERT OR REPLACE INTO \"{table}\" ({}) VALUES ({})",
        columns.iter().map(|column| format!("\"{column}\"")).collect::<Vec<_>>().join(", "),
        vec!["?"; columns.len()].join(", ")
    );
    let tx = out.transaction()?;
    for row in &read {
        tx.execute(&sql, params_from_iter(row.iter()))?;
    }
    tx.commit()?;
    Ok(read.len())
}
