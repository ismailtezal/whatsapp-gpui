//! Plain data the interface works with, independent of the protocol types.

/// What a message is, for rendering and for the chat-list preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum MsgKind {
    Text = 0,
    Image = 1,
    Video = 2,
    Gif = 3,
    Audio = 4,
    Voice = 5,
    Document = 6,
    Sticker = 7,
    Location = 8,
    LiveLocation = 9,
    Contact = 10,
    Contacts = 11,
    Poll = 12,
    Event = 13,
    GroupInvite = 14,
    Call = 15,
    /// Group and chat notices ("X added Y", "Messages are end-to-end encrypted").
    System = 16,
    Revoked = 17,
    VideoNote = 18,
    Album = 19,
    Interactive = 20,
    Product = 21,
    Order = 22,
    Payment = 23,
    Template = 24,
    PollResult = 25,
    NewsletterInvite = 26,
    StickerPack = 27,
    /// Content this build cannot show yet; kept in full so a later build can.
    Unsupported = 99,
}

impl MsgKind {
    pub fn from_i32(value: i32) -> Self {
        use MsgKind::*;
        match value {
            0 => Text,
            1 => Image,
            2 => Video,
            3 => Gif,
            4 => Audio,
            5 => Voice,
            6 => Document,
            7 => Sticker,
            8 => Location,
            9 => LiveLocation,
            10 => Contact,
            11 => Contacts,
            12 => Poll,
            13 => Event,
            14 => GroupInvite,
            15 => Call,
            16 => System,
            17 => Revoked,
            18 => VideoNote,
            19 => Album,
            20 => Interactive,
            21 => Product,
            22 => Order,
            23 => Payment,
            24 => Template,
            25 => PollResult,
            26 => NewsletterInvite,
            27 => StickerPack,
            _ => Unsupported,
        }
    }

    pub fn is_media(self) -> bool {
        use MsgKind::*;
        matches!(
            self,
            Image | Video | Gif | Audio | Voice | Document | Sticker | VideoNote
        )
    }
}

/// What kind of conversation a JID is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ChatKind {
    User = 0,
    Group = 1,
    Broadcast = 2,
    Newsletter = 3,
    Status = 4,
    Community = 5,
}

impl ChatKind {
    pub fn from_i32(value: i32) -> Self {
        match value {
            1 => ChatKind::Group,
            2 => ChatKind::Broadcast,
            3 => ChatKind::Newsletter,
            4 => ChatKind::Status,
            5 => ChatKind::Community,
            _ => ChatKind::User,
        }
    }

    pub fn of_jid(jid: &str) -> Self {
        if jid == "status@broadcast" {
            ChatKind::Status
        } else if jid.ends_with("@g.us") {
            ChatKind::Group
        } else if jid.ends_with("@newsletter") {
            ChatKind::Newsletter
        } else if jid.ends_with("@broadcast") {
            ChatKind::Broadcast
        } else {
            ChatKind::User
        }
    }
}

/// Delivery state of a message we sent, in the order the ticks progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum Ack {
    Error = 0,
    Pending = 1,
    Sent = 2,
    Delivered = 3,
    Read = 4,
    Played = 5,
}

impl Ack {
    pub fn from_i32(value: i32) -> Self {
        match value {
            0 => Ack::Error,
            2 => Ack::Sent,
            3 => Ack::Delivered,
            4 => Ack::Read,
            5 => Ack::Played,
            _ => Ack::Pending,
        }
    }
}

/// One row of the chat list.
#[derive(Clone, Debug)]
pub struct ChatRow {
    pub jid: String,
    pub kind: ChatKind,
    /// Best display name: saved contact name, then group subject, then push name, then number.
    pub title: String,
    pub last_ts: i64,
    pub unread: i64,
    pub marked_unread: bool,
    pub archived: bool,
    pub pinned: i64,
    pub mute_until: i64,
    pub preview: Option<Preview>,
    pub draft: Option<String>,
}

/// The last message of a chat, as the chat list shows it.
#[derive(Clone, Debug)]
pub struct Preview {
    pub kind: MsgKind,
    pub text: Option<String>,
    pub from_me: bool,
    pub sender_name: Option<String>,
    pub ack: Ack,
}

/// One message, with the columns the message list needs without decoding the protocol blob.
#[derive(Clone, Debug)]
pub struct MsgRow {
    pub chat: String,
    pub id: String,
    pub from_me: bool,
    pub sender: Option<String>,
    pub ts: i64,
    pub kind: MsgKind,
    pub text: Option<String>,
    pub ack: Ack,
    pub starred: bool,
    pub quoted_id: Option<String>,
    pub edited: bool,
    pub forwarded: bool,
    pub media_path: Option<String>,
    /// The full `WebMessageInfo`, for everything the columns above do not carry.
    pub proto: Vec<u8>,
    pub rowid: i64,
}

#[derive(Clone, Debug, Default)]
pub struct ContactRow {
    pub jid: String,
    pub lid: Option<String>,
    pub name: Option<String>,
    pub first_name: Option<String>,
    pub push_name: Option<String>,
    pub verified_name: Option<String>,
    pub username: Option<String>,
    pub about: Option<String>,
}

/// Formats a phone-number JID the way a person reads a number: `+90 555 123 45 67` style
/// grouping is locale-specific, so this keeps it simple and unambiguous: `+905551234567`.
pub fn display_number(jid: &str) -> String {
    let user = jid.split('@').next().unwrap_or(jid);
    let user = user.split(':').next().unwrap_or(user);
    if jid.ends_with("@s.whatsapp.net") || jid.ends_with("@c.us") {
        format!("+{user}")
    } else {
        user.to_string()
    }
}

/// Whether a chat is muted right now. `mute_until` is 0 for not muted, -1 for muted
/// until further notice, else the moment the mute ends (seconds; imported history
/// gives milliseconds).
pub fn muted_now(mute_until: i64) -> bool {
    if mute_until < 0 {
        return true;
    }
    if mute_until == 0 {
        return false;
    }
    let ends = if mute_until > 100_000_000_000 { mute_until / 1000 } else { mute_until };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    ends > now
}
