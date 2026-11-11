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
    /// Something happened to a call.
    Call(CallUpdate),
    /// A channel found by its link: who it is, before deciding to follow.
    ChannelFound {
        link: String,
        /// The channel's address, for reading its posts before following.
        jid: String,
        name: String,
        description: String,
        followers: u64,
        verified: bool,
    },
    /// A group's invite link, asked for with `Command::GroupInvite`.
    InviteLink { chat: String, link: String },
    /// The people waiting to be let into a group.
    JoinRequests { chat: String, people: Vec<String> },
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

/// What happened to a call, for the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallUpdate {
    /// Someone is calling.
    Ringing { call_id: String, from: String, video: bool },
    /// We are calling someone and it is ringing there.
    Calling { peer: String },
    /// We answered; sound is being set up.
    Answered,
    /// Sound is flowing.
    Connected,
    Muted(bool),
    /// The call is over (or never happened), with a few words on why.
    Ended { reason: String },
}

/// A community and its groups, as last read from the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Community {
    pub jid: String,
    pub name: String,
    pub groups: Vec<CommunityGroup>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityGroup {
    pub jid: String,
    pub name: String,
    /// The announcements group every member is in.
    pub announcements: bool,
    pub members: u32,
}

impl Community {
    /// Where the list of communities is kept in the store's settings table.
    pub const KEY: &'static str = "communities";

    /// The form kept in the store: one line per community or group, tab-separated;
    /// a group's line starts with a tab.
    pub fn encode(list: &[Community]) -> String {
        let clean = |text: &str| text.replace(['\t', '\n'], " ");
        let mut out = String::new();
        for community in list {
            out.push_str(&format!("{}\t{}\n", community.jid, clean(&community.name)));
            for group in &community.groups {
                out.push_str(&format!(
                    "\t{}\t{}\t{}\t{}\n",
                    group.jid,
                    clean(&group.name),
                    group.announcements as u8,
                    group.members
                ));
            }
        }
        out
    }

    pub fn decode(text: &str) -> Vec<Community> {
        let mut list: Vec<Community> = Vec::new();
        for line in text.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            match parts.as_slice() {
                ["", jid, name, announcements, members] => {
                    if let Some(community) = list.last_mut() {
                        community.groups.push(CommunityGroup {
                            jid: jid.to_string(),
                            name: name.to_string(),
                            announcements: *announcements == "1",
                            members: members.parse().unwrap_or(0),
                        });
                    }
                }
                [jid, name] if !jid.is_empty() => list.push(Community {
                    jid: jid.to_string(),
                    name: name.to_string(),
                    groups: Vec::new(),
                }),
                _ => {}
            }
        }
        list
    }
}

/// What is known about a channel, as last read from the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelInfo {
    /// "owner", "admin", "subscriber" or "guest".
    pub role: String,
    pub followers: u64,
    pub muted: bool,
    pub verified: bool,
    pub invite: String,
}

impl ChannelInfo {
    pub fn can_post(&self) -> bool {
        self.role == "owner" || self.role == "admin"
    }

    /// The form kept in the store's settings table.
    pub fn encode(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.role, self.followers, self.muted as u8, self.verified as u8, self.invite
        )
    }

    pub fn decode(text: &str) -> Option<Self> {
        let mut parts = text.splitn(5, ' ');
        Some(ChannelInfo {
            role: parts.next()?.to_string(),
            followers: parts.next()?.parse().ok()?,
            muted: parts.next()? == "1",
            verified: parts.next()? == "1",
            invite: parts.next().unwrap_or_default().to_string(),
        })
    }

    pub fn key(chat: &str) -> String {
        format!("channel-info:{chat}")
    }
}

/// One of a group's switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSetting {
    /// Only admins may change the group's name, photo and description.
    Locked(bool),
    /// Only admins may send messages.
    Announce(bool),
    /// New members must be approved by an admin.
    Approval(bool),
    /// Members who are not admins may add people.
    MembersCanAdd(bool),
}

/// A group's switches and timer as last read from the server.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GroupSettings {
    pub locked: bool,
    pub announce: bool,
    pub approval: bool,
    pub members_can_add: bool,
    /// Disappearing-message timer in seconds; 0 is off.
    pub timer: u32,
}

impl GroupSettings {
    /// The form kept in the store's settings table.
    pub fn encode(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.locked as u8, self.announce as u8, self.approval as u8, self.members_can_add as u8, self.timer
        )
    }

    pub fn decode(text: &str) -> Option<Self> {
        let mut parts = text.split(' ');
        let mut flag = || Some(parts.next()? == "1");
        let (locked, announce, approval, members_can_add) = (flag()?, flag()?, flag()?, flag()?);
        Some(GroupSettings {
            locked,
            announce,
            approval,
            members_can_add,
            timer: parts.next()?.parse().ok()?,
        })
    }

    /// Where a group's settings are kept in the store's settings table.
    pub fn key(chat: &str) -> String {
        format!("group-settings:{chat}")
    }
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
        /// People mentioned in the caption.
        mentions: Vec<String>,
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
    /// Call someone (voice).
    StartCall { chat: String },
    /// Answer or decline a call that is ringing.
    AnswerCall { call_id: String },
    DeclineCall { call_id: String },
    /// End the call in progress.
    EndCall,
    MuteCall { muted: bool },
    /// Read the communities this account is in, with their groups.
    SyncCommunities,
    CreateCommunity { name: String },
    /// Start a new group inside a community.
    CreateSubgroup { parent: String, name: String },
    /// Put an existing group into a community, or take it out.
    LinkGroup { parent: String, group: String, linked: bool },
    /// Join a group of a community we are in.
    JoinSubgroup { parent: String, group: String },
    /// Close a community for everyone (its groups stay).
    DeactivateCommunity { parent: String },
    /// Read the list of channels this account follows.
    SyncChannels,
    /// Fetch a channel's newest posts, or the page before the oldest one held.
    LoadChannel { chat: String, older: bool },
    /// Look a channel up by its link or invite code, without following it.
    LookupChannel { link: String },
    /// Follow a channel from its link or invite code.
    FollowChannel { link: String },
    UnfollowChannel { chat: String },
    MuteChannel { chat: String, muted: bool },
    /// Start a channel of our own.
    CreateChannel { name: String, description: String },
    /// Change one of a group's switches (admins only).
    SetGroupSetting { chat: String, setting: GroupSetting },
    /// The disappearing-message timer of a chat or group, in seconds; 0 turns it off.
    SetTimer { chat: String, seconds: u32 },
    /// Ask for a group's invite link; `reset` makes a new one and cancels the old.
    GroupInvite { chat: String, reset: bool },
    /// Join a group from an invite link or code.
    JoinGroup { link: String },
    /// Set a group's photo from a picture file, or remove it.
    SetGroupPhoto {
        chat: String,
        path: Option<std::path::PathBuf>,
    },
    /// Ask who is waiting to be let into a group.
    JoinRequests { chat: String },
    /// Let someone into a group, or turn them down.
    AnswerJoinRequest {
        chat: String,
        person: String,
        approve: bool,
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
        /// People mentioned in the new text.
        mentions: Vec<String>,
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
