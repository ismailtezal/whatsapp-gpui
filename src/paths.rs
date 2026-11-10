//! Where the app keeps its data: `%LOCALAPPDATA%\whatsapp-gpui`.

use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("WA_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("whatsapp-gpui"))
        })
        .unwrap_or_else(|| PathBuf::from("whatsapp-gpui-data"));
    let _ = std::fs::create_dir_all(&base);
    base
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
