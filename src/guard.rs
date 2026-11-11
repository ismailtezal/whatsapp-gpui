//! Keeps the chat database safe.
//!
//! - [`open`] opens the database for the app. If SQLite cannot read it, the damaged
//!   file is set aside and a new one is rebuilt from it ([`crate::salvage`]), topped up
//!   from the newest backup, and the app starts normally with a notice.
//! - [`maintain`] runs in the background after start: a full integrity check, then one
//!   backup per day (the three newest are kept). A failed check leaves a marker so the
//!   next start repairs the file.
//!
//! Setting a damaged database aside always moves its `-wal` and `-shm` files with it:
//! leaving them next to a new file of the same name would make SQLite apply the old
//! write-ahead log to the new database.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::store::Store;

/// How many daily backups are kept.
const BACKUPS_KEPT: usize = 3;

pub struct Opened {
    pub store: Store,
    /// Something the owner should be told (a repair happened), if anything.
    pub notice: Option<String>,
}

fn backup_dir() -> PathBuf {
    let dir = crate::paths::data_dir().join("backups");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn repair_marker() -> PathBuf {
    crate::paths::data_dir().join("repair-needed")
}

fn stamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    seconds.to_string()
}

/// Backups, newest first.
fn backups() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(backup_dir())
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension().is_some_and(|ext| ext == "db")
                        && path
                            .file_name()
                            .is_some_and(|name| name.to_string_lossy().starts_with("chats-"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files.reverse();
    files
}

/// Whether SQLite reports the file as fully intact.
pub fn is_intact(path: &Path) -> bool {
    let check = || -> Result<bool> {
        let db = Connection::open(path)?;
        db.busy_timeout(Duration::from_secs(10))?;
        let verdict: String = db.query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))?;
        Ok(verdict == "ok")
    };
    check().unwrap_or(false)
}

/// Opens the store and reads a little from the main tables, which fails on the kind of
/// damage that would otherwise only show up later as missing chats.
fn open_and_probe(path: &Path) -> Result<Store> {
    let store = Store::open(path)?;
    store.probe()?;
    Ok(store)
}

/// Opens the chat database, repairing it first if it is damaged.
pub fn open() -> Result<Opened> {
    let path = crate::paths::chat_db();
    let marked = repair_marker().exists();
    if !marked {
        match open_and_probe(&path) {
            Ok(store) => {
                return Ok(Opened {
                    store,
                    notice: None,
                });
            }
            Err(error) => eprintln!("the chat database cannot be read ({error:#}); repairing it"),
        }
    }
    let notice = repair(&path).context("repairing the chat database")?;
    let _ = std::fs::remove_file(repair_marker());
    Ok(Opened {
        store: open_and_probe(&path)?,
        notice: Some(notice),
    })
}

/// Sets the damaged database aside and builds a new one at `path` from everything that
/// can be recovered. Returns a sentence describing what was done.
fn repair(path: &Path) -> Result<String> {
    let aside = crate::paths::data_dir().join(format!("damaged-{}", stamp()));
    std::fs::create_dir_all(&aside)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "chats.db".to_string());
    // The database and its write-ahead log travel together.
    let mut moved_any = false;
    for suffix in ["", "-wal", "-shm"] {
        let from = path.with_file_name(format!("{name}{suffix}"));
        if from.exists() {
            std::fs::rename(&from, aside.join(format!("{name}{suffix}")))
                .with_context(|| format!("moving {} aside", from.display()))?;
            moved_any = true;
        }
    }
    if !moved_any {
        return Ok("The chat database was missing; a new one was created.".to_string());
    }

    // Work on a copy, so the damaged original stays exactly as it was found.
    let original = aside.join(&name);
    let work = aside.join("work.db");
    std::fs::copy(&original, &work)?;
    for suffix in ["-wal", "-shm"] {
        let from = aside.join(format!("{name}{suffix}"));
        if from.exists() {
            let _ = std::fs::copy(&from, aside.join(format!("work.db{suffix}")));
        }
    }
    // Fold the write-ahead log into the copy, so the page-by-page reader sees the
    // newest rows too. Best effort: on a badly damaged file this may fail.
    if let Ok(db) = Connection::open(&work) {
        let _ = db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    }

    let mut log = Vec::new();
    let rebuilt = crate::salvage::rebuild(&work.to_string_lossy(), &path.to_string_lossy(), |line| {
        eprintln!("repair: {line}");
        log.push(line);
    });
    let _ = std::fs::write(aside.join("repair.log"), log.join("\n"));

    let backup = backups().into_iter().find(|backup| is_intact(backup));
    match rebuilt {
        Ok(()) => {
            // Anything the newest backup has that the rebuilt file lacks.
            if let Some(backup) = &backup {
                let _ = merge_from(path, backup);
            }
            Ok(format!(
                "The chat database was damaged and has been rebuilt. The damaged file is kept in {}.",
                aside.display()
            ))
        }
        Err(error) => {
            // Rebuilding failed: fall back to the newest good backup, or start empty.
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(path.with_file_name(format!("{name}{suffix}")));
            }
            match backup {
                Some(backup) => {
                    std::fs::copy(&backup, path)?;
                    Ok(format!(
                        "The chat database was damaged and could not be rebuilt ({error:#}); the backup {} was restored. The damaged file is kept in {}.",
                        backup.display(),
                        aside.display()
                    ))
                }
                None => Ok(format!(
                    "The chat database was damaged and could not be rebuilt ({error:#}); a new one was started. The damaged file is kept in {}.",
                    aside.display()
                )),
            }
        }
    }
}

