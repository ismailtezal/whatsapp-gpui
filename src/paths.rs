//! Where the app keeps its data: `%LOCALAPPDATA%\whatsapp-gpui`.
//!
//! The folder is resolved to its true location once, and every file is then opened by
//! that true path. This matters when the app is started from inside a packaged
//! (MSIX) program, such as a terminal in a packaged editor. Windows shows such a
//! process a *merged* `AppData\Local`: what it writes goes to the package's private copy
//! (`AppData\Local\Packages\<package>\LocalCache\Local\…`), and a file that copy lacks is
//! silently taken from the real folder instead. A database opened through that merged
//! view can be paired with the write-ahead log of a *different* database, and deleting
//! "the" file can delete the other one. Both happened on 2026-11-11 and damaged two
//! databases. Opened by its true path, a folder is only ever itself.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Where a folder under `AppData\Local` really is for this process: the package's
/// private copy when Windows redirects this process, else None.
///
/// Found by experiment, since a redirected process cannot ask: a marker file is written
/// through the given path and then looked for under each package's private copy.
fn redirected(path: &Path) -> Option<PathBuf> {
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    let lower = |path: &Path| path.to_string_lossy().to_lowercase().replace('/', "\\");
    let (path_text, local_text) = (lower(path), lower(&local));
    let below = path_text.strip_prefix(&local_text)?.trim_start_matches('\\').to_string();
    if below.is_empty() || below.starts_with("packages\\") || below == "packages" {
        return None;
    }
    let marker = format!(".where-{}", std::process::id());
    std::fs::write(path.join(&marker), b"").ok()?;
    let mut found = None;
    if let Ok(packages) = std::fs::read_dir(local.join("Packages")) {
        for package in packages.flatten() {
            let candidate = package.path().join("LocalCache").join("Local").join(&below);
            if candidate.join(&marker).exists() {
                found = Some(candidate);
                break;
            }
        }
    }
    // Removed where it was written; both names lead to the same file here.
    let _ = std::fs::remove_file(path.join(&marker));
    found
}

/// The folder a path really is; the path as given when nothing redirects it.
fn true_location(path: &Path) -> PathBuf {
    let _ = std::fs::create_dir_all(path);
    if let Some(private) = redirected(path) {
        eprintln!(
            "note: this process runs inside a packaged program; its data folder is that package's private copy: {}",
            private.display()
        );
        return private;
    }
    path.to_path_buf()
}

/// Whether this process sees a redirected `AppData\Local` (it was started from inside a
/// packaged program) and therefore works on a private copy of the data folder, not on
/// the one the app uses when it is started from the Start menu or Explorer.
pub fn is_private_copy() -> bool {
    let dir = data_dir().to_string_lossy().to_lowercase();
    dir.contains("\\packages\\") && dir.contains("\\localcache\\local\\")
}

/// The app's own data folder, whatever `WA_DATA_DIR` says: its true location.
pub fn default_data_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(|dir| PathBuf::from(dir).join("whatsapp-gpui"))
            .unwrap_or_else(|| PathBuf::from("whatsapp-gpui-data"));
        true_location(&base)
    })
    .clone()
}

/// The data folder in use: `WA_DATA_DIR` when set (test copies), else the app's own.
pub fn data_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| match std::env::var_os("WA_DATA_DIR") {
        Some(dir) => true_location(&PathBuf::from(dir)),
        None => default_data_dir(),
    })
    .clone()
}

fn sub_dir(name: &str) -> PathBuf {
    let dir = data_dir().join(name);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// The linked-device session (keys, identity). Owned by the connection library.
pub fn session_db() -> PathBuf {
    data_dir().join("session.db")
}

/// Our own database of chats, messages and contacts.
pub fn chat_db() -> PathBuf {
    data_dir().join("chats.db")
}

/// Raw history-sync blobs, kept so the chat database can be rebuilt.
pub fn history_dir() -> PathBuf {
    sub_dir("history")
}

/// Downloaded and decrypted media, named by content hash.
pub fn media_dir() -> PathBuf {
    sub_dir("media")
}

/// Profile pictures, named by JID.
pub fn avatar_dir() -> PathBuf {
    sub_dir("avatars")
}
