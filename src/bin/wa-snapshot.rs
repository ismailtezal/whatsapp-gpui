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
    // The real data folder: whatever WA_DATA_DIR says is ignored on purpose.
    let real = std::env::var_os("LOCALAPPDATA")
        .map(|dir| Path::new(&dir).join("whatsapp-gpui"))
        .context("LOCALAPPDATA is not set")?;
    if folder == real {
        bail!("the snapshot folder must not be the real data folder");
    }
    std::fs::create_dir_all(folder)?;

    // A fresh copy each time; the old one goes together with its write-ahead log.
    for name in ["chats.db", "chats.db-wal", "chats.db-shm"] {
        let _ = std::fs::remove_file(folder.join(name));
    }
    let source = real.join("chats.db");
    if source.exists() {
        let db = Connection::open(&source)?;
        db.busy_timeout(Duration::from_secs(30))?;
        db.execute(
            "VACUUM INTO ?1",
            [folder.join("chats.db").to_string_lossy().to_string()],
        )
        .context("copying the chat database")?;
        println!("copied the chat database");
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