/// Copies rows that `backup` has and `path` lacks. Existing rows are never replaced.
fn merge_from(path: &Path, backup: &Path) -> Result<()> {
    let db = Connection::open(path)?;
    db.execute("ATTACH DATABASE ?1 AS old", [backup.to_string_lossy().to_string()])?;
    db.execute_batch(
        "INSERT OR IGNORE INTO messages (chat, id, from_me, sender, ts, kind, text, status, starred, quoted_id, edited, forwarded, media_path, proto)
            SELECT chat, id, from_me, sender, ts, kind, text, status, starred, quoted_id, edited, forwarded, media_path, proto FROM old.messages;
         INSERT OR IGNORE INTO chats SELECT * FROM old.chats;
         INSERT OR IGNORE INTO contacts SELECT * FROM old.contacts;
         INSERT OR IGNORE INTO lid_map SELECT * FROM old.lid_map;
         INSERT OR IGNORE INTO reactions SELECT * FROM old.reactions;
         INSERT OR IGNORE INTO receipts SELECT * FROM old.receipts;
         INSERT OR IGNORE INTO group_members SELECT * FROM old.group_members;
         INSERT OR IGNORE INTO calls SELECT * FROM old.calls;
         INSERT OR IGNORE INTO poll_votes SELECT * FROM old.poll_votes;
         INSERT OR IGNORE INTO pins SELECT * FROM old.pins;
         INSERT OR IGNORE INTO kv SELECT * FROM old.kv;
         DETACH DATABASE old;",
    )?;
    Ok(())
}

/// Background upkeep, run once after the app has started: checks the whole database
/// and, when it is intact, makes today's backup. Returns a notice for the owner when
/// the check fails. Blocks for a few seconds; call it off the interface thread.
pub fn maintain() -> Option<String> {
    let path = crate::paths::chat_db();
    if !is_intact(&path) {
        let _ = std::fs::write(repair_marker(), b"the integrity check failed");
        return Some(
            "The chat database failed its integrity check. It will be repaired the next time the app starts."
                .to_string(),
        );
    }
    if let Err(error) = backup_today(&path) {
        eprintln!("backup failed: {error:#}");
    }
    None
}

/// Makes today's backup unless it exists, and removes backups beyond the newest few.
fn backup_today(path: &Path) -> Result<()> {
    let day: String = {
        let db = Connection::open(path)?;
        db.query_row("SELECT strftime('%Y-%m-%d', 'now', 'localtime')", [], |row| row.get(0))?
    };
    let target = backup_dir().join(format!("chats-{day}.db"));
    if !target.exists() {
        let partial = backup_dir().join(format!("chats-{day}.partial"));
        let _ = std::fs::remove_file(&partial);
        {
            let db = Connection::open(path)?;
            db.busy_timeout(Duration::from_secs(30))?;
            // A consistent copy, safe to take while the app is writing.
            db.execute("VACUUM INTO ?1", [partial.to_string_lossy().to_string()])?;
        }
        if is_intact(&partial) {
            std::fs::rename(&partial, &target)?;
        } else {
            let _ = std::fs::remove_file(&partial);
            anyhow::bail!("the copy did not pass its integrity check");
        }
    }
    for old in backups().into_iter().skip(BACKUPS_KEPT) {
        let _ = std::fs::remove_file(old);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wa-guard-{}-{}", name, std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn a_damaged_file_is_not_reported_intact() {
        let dir = scratch("intact");
        let path = dir.join("t.db");
        drop(Store::open(&path).expect("open"));
        assert!(is_intact(&path));

        // Overwrite the first page of the schema table's content with noise.
        let mut bytes = std::fs::read(&path).expect("read");
        for byte in bytes.iter_mut().skip(100).take(2000) {
            *byte = 0xA5;
        }
        std::fs::write(&path, bytes).expect("write");
        let _ = std::fs::remove_file(dir.join("t.db-wal"));
        let _ = std::fs::remove_file(dir.join("t.db-shm"));
        assert!(!is_intact(&path));
    }
}
