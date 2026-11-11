//! Makes a throwaway copy of the app's data for interface tests, so a test window never
//! opens the real chat database.
//!
//! `wa-snapshot <folder>` writes a consistent copy of the chat database into the folder
//! (safe to take while the app is running) and links the media, profile picture and
//! history folders into it. The session (the linked-device keys) is never copied: test
//! copies do not connect.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

fn main() -> Result<()> {
    let folder = std::env::args().nth(1).ok_or_else(|| anyhow::anyhow!("usage: wa-snapshot <folder>"))?;
    let folder = Path::new(&folder);
    // The app's own data folder, by its true location (see `paths`): whatever
    // WA_DATA_DIR says is ignored on purpose.
    let real = whatsapp_gpui::paths::default_data_dir();
    if folder == real || std::fs::canonicalize(folder).ok() == std::fs::canonicalize(&real).ok() {
        bail!("the snapshot folder must not be the real data folder");
    }
    std::fs::create_dir_all(folder)?;

    let source = real.join("chats.db");
    if source.exists() {
        // The copy is made beside the old snapshot and only then takes its place, so a
        // copy that fails (a damaged source) leaves the previous snapshot standing.
        let fresh = folder.join("chats.new.db");
        let _ = std::fs::remove_file(&fresh);
        {
            let db = Connection::open(&source)?;
            db.busy_timeout(Duration::from_secs(30))?;
            db.execute("VACUUM INTO ?1", [fresh.to_string_lossy().to_string()])
                .context("copying the chat database")?;
        }
        // The old snapshot goes together with its write-ahead log.
        for name in ["chats.db", "chats.db-wal", "chats.db-shm"] {
            let _ = std::fs::remove_file(folder.join(name));
        }
        std::fs::rename(&fresh, folder.join("chats.db")).context("putting the new snapshot in place")?;
        println!("copied the chat database from {}", real.display());
    } else {
        println!("there is no chat database yet; the test copy will start empty");
    }

    // Media and pictures are only read by a test copy, so links are enough.
    for name in ["media", "avatars", "history"] {
        let link = folder.join(name);
        let target = real.join(name);
        if link.exists() || !target.exists() {
            continue;
        }
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .context("creating a folder link")?;
        if !status.status.success() {
            println!("could not link {name}: {}", String::from_utf8_lossy(&status.stderr).trim());
        }
    }
    println!("snapshot ready in {}", folder.display());
    Ok(())
}
