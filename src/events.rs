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
    /// Privacy settings as (category, value) and the blocked people.
    AccountInfo {
        privacy: Vec<(String, String)>,
        blocked: Vec<String>,
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
    /// Post a status update: text on a coloured background, or a photo or video with
    /// the text as its caption. `only_me` limits who receives it to this account's own
    /// devices (used by tests, so nobody else is sent anything).
    PostStatus {
        text: String,
        /// Background colour of a text status, as 0xAARRGGBB.
        background: u32,
        file: Option<std::path::PathBuf>,
        only_me: bool,
    },
    /// Answer someone's status update; the answer goes to their chat.
    ReplyStatus {
        sender: String,
        id: String,
        text: String,
    },
    /// Change the name other people see.
    SetProfileName { name: String },
    /// Change the About line; an empty text removes it.
    SetAbout { text: String },
    /// Set the profile photo from a picture file, or remove it.
    SetProfilePhoto { path: Option<std::path::PathBuf> },
    /// Change one privacy setting. Both are the names the account info reports.
    SetPrivacy { category: String, value: String },
    /// The disappearing-message timer new chats start with; 0 turns it off.
    SetDefaultTimer { seconds: u32 },
    /// Take down one of our own status updates.
    DeleteStatus { id: String },
    /// Tell the poster their status update was seen.
    SeenStatus { sender: String, id: String },
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
    /// Read a group's current subject, description and members from the server.
    RefreshGroup { chat: String },
    LeaveGroup { chat: String },
    /// Pin a message for everyone in the chat (for a week), or unpin it.
    PinMessage {
        chat: String,
        id: String,
        pinned: bool,
    },
    /// Read the privacy settings and the block list.
    FetchAccountInfo,
    Block { jid: String, blocked: bool },
    CreateGroup {
        subject: String,
        /// Phone-number JIDs of the people to add.
        members: Vec<String>,
    },
    SetSubject { chat: String, subject: String },
    SetDescription { chat: String, description: String },
    ChangeMembers {
        chat: String,
        members: Vec<String>,
        change: MemberChange,
    },
    VotePoll {
        chat: String,
        id: String,
        /// Names of the options picked; empty retracts the vote.
        options: Vec<String>,
    },
    CreatePoll {
        chat: String,
        question: String,
        options: Vec<String>,
        /// Whether a person may pick more than one option.
        multiple: bool,
    },
    Logout,
    Shutdown,
}

/// A change to who is in a group or who administers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberChange {
    Add,
    Remove,
    Promote,
    Demote,
}
