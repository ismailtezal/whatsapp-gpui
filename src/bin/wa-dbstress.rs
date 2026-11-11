//! Development tool: tries to break a SQLite database the way the app might, to find
//! out what is safe. Always run it on a scratch copy, never on the real chat database.
//!
//! `wa-dbstress writer <file.db> <seconds> [tag]`
//!     Opens the file the way the app does (write-ahead log) and writes rows in a
//!     loop for the given time. Start several, kill some at random, then check the
//!     file with wa-dbcheck.
//!
//! `wa-dbstress voice <file.db> <rounds>`
//!     In one process: reads a lot of the database into SQLite's page cache, then
//!     alternates voice encoding / decoding (the Opus code) with database writes.
//!     If the codec scribbled over memory it does not own, the database would show it.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use rusqlite::Connection;

fn open(path: &str) -> Result<Connection> {
    let db = Connection::open(path)?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    db.pragma_update(None, "synchronous", "NORMAL")?;
    db.busy_timeout(Duration::from_secs(10))?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS stress (id INTEGER PRIMARY KEY, tag TEXT, n INTEGER, filler BLOB);
         CREATE INDEX IF NOT EXISTS stress_by_n ON stress (n);",
    )?;
    Ok(db)
}

fn write_some(db: &mut Connection, tag: &str, counter: &mut i64) -> Result<()> {
    let tx = db.transaction()?;
    for _ in 0..20 {
        *counter += 1;
        // Rows of varied size, some large enough to need overflow pages.
        let size = 200 + (*counter as usize * 7919) % 9000;
        tx.execute(
            "INSERT INTO stress (tag, n, filler) VALUES (?, ?, zeroblob(?))",
            rusqlite::params![tag, *counter, size as i64],
        )?;
    }
    // Updates and deletes too, so pages are rewritten and freed, not only appended.
    tx.execute("UPDATE stress SET n = n + 1 WHERE id % 97 = ?", [*counter % 97])?;
    tx.execute("DELETE FROM stress WHERE id % 211 = ? AND id < ?", [*counter % 211, *counter - 2000])?;
    tx.commit()?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("writer") => {
            let path = args.get(1).ok_or_else(|| anyhow::anyhow!("missing file"))?;
            let seconds: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
            let tag = args.get(3).cloned().unwrap_or_else(|| "w".to_string());
            let mut db = open(path)?;
            let end = Instant::now() + Duration::from_secs(seconds);
            let mut counter = std::process::id() as i64 * 1_000_000;
            while Instant::now() < end {
                write_some(&mut db, &tag, &mut counter)?;
            }
            println!("writer {tag} finished cleanly");
        }
        Some("voice") => {
            let path = args.get(1).ok_or_else(|| anyhow::anyhow!("missing file"))?;
            let rounds: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(50);
            let mut db = open(path)?;
            // Pull a good part of the file into the page cache.
            db.pragma_update(None, "cache_size", -200_000)?;
            let touched: i64 = db
                .query_row("SELECT COUNT(*) + SUM(length(proto)) FROM messages", [], |row| row.get(0))
                .unwrap_or(0);
            println!("warmed the cache ({touched})");
            let mut counter = 0i64;
            for round in 0..rounds {
                let rate = 48_000u32;
                let seconds = 1 + round % 4;
                let samples: Vec<f32> = (0..rate as usize * seconds)
                    .map(|i| {
                        let t = i as f32 / rate as f32;
                        (t * (200. + 37. * round as f32) * std::f32::consts::TAU).sin() * 0.4
                    })
                    .collect();
                let recording = whatsapp_gpui::voice::finish(&samples, rate)?;
                let decoded = whatsapp_gpui::voice::decode_ogg_opus(&recording.ogg)?;
                if decoded.len() != samples.len() {
                    bail!("round {round}: decoded {} samples, expected {}", decoded.len(), samples.len());
                }
                write_some(&mut db, "voice", &mut counter)?;
            }
            let check: String = db.query_row("PRAGMA integrity_check(5)", [], |row| row.get(0))?;
            println!("after {rounds} rounds of encoding and decoding: integrity {check}");
        }
        _ => bail!("usage: wa-dbstress writer <file.db> <seconds> [tag] | voice <file.db> <rounds>"),
    }
    Ok(())
}
