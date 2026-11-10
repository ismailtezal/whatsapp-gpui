//! What the connection tells the interface, and what the interface asks of the connection.

/// Where the connection to WhatsApp stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnState {
    Starting,
    /// Not linked yet: a QR code is being shown.
    Pairing,
    Connecting,
    Online,
    Offline(String),
    LoggedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Typing,
    Recording,
    Idle,
}

/// Something changed; the interface re-reads the store for the parts it shows.
#[derive(Clone, Debug)]
pub enum UiEvent {
    Connection(ConnState),
    Qr { code: String, valid_secs: u64 },
    PairCode(String),
    /// The chat list changed (order, previews, unread counts, names).
    ChatsChanged,
    /// Messages of one chat changed (new, edited, deleted, receipts, reactions).
    ChatChanged { chat: String },
    /// A new incoming message worth a notification.
    Incoming {
        chat: String,
        id: String,
        sender: Option<String>,
        preview: String,
    },
    Activity {
        chat: String,
        sender: String,
        activity: Activity,
    },
    Presence {
        jid: String,
        online: bool,
        last_seen: Option<i64>,
    },
    HistoryProgress { progress: Option<u32> },
    /// A message's media finished downloading (or failed).
    MediaReady {
        chat: String,
        id: String,
        path: Option<String>,
        error: Option<String>,
    },
    AvatarReady { jid: String },
    IncomingCall {
        from: String,
        call_id: String,
        video: bool,
    },
    Identity {
        pn: Option<String>,
        lid: Option<String>,
        push_name: String,
    },
    Error(String),
}

/// What kind of file is being sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attach {
    Image,
    Video,
    Audio,
    Voice,
    Document,
    Sticker,
}

/// A request from the interface to the connection.
#[derive(Clone, Debug)]
pub enum Command {
    SendText {
        chat: String,
        text: String,
        /// ID of the message being replied to.
        reply_to: Option<String>,
        mentions: Vec<String>,
    },
    SendFile {
        chat: String,
        path: std::path::PathBuf,
        kind: Attach,
        caption: Option<String>,
        reply_to: Option<String>,
    },
    React {
        chat: String,
        id: String,
        emoji: String,
    },
    Edit {
        chat: String,
        id: String,
        text: String,
    },
    /// Delete for everyone.
    Revoke { chat: String, id: String },
    /// Delete only on this account's devices.
    DeleteForMe { chat: String, id: String },
    Star {
        chat: String,
        id: String,
        starred: bool,
    },
    Forward {
        from_chat: String,
        id: String,
        to_chats: Vec<String>,
    },
    MarkRead { chat: String },
    MarkUnread { chat: String },
    Typing { chat: String, activity: Activity },
    WatchPresence { jid: String },
    Archive { chat: String, archived: bool },
    Pin { chat: String, pinned: bool },
    Mute { chat: String, until: Option<i64> },
    DeleteChat { chat: String },
    ClearChat { chat: String },
    DownloadMedia { chat: String, id: String },
    FetchAvatar { jid: String },
    /// Ask the phone for older messages of a chat.
    LoadOlder { chat: String },
    SetPresence { online: bool },
    Logout,
    Shutdown,
}
