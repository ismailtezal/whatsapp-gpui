//! Recovery tool: adds to one chat database the rows that others have and it lacks.
//! Rows already in the target are never replaced. Every file is checked first; a
//! damaged source is refused, and so is a target that fails its check afterwards.
//!
//!   wa-merge <target.db> <source.db> [more sources...]
//!   wa-merge --refresh <target.db>                 after a merge: latest-message pointers
//!                                                   and ticks
//!   wa-merge --restore <backup.db> <target.db>     copy a backup to a target that does
//!                                                   not exist yet (never overwrites)
//!
//! Run it only while no app is using the target. Give full, real paths.

use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags};

fn counts(path: &Path) -> Result<String> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let count = |table: &str| -> String {
        db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get::<_, i64>(0))
            .map(|n| n.to_string())
            .unwrap_or_else(|error| format!("unreadable ({error})"))
    };
    Ok(format!(
        "messages {}, chats {}, contacts {}, reactions {}, receipts {}, settings {}",
        count("messages"),
        count("chats"),
        count("contacts"),
        count("reactions"),
        count("receipts"),
        count("kv")
    ))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--restore") {
        let [_, backup, target] = args.as_slice() else {
            bail!("usage: wa-merge --restore <backup.db> <target.db>");
        };
        let (backup, target) = (Path::new(backup), Path::new(target));
        if target.exists() {
            bail!("{} already exists; it is not overwritten", target.display());
        }
        for suffix in ["-wal", "-shm"] {
            let stray = target.with_file_name(format!(
                "{}{suffix}",
                target.file_name().map(|name| name.to_string_lossy()).unwrap_or_default()
            ));
            if stray.exists() {
                bail!("{} exists: a database must not be put next to another one's log", stray.display());
            }
        }
        if !whatsapp_gpui::guard::is_intact(backup) {
            bail!("{} fails its integrity check", backup.display());
        }
        std::fs::copy(backup, target).context("copying the backup")?;
        println!("restored {} from {}", target.display(), backup.display());
        println!("now: {}", counts(target)?);
        return Ok(());
    }

    if args.first().map(String::as_str) == Some("--refresh") {
        // After a merge: each chat points at its newest message again, and the ticks of
        // our own messages follow the receipts on record.
        let [_, target] = args.as_slice() else {
            bail!("usage: wa-merge --refresh <target.db>");
        };
        let store = whatsapp_gpui::store::Store::open(Path::new(target))?;
        let mut chats = store.chats(false)?;
        chats.extend(store.chats(true)?);
        let ticks = store.batch(|tx| {
            for chat in &chats {
                tx.refresh_last(&chat.jid)?;
            }
            tx.reapply_receipts()
        })?;
        store.set_kv("receipts-reapplied", "1")?;
        println!("refreshed {} chats; the ticks of {ticks} messages were brought up to their receipts", chats.len());
        return Ok(());
    }

    let [target, sources @ ..] = args.as_slice() else {
        bail!("usage: wa-merge <target.db> <source.db> [more sources...]");
    };
    if sources.is_empty() {
        bail!("give at least one source");
    }
    let target = Path::new(target);
    if !whatsapp_gpui::guard::is_intact(target) {
        bail!("the target {} fails its integrity check", target.display());
    }
    println!("target before: {}", counts(target)?);
    for source in sources {
        let source = Path::new(source);
        if !whatsapp_gpui::guard::is_intact(source) {
            println!("skipped {}: it fails its integrity check", source.display());
            continue;
        }
        println!("source {}: {}", source.display(), counts(source)?);
        whatsapp_gpui::guard::merge_from(target, source)
            .with_context(|| format!("merging {}", source.display()))?;
        println!("target now: {}", counts(target)?);
    }
    if !whatsapp_gpui::guard::is_intact(target) {
        bail!("the target fails its integrity check after merging");
    }
    println!("the target passes its integrity check");
    Ok(())
}
