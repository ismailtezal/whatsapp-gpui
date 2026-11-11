//! The window: title bar, rail, the column for the current section, and the floating
//! main panel.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, Image, ImageFormat, MouseButton, MouseDownEvent,
    Pixels, Point, SharedString, SystemNotification, Task, Window, WindowControlArea, anchored,
    deferred, div, point, px,
};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};

use super::chat::{ChatEvent, ChatView};
use super::theme::{COLUMN_W, HEADER_H, R_LG, R_MD, R_XL, RAIL_W, Surface, TITLE_BAR_H, Tokens, tokens};
use super::widgets::{avatar, count_badge, duration, icon, icon_button, list_time, tip};
use crate::backend::Backend;
use crate::content::preview_label;
use crate::events::{Command, ConnState, UiEvent};
use crate::model::{Ack, ChatKind, ChatRow, ContactRow, MsgKind, MsgRow, display_number};
use crate::store::Store;
use crate::store_more::CallRow;

/// One row of the column, by position in the list it comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RowKey {
    Label(&'static str),
    Chat(usize),
    Call(usize),
    Starred(usize),
    Hit(usize),
    Contact(usize),
    Status(usize),
    /// A community, and one of its groups: (community, group).
    Community(usize),
    CommunityGroup(usize, usize),
    /// "My status", shown when I have posted nothing in the last day.
    MyStatus,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Chats,
    Unread,
    Groups,
    Archived,
    Calls,
    Status,
    Channels,
    Communities,
    Starred,
    Settings,
}

impl Section {
    fn lists_chats(self) -> bool {
        matches!(
            self,
            Section::Chats | Section::Unread | Section::Groups | Section::Archived | Section::Channels
        )
    }

    fn title(self) -> &'static str {
        match self {
            Section::Chats => "Chats",
            Section::Unread => "Unread",
            Section::Groups => "Groups",
            Section::Archived => "Archived",
            Section::Calls => "Calls",
            Section::Status => "Status",
            Section::Channels => "Channels",
            Section::Communities => "Communities",
            Section::Starred => "Starred messages",
            Section::Settings => "Settings",
        }
    }
}

/// The call on screen: ringing, being placed, in progress, or just ended.
struct CallView {
    /// Set while an incoming call rings, to answer or decline it.
    call_id: Option<String>,
    peer: String,
    name: String,
    video: bool,
    phase: CallPhase,
    muted: bool,
    /// When sound started flowing, for the timer.
    connected_at: Option<std::time::Instant>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CallPhase {
    Ringing,
    Calling,
    Connecting,
    Connected,
    Ended(String),
}

/// A status update being written.
struct StatusDraft {
    /// Index into [`STATUS_COLORS`] for a text status.
    background: usize,
    /// A photo or video to post instead of text on a colour; the text becomes its caption.
    file: Option<std::path::PathBuf>,
}

/// Background colours offered for a text status, as 0xAARRGGBB.
const STATUS_COLORS: [u32; 8] = [
    0xFF1E6E4F, 0xFF2C5F8D, 0xFF6B4BB5, 0xFFB5484B, 0xFFC27B2B, 0xFF2B8A8A, 0xFF5B5F6B, 0xFF8D2C5F,
];

/// The colour of a text status as it is stored in the message, for painting.
fn argb_to_color(argb: u32) -> gpui::Hsla {
    gpui::rgba(((argb & 0x00FF_FFFF) << 8) | 0xFF).into()
}

/// A person's status updates, oldest first.
struct StatusGroup {
    sender: String,
    name: String,
    items: Vec<MsgRow>,
}

pub struct Shell {
    backend: Backend,
    store: Arc<Store>,
    conn: ConnState,
    me_name: String,
    me_jid: Option<String>,
    section: Section,
    chats: Vec<ChatRow>,
    selected: Option<String>,
    chat: Option<Entity<ChatView>>,
    search: Entity<InputState>,
    query: String,
    hits: Vec<MsgRow>,
    /// Choosing a contact to start a chat with.
    picking: bool,
    contacts: Vec<ContactRow>,
    calls: Vec<CallRow>,
    starred: Vec<MsgRow>,
    statuses: Vec<StatusGroup>,
    /// Open status: (person index, item index).
    status_open: Option<(usize, usize)>,
    chat_menu: Option<(String, Point<Pixels>)>,
    qr: Option<String>,
    last_error: Option<String>,
    confirm_logout: bool,
    /// The window backdrop last applied, so it is only set when the style changes.
    backdrop: Option<gpui::WindowBackgroundAppearance>,
    /// Set by events; the lists are re-read once after a batch of events.
    stale_chats: bool,
    stale_open_chat: bool,
    /// The name field must be refilled from the account (done at the next render, which
    /// has the window an input needs).
    profile_name_stale: bool,
    /// Whether new messages raise a Windows notification (a setting, on by default).
    notify_on: bool,
    /// Whether the app is set to start with Windows (read once at start).
    autostart: bool,
    fake_call_shown: bool,
    /// The call card, while there is a call to show.
    call: Option<CallView>,
    /// Communities as last read from the server, and the create panel.
    communities: Vec<crate::events::Community>,
    community_panel: bool,
    community_name: Entity<InputState>,
    /// Channels: the find-or-create panel, its fields, and the channel a link led to.
    channel_panel: bool,
    channel_link: Entity<InputState>,
    channel_name: Entity<InputState>,
    channel_found: Option<(String, String, String, u64, bool)>,
    /// New chat panel: an invite link to join a group with.
    join_link: Entity<InputState>,
    /// Settings: the name and About fields.
    profile_name: Entity<InputState>,
    profile_about: Entity<InputState>,
    /// The status being written, while the composer is open.
    status_draft: Option<StatusDraft>,
    status_text: Entity<TextareaState>,
    status_reply: Entity<InputState>,
    /// Status updates whose "seen" was already sent in this run.
    status_seen: std::collections::HashSet<String>,
    /// Whether the list of people who saw my status is unfolded.
    status_viewers: bool,
    /// Who saw the status on screen, read once per status rather than on every frame.
    status_viewer_cache: std::cell::RefCell<Option<(String, Vec<(String, i64)>)>>,
    /// Names already looked up for rows (calls, search hits); cleared when chats change.
    titles: std::cell::RefCell<std::collections::HashMap<String, String>>,
    /// Names for the `@number` texts in previews, looked up once each (None: unknown).
    mention_names: std::cell::RefCell<std::collections::HashMap<String, Option<String>>>,
    privacy: Vec<(String, String)>,
    blocked: Vec<String>,
    /// Picking several contacts for a new group instead of one to chat with.
    group_mode: bool,
    group_picks: Vec<String>,
    group_name: Entity<InputState>,
    window_active: bool,
    _pump: Task<()>,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Shell {
    pub fn new(backend: Backend, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = backend.store().clone();
        let notify_on = store.kv("notifications").ok().flatten().as_deref() != Some("off");
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let group_name = cx.new(|cx| InputState::new(window, cx).placeholder("Group name"));
        let status_text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                .placeholder("Type a status")
        });
        let status_reply = cx.new(|cx| InputState::new(window, cx).placeholder("Reply"));
        let join_link = cx.new(|cx| InputState::new(window, cx).placeholder("Paste a group invite link"));
        let community_name = cx.new(|cx| InputState::new(window, cx).placeholder("Name of a new community or group"));
        let channel_link = cx.new(|cx| InputState::new(window, cx).placeholder("Paste a channel link"));
        let channel_name = cx.new(|cx| InputState::new(window, cx).placeholder("Name of a new channel"));
        let profile_name = cx.new(|cx| InputState::new(window, cx).placeholder("Your name"));
        let profile_about = cx.new(|cx| InputState::new(window, cx).placeholder("About (leave empty to remove it)"));
        cx.subscribe_in(&status_reply, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.send_status_reply(window, cx);
            }
        })
        .detach();
        cx.subscribe_in(&search, window, |this, state, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = state.read(cx).value().to_string();
                this.run_search();
                cx.notify();
            }
        })
        .detach();
        cx.observe_window_activation(window, |this, window, _cx| {
            this.window_active = window.is_window_active();
            // Show as online only while the window is in front, like the official client.
            this.backend.send(Command::SetPresence {
                online: this.window_active,
            });
            if this.window_active
                && let Some(chat) = this.selected.clone()
            {
                this.backend.send(Command::MarkRead { chat });
            }
        })
        .detach();

        // Clicking a notification brings the window forward on that chat.
        let handle = window.window_handle();
        let shell = cx.entity().downgrade();
        cx.on_system_notification_response(move |response, cx| {
            let chat = response.tag.to_string();
            let shell = shell.clone();
            let _ = handle.update(cx, |_, window, cx| {
                // The window may be hidden behind the tray icon.
                crate::tray::show_main_window();
                window.activate_window();
                let _ = shell.update(cx, |this, cx| this.open_chat(&chat, window, cx));
            });
        });

        let events = backend.events();
        let pump = cx.spawn(async move |this, cx| {
            while let Ok(first) = events.recv().await {
                // Take everything that is already waiting: a burst of events (history
                // arriving, a busy group) is applied in one go and the lists are
                // re-read once, not once per event.
                let mut batch = vec![first];
                while batch.len() < 1024 {
                    match events.try_recv() {
                        Ok(event) => batch.push(event),
                        Err(_) => break,
                    }
                }
                let alive = this
                    .update(cx, |this, cx| {
                        for event in batch {
                            this.on_event(event, cx);
                        }
                        this.flush_reloads(cx);
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        });

        let mut shell = Shell {
            backend,
            store,
            conn: ConnState::Starting,
            me_name: "You".to_string(),
            me_jid: None,
            section: Section::Chats,
            chats: Vec::new(),
            selected: None,
            chat: None,
            search,
            query: String::new(),
            hits: Vec::new(),
            picking: false,
            contacts: Vec::new(),
            calls: Vec::new(),
            starred: Vec::new(),
            statuses: Vec::new(),
            status_open: None,
            chat_menu: None,
            qr: None,
            last_error: None,
            confirm_logout: false,
            backdrop: None,
            stale_chats: false,
            stale_open_chat: false,
            fake_call_shown: false,
            call: None,
            communities: Vec::new(),
            community_panel: false,
            community_name,
            channel_panel: false,
            channel_link,
            channel_name,
            channel_found: None,
            notify_on,
            autostart: crate::tray::starts_with_windows(),
            join_link,
            profile_name_stale: true,
            profile_name,
            profile_about,
            status_draft: None,
            status_text,
            status_reply,
            status_seen: Default::default(),
            status_viewers: false,
            status_viewer_cache: Default::default(),
            titles: Default::default(),
            mention_names: Default::default(),
            privacy: Vec::new(),
            blocked: Vec::new(),
            group_mode: false,
            group_picks: Vec::new(),
            group_name,
            window_active: true,
            _pump: pump,
        };
        shell.reload();
        // Development aid: open a chat by (part of) its title straight away.
        if let Ok(wanted) = std::env::var("WA_OPEN_CHAT") {
            let wanted = wanted.to_lowercase();
            let found = shell
                .chats
                .iter()
                .find(|c| !wanted.is_empty() && c.title.to_lowercase().contains(&wanted))
                .map(|c| c.jid.clone());
            if let Some(jid) = found {
                shell.open_chat(&jid, window, cx);
            }
        }
        // Development aid: start on a given section.
        if let Ok(section) = std::env::var("WA_SECTION") {
            shell.section = match section.as_str() {
                "calls" => Section::Calls,
                "status" => Section::Status,
                "starred" => Section::Starred,
                "settings" => Section::Settings,
                "archived" => Section::Archived,
                _ => Section::Chats,
            };
            shell.reload();
        }
        shell
    }

    /// Re-reads what the current section shows.
    fn reload(&mut self) {
        let _perf = crate::perf::scope("shell.reload");
        self.titles.borrow_mut().clear();
        self.reload_chats_and_badge();
    }

    /// Tells the tray icon and the taskbar badge how many messages are unread.
    fn update_badge(&self) {
        if let Some(tray) = crate::tray::current() {
            let unread: i64 = self
                .chats
                .iter()
                .filter(|chat| chat.mute_until == 0 && !chat.archived)
                .map(|chat| chat.unread.max(chat.marked_unread as i64))
                .sum();
            tray.set_unread(unread.clamp(0, u32::MAX as i64) as u32);
        }
    }

    fn reload_chats_and_badge(&mut self) {
        self.reload_lists();
        self.update_badge();
    }

    fn reload_lists(&mut self) {
        if self.section == Section::Communities {
            self.communities = self
                .store
                .kv(crate::events::Community::KEY)
                .ok()
                .flatten()
                .map(|text| crate::events::Community::decode(&text))
                .unwrap_or_default();
        }
        self.chats = self
            .store
            .chats(self.section == Section::Archived)
            .unwrap_or_default();
        match self.section {
            Section::Calls => self.calls = self.store.calls(300).unwrap_or_default(),
            Section::Starred => self.starred = self.store.starred(300).unwrap_or_default(),
            Section::Status => self.reload_statuses(),
            _ => {}
        }
        if self.picking {
            self.contacts = self.store.contacts().unwrap_or_default();
        }
    }

    fn reload_statuses(&mut self) {
        self.status_viewer_cache.borrow_mut().take();
        // Statuses live for 24 hours.
        let rows = self.store.status_updates(now_secs() - 24 * 3600).unwrap_or_default();
        let mut groups: Vec<StatusGroup> = Vec::new();
        for row in rows {
            // A status that was taken down is gone, not shown as "deleted".
            if row.kind == MsgKind::Revoked {
                continue;
            }
            let sender = if row.from_me {
                "me".to_string()
            } else {
                row.sender.clone().unwrap_or_default()
            };
            match groups.iter_mut().find(|g| g.sender == sender) {
                Some(group) => group.items.push(row),
                None => {
                    let name = if sender == "me" {
                        "My status".to_string()
                    } else {
                        self.store
                            .display_name(&sender)
                            .unwrap_or_else(|_| display_number(&sender))
                    };
                    groups.push(StatusGroup {
                        sender,
                        name,
                        items: vec![row],
                    });
                }
            }
        }
        // My own status first, then the most recent.
        groups.sort_by_key(|g| (g.sender != "me", std::cmp::Reverse(g.items.last().map(|r| r.ts).unwrap_or(0))));
        self.statuses = groups;
    }

    fn run_search(&mut self) {
        let query = self.query.trim();
        self.hits = if query.chars().count() >= 2 && self.section.lists_chats() && !self.picking {
            self.store.search_messages(query, None, 40).unwrap_or_default()
        } else {
            Vec::new()
        };
    }

    fn set_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if section == Section::Settings {
            self.backend.send(Command::FetchAccountInfo);
        }
        if section == Section::Communities {
            self.backend.send(Command::SyncCommunities);
        }
        if section == Section::Channels {
            // The list of followed channels lives on the server.
            self.backend.send(Command::SyncChannels);
        }
        self.section = section;
        self.picking = false;
        self.status_open = None;
        self.confirm_logout = false;
        self.reload();
        self.run_search();
        cx.notify();
    }

    fn on_event(&mut self, event: UiEvent, cx: &mut Context<Self>) {
        let _perf = crate::perf::scope("shell.event");
        match event {
            UiEvent::Connection(state) => {
                if state == ConnState::Online {
                    // Groups whose name never arrived get it fetched now.
                    for chat in self
                        .chats
                        .iter()
                        .filter(|chat| chat.kind == ChatKind::Group && chat.title == "Group")
                        .take(12)
                    {
                        self.backend.send(Command::RefreshGroup {
                            chat: chat.jid.clone(),
                        });
                    }
                    if self.section == Section::Settings {
                        self.backend.send(Command::FetchAccountInfo);
                    }
                    self.qr = None;
                }
                self.conn = state;
            }
            UiEvent::Qr { code, .. } => self.qr = Some(code),
            UiEvent::Identity { pn, push_name, .. } => {
                self.me_jid = pn;
                if !push_name.is_empty() {
                    self.me_name = push_name;
                    self.profile_name_stale = true;
                }
                if let Some(view) = &self.chat {
                    let (name, jid) = (self.me_name.clone(), self.me_jid.clone());
                    view.update(cx, |view, cx| view.set_me(name, jid, cx));
                }
            }
            UiEvent::ChatsChanged => self.stale_chats = true,
            UiEvent::HistoryProgress { .. } => {
                // History arrived (first sync or an answer to "get earlier messages").
                self.stale_chats = true;
                self.stale_open_chat = true;
            }
            UiEvent::ChatChanged { chat } => {
                if self.selected.as_deref() == Some(chat.as_str()) {
                    self.stale_open_chat = true;
                }
                if chat == "status@broadcast" && self.section == Section::Status {
                    self.reload_statuses();
                }
            }
            UiEvent::Incoming {
                chat,
                sender,
                preview,
                ..
            } => {
                let open_and_focused = self.window_active && self.selected.as_deref() == Some(chat.as_str());
                if open_and_focused {
                    // The chat is in front of the user: what arrives is read as it arrives.
                    self.backend.send(Command::MarkRead { chat: chat.clone() });
                }
                let row = self.chats.iter().find(|c| c.jid == chat);
                let muted = row.is_some_and(|c| c.mute_until != 0);
                if self.notify_on && !open_and_focused && !muted && chat != "status@broadcast" {
                    let title = row.map(|c| c.title.clone()).unwrap_or_else(|| display_number(&chat));
                    let body = match (row.map(|c| c.kind), sender) {
                        (Some(ChatKind::User), _) | (_, None) => preview,
                        (_, Some(sender)) => format!(
                            "{}: {preview}",
                            self.store.display_name(&sender).unwrap_or_default()
                        ),
                    };
                    cx.show_system_notification(SystemNotification {
                        tag: SharedString::from(chat.clone()),
                        title: title.into(),
                        body: body.into(),
                        actions: Vec::new(),
                    });
                }
            }
            UiEvent::Activity {
                chat,
                sender,
                activity,
            } => {
                if self.selected.as_deref() == Some(chat.as_str())
                    && let Some(view) = &self.chat
                {
                    view.update(cx, |view, cx| view.set_activity(sender, activity, cx));
                }
            }
            UiEvent::Presence {
                jid,
                online,
                last_seen,
            } => {
                if self.selected.as_deref() == Some(jid.as_str())
                    && let Some(view) = &self.chat
                {
                    view.update(cx, |view, cx| view.set_presence(online, last_seen, cx));
                }
            }
            UiEvent::ChannelFound {
                link,
                name,
                description,
                followers,
                verified,
                ..
            } => {
                self.channel_found = Some((link, name, description, followers, verified));
            }
            UiEvent::InviteLink { chat, link } => {
                if self.selected.as_deref() == Some(chat.as_str())
                    && let Some(view) = &self.chat
                {
                    view.update(cx, |view, cx| view.invite_link_arrived(link, cx));
                }
            }
            UiEvent::JoinRequests { chat, people } => {
                if self.selected.as_deref() == Some(chat.as_str())
                    && let Some(view) = &self.chat
                {
                    view.update(cx, |view, cx| view.join_requests_arrived(people, cx));
                }
            }
            UiEvent::Call(update) => self.on_call_update(update, cx),
            UiEvent::IncomingCall { from, video, .. } => {
                let name = self
                    .store
                    .display_name(&from)
                    .unwrap_or_else(|_| display_number(&from));
                cx.show_system_notification(SystemNotification {
                    tag: SharedString::from(format!("call-{from}")),
                    title: name.into(),
                    body: format!(
                        "Incoming {} call. Open WhatsApp to answer.",
                        if video { "video" } else { "voice" }
                    )
                    .into(),
                    actions: Vec::new(),
                });
            }
            UiEvent::AccountInfo { privacy, blocked } => {
                self.privacy = privacy;
                self.blocked = blocked;
            }
            UiEvent::AvatarReady { jid } => crate::avatars::refresh(&jid),
            UiEvent::MediaReady {
                chat,
                id,
                path,
                error,
            } => {
                if self.selected.as_deref() == Some(chat.as_str())
                    && let Some(view) = &self.chat
                {
                    view.update(cx, |view, cx| view.media_ready(&id, path, error, cx));
                }
                if chat == "status@broadcast" {
                    self.reload_statuses();
                }
            }
            UiEvent::Error(message) => self.last_error = Some(message),
            _ => {}
        }
        cx.notify();
    }

    /// Re-reads what events marked as stale. Called once after a batch of events.
    fn flush_reloads(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.stale_chats) {
            self.reload();
        }
        if std::mem::take(&mut self.stale_open_chat)
            && let Some(view) = &self.chat
        {
            view.update(cx, |view, cx| view.reload(cx));
        }
        cx.notify();
    }

    fn open_chat(&mut self, jid: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.picking = false;
        self.status_open = None;
        if self.section == Section::Settings || self.section == Section::Status {
            self.section = Section::Chats;
        }
        if self.selected.as_deref() == Some(jid) {
            cx.notify();
            return;
        }
        let known = self.chats.iter().find(|c| c.jid == jid).cloned();
        let Some(chat) = known.or_else(|| self.store.chat(jid).ok().flatten()) else {
            return;
        };
        // Keep what was typed but not sent in the chat being left.
        if let (Some(old), Some(view)) = (self.selected.clone(), &self.chat) {
            let draft = view.read(cx).draft(cx);
            let _ = self.store.batch(|tx| tx.set_draft(&old, Some(&draft)));
        }
        self.selected = Some(jid.to_string());
        let backend = self.backend.clone();
        let me_name = self.me_name.clone();
        let me_jid = self.me_jid.clone();
        let view = cx.new(|cx| ChatView::new(backend, chat, me_name, me_jid, window, cx));
        cx.subscribe_in(&view, window, |this, _, event: &ChatEvent, window, cx| match event {
            ChatEvent::OpenChat(jid) => {
                let jid = jid.clone();
                this.start_chat_with(&jid, window, cx);
            }
        })
        .detach();
        view.update(cx, |view, cx| view.focus_input(window, cx));
        self.chat = Some(view);
        self.reload();
        cx.notify();
    }

    fn start_chat_with(&mut self, jid: &str, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.store.ensure_chat(jid);
        self.section = Section::Chats;
        self.reload();
        self.open_chat(jid, window, cx);
    }

    /// The name shown for a chat or person. Rows are rebuilt on every frame they are on
    /// screen, so the answer is remembered rather than read from the database each time.
    /// The one-line preview of a message, with the people it mentions shown by name
    /// instead of by number.
    fn preview_of(&self, kind: MsgKind, text: Option<&str>) -> String {
        let label = preview_label(kind, text);
        crate::richtext::name_mentions(&label, |digits| {
            if let Some(known) = self.mention_names.borrow().get(digits) {
                return known.clone();
            }
            let name = self.store.mention_name(digits);
            self.mention_names.borrow_mut().insert(digits.to_string(), name.clone());
            name
        })
    }

    fn title_of(&self, jid: &str) -> String {
        if let Some(title) = self.titles.borrow().get(jid) {
            return title.clone();
        }
        let title = match self.store.chat(jid).ok().flatten() {
            Some(chat) => chat.title,
            None => self
                .store
                .display_name(jid)
                .unwrap_or_else(|_| display_number(jid)),
        };
        self.titles.borrow_mut().insert(jid.to_string(), title.clone());
        title
    }

    // ---- title bar -----------------------------------------------------------------

    fn title_bar(&self, window: &Window, cx: &App) -> impl IntoElement {
        let t = tokens(cx);
        let caption = |id: &'static str, name: &'static str, area: WindowControlArea, close: bool| {
            let hover_bg = if close {
                gpui::rgb(0xed4245).into()
            } else {
                t.fg(0.10)
            };
            let hover_fg = if close { gpui::white() } else { t.foreground };
            div()
                .id(id)
                .w(px(46.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.muted_foreground)
                .hover(move |style| style.bg(hover_bg).text_color(hover_fg))
                .window_control_area(area)
                .child(icon(name).size(px(13.)))
        };
        let status = match &self.conn {
            ConnState::Online => None,
            ConnState::Starting | ConnState::Connecting => Some("Connecting…".to_string()),
            ConnState::Pairing => Some("Waiting for you to link this device".to_string()),
            ConnState::Offline(_) => Some("Offline: reconnecting…".to_string()),
            ConnState::LoggedOut => Some("Logged out".to_string()),
        };
        div()
            .h(TITLE_BAR_H)
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .when(!t.liquid(), |this| this.bg(t.rail))
            .child(
                div()
                    .id("title-drag")
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .pl(px(12.))
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        div()
                            .size(px(16.))
                            .rounded(px(4.))
                            .bg(t.primary)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(t.primary_foreground)
                            .child(icon("message-square").size(px(10.))),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.muted_foreground)
                            .child("WhatsApp"),
                    )
                    .when_some(status, |this, status| {
                        this.child(
                            div()
                                .ml(px(8.))
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .text_size(px(11.))
                                .text_color(t.idle)
                                .child(icon("wifi-off").size(px(11.)))
                                .child(status),
                        )
                    }),
            )
            .child(caption("win-min", "minus", WindowControlArea::Min, false))
            .child(caption(
                "win-max",
                if window.is_maximized() { "minimize-2" } else { "square" },
                WindowControlArea::Max,
                false,
            ))
            .child(caption("win-close", "x", WindowControlArea::Close, true))
    }

    // ---- rail ------------------------------------------------------------------------

    fn rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx).clone();
        let unread_total: i64 = self
            .chats
            .iter()
            .filter(|c| c.mute_until == 0 && self.section != Section::Archived)
            .map(|c| c.unread)
            .sum();
        let chat_sections = [Section::Chats, Section::Unread, Section::Groups];
        let tile = |section: Section, name: &'static str, label: &'static str, cx: &mut Context<Self>| {
            let selected =
                self.section == section || (section == Section::Chats && chat_sections.contains(&self.section));
            let t = t.clone();
            let hover_ring = t.fg(0.15);
            div()
                .id(SharedString::from(format!("rail-{label}")))
                .size(px(40.))
                .rounded(R_XL)
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .border_2()
                .border_color(if selected { t.primary } else { gpui::transparent_black() })
                .when(selected, |this| {
                    this.bg(t.tint(0.12)).text_color(t.primary).shadow(t.glow())
                })
                .when(!selected, |this| {
                    this.bg(t.sidebar_accent)
                        .text_color(t.muted_foreground)
                        .hover(move |style| style.border_color(hover_ring))
                })
                .child(icon(name).size(px(18.)))
                .tooltip(tip(label))
                .on_click(cx.listener(move |this, _, _, cx| this.set_section(section, cx)))
        };
        div()
            .flex_shrink_0()
            .when(!t.liquid(), |this| this.w(RAIL_W).h_full().bg(t.rail))
            .when(t.liquid(), |this| {
                this.w(px(56.))
                    .ml(px(8.))
                    .mb(px(8.))
                    .rounded(px(22.))
                    .bg(t.surface(Surface::Bar, t.rail))
            })
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .pt(px(8.))
            .pb(px(12.))
            .child(
                div()
                    .relative()
                    .child(tile(Section::Chats, "messages-square", "Chats", cx))
                    .when(unread_total > 0, |this| {
                        this.child(
                            div()
                                .absolute()
                                .bottom(px(-4.))
                                .right(px(-6.))
                                .child(count_badge(unread_total, false, cx)),
                        )
                    }),
            )
            .child(tile(Section::Calls, "phone", "Calls", cx))
            .child(tile(Section::Status, "circle-dashed", "Status", cx))
            .child(tile(Section::Channels, "megaphone", "Channels", cx))
            .child(tile(Section::Communities, "users", "Communities", cx))
            .child(div().w(px(28.)).h(px(1.)).bg(t.sidebar_border))
            .child(tile(Section::Starred, "star", "Starred messages", cx))
            .child(tile(Section::Archived, "archive", "Archived", cx))
            .child(div().flex_1())
            .child(tile(Section::Settings, "settings", "Settings", cx))
            .child(
                div()
                    .id("rail-profile")
                    .cursor_pointer()
                    .child(avatar(
                        "me",
                        &self.me_name,
                        36.,
                        self.me_jid.as_deref().and_then(crate::avatars::get),
                        cx,
                    ))
                    .tooltip(tip("Profile"))
                    .on_click(cx.listener(|this, _, _, cx| this.set_section(Section::Settings, cx))),
            )
    }

    // ---- column rows -------------------------------------------------------------------

    /// The two-line row every list in the column uses.
    #[allow(clippy::too_many_arguments)]
    fn list_row(
        &self,
        id: SharedString,
        seed: &str,
        title: &str,
        picture_jid: Option<&str>,
        time: String,
        detail: AnyElement,
        trailing: Option<AnyElement>,
        selected: bool,
        bold: bool,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let t = tokens(cx);
        let hover = Tokens::with_alpha(t.sidebar_accent, 0.45);
        div()
            .id(id)
            .h(px(60.))
            .mx(px(8.))
            .px(px(8.))
            .rounded(R_LG)
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(12.))
            .cursor_pointer()
            .when(selected, |this| this.bg(t.fg(0.09)))
            .when(!selected, |this| this.hover(move |style| style.bg(hover)))
            .child(avatar(seed, title, 40., picture_jid.and_then(crate::avatars::get), cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(14.))
                                    .font_weight(if bold || selected {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .text_color(t.sidebar_foreground)
                                    .child(SharedString::from(title.to_string())),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(11.))
                                    .text_color(if bold { t.primary } else { t.muted_foreground })
                                    .child(time),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(12.))
                                    .text_color(if bold {
                                        Tokens::with_alpha(t.sidebar_foreground, 0.85)
                                    } else {
                                        t.muted_foreground
                                    })
                                    .child(detail),
                            )
                            .children(trailing),
                    ),
            )
    }

    fn chat_row(&self, chat: &ChatRow, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        let selected = self.selected.as_deref() == Some(chat.jid.as_str());
        let unread = chat.unread > 0 || chat.marked_unread;
        let muted = chat.mute_until != 0;

        let detail: AnyElement = if let Some(draft) = chat.draft.as_deref().filter(|d| !d.is_empty()) {
            div()
                .flex()
                .gap(px(4.))
                .child(div().text_color(t.destructive).child("Draft:"))
                .child(div().truncate().child(draft.to_string()))
                .into_any_element()
        } else if let Some(preview) = &chat.preview {
            let text = self.preview_of(preview.kind, preview.text.as_deref());
            let prefix = if preview.from_me || chat.kind == ChatKind::User {
                None
            } else {
                preview.sender_name.clone().map(|name| format!("{name}:"))
            };
            let tick = preview.from_me.then(|| {
                let (name, color) = match preview.ack {
                    Ack::Error => ("circle-alert", t.destructive),
                    Ack::Pending => ("clock", t.muted_foreground),
                    Ack::Sent => ("check", t.muted_foreground),
                    Ack::Delivered => ("check-check", t.muted_foreground),
                    Ack::Read | Ack::Played => ("check-check", t.link),
                };
                icon(name).size(px(13.)).text_color(color)
            });
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .min_w_0()
                .children(tick)
                .children(prefix.map(|p| div().flex_shrink_0().child(p)))
                .child(div().truncate().child(SharedString::from(text)))
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let mut trailing = div().flex().items_center().gap(px(6.));
        if muted {
            trailing = trailing.child(icon("bell-off").size(px(12.)).text_color(t.muted_foreground));
        }
        if chat.pinned > 0 {
            trailing = trailing.child(icon("pin").size(px(12.)).text_color(t.muted_foreground));
        }
        if chat.unread > 0 {
            trailing = trailing.child(count_badge(chat.unread, muted, cx));
        } else if chat.marked_unread {
            trailing = trailing.child(div().size(px(10.)).rounded_full().bg(t.primary));
        }

        let jid = chat.jid.clone();
        let menu_jid = chat.jid.clone();
        self.list_row(
            SharedString::from(format!("chat-{}", chat.jid)),
            &chat.jid,
            &chat.title,
            Some(&chat.jid),
            list_time(chat.last_ts),
            detail,
            Some(trailing.into_any_element()),
            selected,
            unread && !muted,
            cx,
        )
        .on_click(cx.listener(move |this, _, window, cx| this.open_chat(&jid, window, cx)))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.chat_menu = Some((menu_jid.clone(), event.position));
                cx.notify();
            }),
        )
        .into_any_element()
    }

    fn message_hit_row(&self, slot: usize, row: &MsgRow, cx: &mut Context<Self>) -> AnyElement {
        let title = self.title_of(&row.chat);
        let who = if row.from_me { "You: " } else { "" };
        let detail = div()
            .truncate()
            .child(SharedString::from(format!(
                "{who}{}",
                self.preview_of(row.kind, row.text.as_deref())
            )))
            .into_any_element();
        let jid = row.chat.clone();
        self.list_row(
            SharedString::from(format!("hit-{slot}")),
            &row.chat,
            &title,
            Some(&row.chat),
            list_time(row.ts),
            detail,
            None,
            false,
            false,
            cx,
        )
        .on_click(cx.listener(move |this, _, window, cx| this.open_chat(&jid, window, cx)))
        .into_any_element()
    }

    fn call_row(&self, slot: usize, call: &CallRow, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        let title = self.title_of(&call.peer);
        let (glyph, color, word) = if call.missed() {
            ("phone-missed", t.destructive, "Missed")
        } else if call.outgoing {
            ("phone-outgoing", t.online, "Outgoing")
        } else {
            ("phone-incoming", t.online, "Incoming")
        };
        let mut text = word.to_string();
        if call.duration > 0 {
            text = format!("{text} · {}", duration(call.duration as u32));
        }
        let detail = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(icon(glyph).size(px(13.)).text_color(color))
            .child(text)
            .into_any_element();
        let trailing = icon(if call.video { "video" } else { "phone" })
            .size(px(14.))
            .text_color(t.muted_foreground)
            .into_any_element();
        let jid = call.peer.clone();
        self.list_row(
            SharedString::from(format!("call-{slot}")),
            &call.peer,
            &title,
            Some(&call.peer),
            list_time(call.ts),
            detail,
            Some(trailing),
            false,
            false,
            cx,
        )
        .on_click(cx.listener(move |this, _, window, cx| this.start_chat_with(&jid, window, cx)))
        .into_any_element()
    }

    /// What the column lists, as cheap keys. Only the rows on screen are built from
    /// them, so a long list costs nothing per frame.
    fn row_keys(&self) -> (Vec<RowKey>, &'static str) {
        if self.picking {
            let query = self.query.trim().to_lowercase();
            let keys = self
                .contacts
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    query.is_empty()
                        || c.name.as_deref().unwrap_or_default().to_lowercase().contains(&query)
                        || c.jid.contains(&query)
                })
                .map(|(index, _)| RowKey::Contact(index))
                .collect();
            return (keys, "No contacts match.");
        }
        match self.section {
            Section::Calls => ((0..self.calls.len()).map(RowKey::Call).collect(), "No calls yet."),
            Section::Starred => (
                (0..self.starred.len()).map(RowKey::Starred).collect(),
                "No starred messages. Right-click a message and choose Star.",
            ),
            Section::Status => {
                let mut keys = Vec::new();
                if !self.statuses.iter().any(|group| group.sender == "me") {
                    keys.push(RowKey::MyStatus);
                }
                keys.extend((0..self.statuses.len()).map(RowKey::Status));
                (keys, "")
            }
            Section::Communities => {
                let mut keys = Vec::new();
                for (index, community) in self.communities.iter().enumerate() {
                    keys.push(RowKey::Community(index));
                    keys.extend((0..community.groups.len()).map(|group| RowKey::CommunityGroup(index, group)));
                }
                (keys, "You are in no communities. Use + to start one.")
            }
            Section::Settings => (Vec::new(), ""),
            _ => {
                let mut keys = Vec::new();
                let query = self.query.trim().to_lowercase();
                let chats: Vec<usize> = self
                    .chats
                    .iter()
                    .enumerate()
                    .filter(|(_, chat)| match self.section {
                        Section::Unread => chat.unread > 0 || chat.marked_unread,
                        Section::Groups => matches!(chat.kind, ChatKind::Group | ChatKind::Community),
                        Section::Channels => chat.kind == ChatKind::Newsletter,
                        Section::Chats => chat.kind != ChatKind::Newsletter,
                        _ => true,
                    })
                    .filter(|(_, chat)| query.is_empty() || chat.title.to_lowercase().contains(&query))
                    .map(|(index, _)| index)
                    .collect();
                if !query.is_empty() && !chats.is_empty() {
                    keys.push(RowKey::Label("Chats"));
                }
                keys.extend(chats.into_iter().map(RowKey::Chat));
                if !self.hits.is_empty() {
                    keys.push(RowKey::Label("Messages"));
                    keys.extend((0..self.hits.len()).map(RowKey::Hit));
                }
                (keys, "No chats here.")
            }
        }
    }

    /// Builds one row of the column.
    fn render_row(&self, key: RowKey, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        let nothing = || div().into_any_element();
        match key {
            RowKey::Label(text) => div()
                .h_full()
                .px(px(16.))
                .pb(px(6.))
                .flex()
                .items_end()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(Tokens::with_alpha(t.sidebar_foreground, 0.6))
                .child(text)
                .into_any_element(),
            RowKey::Chat(index) => match self.chats.get(index) {
                Some(chat) => self.chat_row(chat, cx),
                None => nothing(),
            },
            RowKey::Call(index) => match self.calls.get(index) {
                Some(call) => self.call_row(index, call, cx),
                None => nothing(),
            },
            RowKey::Starred(index) => match self.starred.get(index) {
                Some(row) => self.message_hit_row(index, row, cx),
                None => nothing(),
            },
            RowKey::Hit(index) => match self.hits.get(index) {
                Some(row) => self.message_hit_row(index, row, cx),
                None => nothing(),
            },
            RowKey::Contact(index) => {
                let Some(contact) = self.contacts.get(index) else {
                    return nothing();
                };
                let name = contact.name.clone().unwrap_or_else(|| display_number(&contact.jid));
                let detail = div()
                    .truncate()
                    .child(contact.about.clone().unwrap_or_else(|| display_number(&contact.jid)))
                    .into_any_element();
                let jid = contact.jid.clone();
                let picked = self.group_picks.contains(&contact.jid);
                self.list_row(
                    SharedString::from(format!("contact-{index}")),
                    &contact.jid,
                    &name,
                    Some(&contact.jid),
                    String::new(),
                    detail,
                    picked.then(|| icon("check").text_color(t.primary).into_any_element()),
                    picked,
                    false,
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| this.pick_contact(&jid, window, cx)))
                .into_any_element()
            }
            RowKey::Community(index) => {
                let Some(community) = self.communities.get(index) else {
                    return nothing();
                };
                let count = community.groups.len();
                let detail = div()
                    .truncate()
                    .child(format!("Community · {count} group{}", if count == 1 { "" } else { "s" }))
                    .into_any_element();
                let parent = community.jid.clone();
                self.list_row(
                    SharedString::from(format!("community-{index}")),
                    &community.jid,
                    &community.name,
                    Some(&community.jid),
                    String::new(),
                    detail,
                    Some(
                        icon_button(("community-add", index), "plus", cx)
                            .tooltip(tip("New group in this community (type its name above)"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                let name = this.community_name.read(cx).value().trim().to_string();
                                if name.is_empty() {
                                    this.community_panel = true;
                                    this.last_error =
                                        Some("Type the new group's name in the box above, then press + again.".to_string());
                                } else {
                                    this.backend.send(Command::CreateSubgroup {
                                        parent: parent.clone(),
                                        name,
                                    });
                                    this.community_name.update(cx, |input, cx| input.set_value("", window, cx));
                                }
                                cx.notify();
                            }))
                            .into_any_element(),
                    ),
                    false,
                    false,
                    cx,
                )
                .into_any_element()
            }
            RowKey::CommunityGroup(index, slot) => {
                let Some((community, group)) = self
                    .communities
                    .get(index)
                    .and_then(|community| Some((community, community.groups.get(slot)?)))
                else {
                    return nothing();
                };
                let joined = self.chats.iter().any(|chat| chat.jid == group.jid);
                let what = match (group.announcements, joined) {
                    (true, _) => "Announcements".to_string(),
                    (false, true) => format!("{} members", group.members),
                    (false, false) => format!("{} members · click to join", group.members),
                };
                let detail = div().truncate().child(what).into_any_element();
                let (parent, target) = (community.jid.clone(), group.jid.clone());
                div()
                    .pl(px(20.))
                    .child(
                        self.list_row(
                            SharedString::from(format!("community-{index}-{slot}")),
                            &group.jid,
                            &group.name,
                            Some(&group.jid),
                            String::new(),
                            detail,
                            None,
                            self.selected.as_deref() == Some(group.jid.as_str()),
                            false,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if joined {
                                this.open_chat(&target, window, cx);
                            } else {
                                this.backend.send(Command::JoinSubgroup {
                                    parent: parent.clone(),
                                    group: target.clone(),
                                });
                                cx.notify();
                            }
                        })),
                    )
                    .into_any_element()
            }
            RowKey::MyStatus => {
                let detail = div().truncate().child("Add a status update").into_any_element();
                self.list_row(
                    SharedString::from("status-mine"),
                    "me",
                    "My status",
                    self.me_jid.as_deref(),
                    String::new(),
                    detail,
                    Some(icon("plus").text_color(t.primary).into_any_element()),
                    self.status_draft.is_some(),
                    false,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.open_status_composer(cx)))
                .into_any_element()
            }
            RowKey::Status(index) => {
                let Some(group) = self.statuses.get(index) else {
                    return nothing();
                };
                let count = group.items.len();
                let detail = div()
                    .truncate()
                    .child(format!("{count} update{}", if count == 1 { "" } else { "s" }))
                    .into_any_element();
                let picture = if group.sender == "me" {
                    self.me_jid.as_deref()
                } else {
                    Some(group.sender.as_str())
                };
                self.list_row(
                    SharedString::from(format!("status-{index}")),
                    &group.sender,
                    &group.name,
                    picture,
                    group.items.last().map(|r| list_time(r.ts)).unwrap_or_default(),
                    detail,
                    None,
                    self.status_open.is_some_and(|(open, _)| open == index),
                    false,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.status_open = Some((index, 0));
                    this.status_draft = None;
                    this.status_viewers = false;
                    this.request_status_media();
                    cx.notify();
                }))
                .into_any_element()
            }
        }
    }

    fn column(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let _perf = crate::perf::scope("shell.column");
        let t = tokens(cx).clone();
        let filter = |section: Section, label: &'static str, cx: &mut Context<Self>| {
            let active = self.section == section;
            let t = t.clone();
            div()
                .id(SharedString::from(format!("filter-{label}")))
                .h(px(26.))
                .px(px(10.))
                .rounded_full()
                .flex()
                .items_center()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .when(active, |this| this.bg(t.tint(0.12)).text_color(t.primary))
                .when(!active, |this| {
                    this.bg(t.fg(0.06))
                        .text_color(t.muted_foreground)
                        .hover(|style| style.bg(t.fg(0.09)))
                })
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_section(section, cx)))
        };
        let show_filters =
            matches!(self.section, Section::Chats | Section::Unread | Section::Groups) && !self.picking;
        let show_search = self.section.lists_chats() || self.picking;
        let (keys, empty_text) = self.row_keys();
        let empty = keys.is_empty();
        let list_id = self.section as usize + if self.picking { 100 } else { 0 };
        let title = if self.picking {
            "New chat"
        } else {
            self.section.title()
        };

        div()
            .w(COLUMN_W)
            .flex_shrink_0()
            .when(!t.liquid(), |this| this.h_full())
            .when(t.liquid(), |this| {
                this.ml(px(8.))
                    .mb(px(8.))
                    .rounded(px(20.))
                    .overflow_hidden()
                    .bg(t.surface(Surface::Panel, t.sidebar))
            })
            .flex()
            .flex_col()
            .child(
                div()
                    .h(HEADER_H)
                    .mt(px(8.))
                    .px(px(16.))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .when(self.picking, |this| {
                        this.child(
                            icon_button("back", "arrow-left", cx)
                                .mr(px(4.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.picking = false;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.foreground)
                            .child(title),
                    )
                    .when(self.section == Section::Communities, |this| {
                        this.child(
                            icon_button("community-panel", "plus", cx)
                                .tooltip(tip("New community"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.community_panel = !this.community_panel;
                                    cx.notify();
                                })),
                        )
                    })
                    .when(self.section == Section::Channels, |this| {
                        this.child(
                            icon_button("channel-panel", "plus", cx)
                                .tooltip(tip("Find or create a channel"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.channel_panel = !this.channel_panel;
                                    this.channel_found = None;
                                    cx.notify();
                                })),
                        )
                    })
                    .when(self.section == Section::Status, |this| {
                        this.child(
                            icon_button("new-status", "plus", cx)
                                .tooltip(tip("New status"))
                                .on_click(cx.listener(|this, _, _, cx| this.open_status_composer(cx))),
                        )
                    })
                    .when(self.section.lists_chats() && !self.picking, |this| {
                        this.child(
                            icon_button("new-chat", "square-pen", cx)
                                .tooltip(tip("New chat"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.picking = true;
                                    this.reload();
                                    this.run_search();
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .when(show_search, |this| {
                this.child(
                    div().px(px(12.)).pb(px(8.)).flex_shrink_0().child(
                        Input::new(&self.search)
                            .prefix(icon("search").size(px(14.)).text_color(t.muted_foreground))
                            .cleanable(true),
                    ),
                )
            })
            .when(self.picking, |this| {
                let t = t.clone();
                let hover = t.fg(0.09);
                this.child(
                    div()
                        .px(px(12.))
                        .pb(px(8.))
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .gap(px(6.))
                        .when(self.group_mode, |this| {
                            this.child(div().flex_1().child(Input::new(&self.group_name)))
                                .child(
                                    div()
                                        .id("create-group")
                                        .h(px(32.))
                                        .px(px(12.))
                                        .rounded(R_LG)
                                        .flex()
                                        .flex_shrink_0()
                                        .items_center()
                                        .bg(t.primary)
                                        .text_color(t.primary_foreground)
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .cursor_pointer()
                                        .child(format!("Create ({})", self.group_picks.len()))
                                        .on_click(cx.listener(|this, _, _, cx| this.create_group(cx))),
                                )
                        })
                        .when(!self.group_mode, |this| {
                            this.child(
                                div()
                                    .id("new-group")
                                    .h(px(32.))
                                    .px(px(10.))
                                    .rounded(R_LG)
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .bg(t.fg(0.06))
                                    .text_size(px(13.))
                                    .cursor_pointer()
                                    .hover(move |style| style.bg(hover))
                                    .child(icon("users").size(px(14.)))
                                    .child("New group")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.group_mode = true;
                                        this.group_picks.clear();
                                        cx.notify();
                                    })),
                            )
                        }),
                )
            })
            .when(self.section == Section::Channels && self.channel_panel, |this| {
                this.child(self.channel_finder(cx))
            })
            .when(self.section == Section::Communities && self.community_panel, |this| {
                let t = t.clone();
                let hover = t.fg(0.09);
                this.child(
                    div()
                        .px(px(12.))
                        .pb(px(8.))
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .gap(px(6.))
                        .child(div().flex_1().child(Input::new(&self.community_name)))
                        .child(
                            div()
                                .id("community-create")
                                .h(px(32.))
                                .px(px(12.))
                                .rounded(R_LG)
                                .flex()
                                .flex_shrink_0()
                                .items_center()
                                .bg(t.fg(0.06))
                                .text_size(px(13.))
                                .cursor_pointer()
                                .hover(move |style| style.bg(hover))
                                .child("New community")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let name = this.community_name.read(cx).value().trim().to_string();
                                    if name.is_empty() {
                                        return;
                                    }
                                    this.backend.send(Command::CreateCommunity { name });
                                    this.community_name.update(cx, |input, cx| input.set_value("", window, cx));
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when(self.picking && !self.group_mode, |this| {
                let t = t.clone();
                let hover = t.fg(0.09);
                this.child(
                    div()
                        .px(px(12.))
                        .pb(px(8.))
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .gap(px(6.))
                        .child(div().flex_1().child(Input::new(&self.join_link)))
                        .child(
                            div()
                                .id("join-with-link")
                                .h(px(32.))
                                .px(px(12.))
                                .rounded(R_LG)
                                .flex()
                                .flex_shrink_0()
                                .items_center()
                                .bg(t.fg(0.06))
                                .text_size(px(13.))
                                .cursor_pointer()
                                .hover(move |style| style.bg(hover))
                                .child("Join")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let link = this.join_link.read(cx).value().trim().to_string();
                                    if link.is_empty() {
                                        return;
                                    }
                                    this.backend.send(Command::JoinGroup { link });
                                    this.join_link.update(cx, |input, cx| input.set_value("", window, cx));
                                    this.picking = false;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when(show_filters, |this| {
                this.child(
                    div()
                        .px(px(12.))
                        .pb(px(8.))
                        .flex()
                        .flex_shrink_0()
                        .gap(px(6.))
                        .child(filter(Section::Chats, "All", cx))
                        .child(filter(Section::Unread, "Unread", cx))
                        .child(filter(Section::Groups, "Groups", cx)),
                )
            })
            .when(empty, |this| {
                this.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .p(px(24.))
                        .text_size(px(13.))
                        .text_color(t.muted_foreground)
                        .child(empty_text),
                )
            })
            .when(!empty, |this| {
                // Every row is 60px tall plus a 2px gap, so only those on screen are built.
                this.child(
                    gpui::uniform_list(
                        ("column-list", list_id),
                        keys.len(),
                        cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                            range
                                .filter_map(|index| keys.get(index).copied())
                                .map(|key| div().h(px(62.)).child(this.render_row(key, cx)))
                                .collect::<Vec<_>>()
                        }),
                    )
                    .flex_1()
                    .min_h_0(),
                )
            })
    }

    // ---- chat row menu -----------------------------------------------------------------

    fn chat_menu_overlay(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (jid, at) = self.chat_menu.clone()?;
        let chat = self.chats.iter().find(|c| c.jid == jid)?.clone();
        let t = tokens(cx).clone();
        let item = |slot: usize, name: &'static str, label: &'static str, danger: bool| {
            let accent = t.accent;
            let danger_fill = Tokens::with_alpha(t.destructive, if t.dark { 0.2 } else { 0.1 });
            div()
                .id(("chat-menu-item", slot))
                .h(px(32.))
                .px(px(8.))
                .rounded(R_MD)
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(14.))
                .cursor_pointer()
                .text_color(if danger { t.destructive } else { t.foreground })
                .hover(move |style| style.bg(if danger { danger_fill } else { accent }))
                .child(icon(name).text_color(if danger { t.destructive } else { t.muted_foreground }))
                .child(label)
        };
        let unread = chat.unread > 0 || chat.marked_unread;
        let actions: Vec<(&'static str, &'static str, bool, Command)> = vec![
            (
                "mail-open",
                if unread { "Mark as read" } else { "Mark as unread" },
                false,
                if unread {
                    Command::MarkRead { chat: jid.clone() }
                } else {
                    Command::MarkUnread { chat: jid.clone() }
                },
            ),
            (
                "pin",
                if chat.pinned > 0 { "Unpin chat" } else { "Pin chat" },
                false,
                Command::Pin {
                    chat: jid.clone(),
                    pinned: chat.pinned == 0,
                },
            ),
            (
                "bell-off",
                if chat.mute_until != 0 { "Unmute" } else { "Mute notifications" },
                false,
                Command::Mute {
                    chat: jid.clone(),
                    until: if chat.mute_until != 0 { None } else { Some(0) },
                },
            ),
            (
                "archive",
                if chat.archived { "Unarchive chat" } else { "Archive chat" },
                false,
                Command::Archive {
                    chat: jid.clone(),
                    archived: !chat.archived,
                },
            ),
            ("trash", "Delete chat", true, Command::DeleteChat { chat: jid.clone() }),
        ];
        let mut panel = div()
            .w(px(224.))
            .p(px(6.))
            .rounded(R_XL)
            .border_1()
            .border_color(t.border)
            .bg(t.popover_fill())
            .shadow(t.shadow_popover())
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for (slot, (glyph, label, danger, command)) in actions.into_iter().enumerate() {
            if danger {
                panel = panel.child(div().h(px(1.)).mx(px(-6.)).my(px(6.)).bg(t.border));
            }
            panel = panel.child(item(slot, glyph, label, danger).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.backend.send(command.clone());
                    this.chat_menu = None;
                    cx.notify();
                },
            )));
        }

        let size = window.viewport_size();
        let close = cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.chat_menu = None;
            cx.notify();
        });
        let close_right = cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.chat_menu = None;
            cx.notify();
        });
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .w(size.width)
                        .h(size.height)
                        .occlude()
                        .on_mouse_down(MouseButton::Left, close)
                        .on_mouse_down(MouseButton::Right, close_right)
                        .child(anchored().position(at).snap_to_window_with_margin(px(8.)).child(panel)),
                ),
            )
            .with_priority(10)
            .into_any_element(),
        )
    }

    // ---- main panel --------------------------------------------------------------------

    fn request_status_media(&mut self) {
        let Some((person, item)) = self.status_open else {
            return;
        };
        // Looking at someone's status tells them it was seen, once.
        if let Some(group) = self.statuses.get(person)
            && group.sender != "me"
            && let Some(row) = group.items.get(item)
            && self.status_seen.insert(row.id.clone())
        {
            self.backend.send(Command::SeenStatus {
                sender: group.sender.clone(),
                id: row.id.clone(),
            });
        }
        if let Some(row) = self.statuses.get(person).and_then(|g| g.items.get(item))
            && row.kind.is_media()
            && row.media_path.is_none()
        {
            self.backend.send(Command::DownloadMedia {
                chat: "status@broadcast".to_string(),
                id: row.id.clone(),
            });
        }
    }

    fn step_status(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some((person, item)) = self.status_open else {
            return;
        };
        let count = self.statuses.get(person).map(|g| g.items.len()).unwrap_or(0) as isize;
        let next = item as isize + delta;
        if next < 0 {
            return;
        }
        if next >= count {
            // On to the next person, or close after the last.
            self.status_open = (person + 1 < self.statuses.len()).then_some((person + 1, 0));
        } else {
            self.status_open = Some((person, next as usize));
        }
        self.request_status_media();
        cx.notify();
    }

    fn status_panel(&self, person: usize, item: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        let Some(group) = self.statuses.get(person) else {
            return self.empty_panel(cx);
        };
        let Some(row) = group.items.get(item) else {
            return self.empty_panel(cx);
        };
        let mut bars = div().flex().gap(px(4.)).px(px(16.)).pt(px(12.));
        for index in 0..group.items.len() {
            bars = bars.child(
                div()
                    .flex_1()
                    .h(px(3.))
                    .rounded_full()
                    .bg(if index <= item { t.primary } else { t.fg(0.15) }),
            );
        }
        let body: AnyElement = match (row.kind, row.media_path.as_ref()) {
            (MsgKind::Image, Some(path)) => gpui::img(std::path::PathBuf::from(path))
                .max_w_full()
                .max_h_full()
                .object_fit(gpui::ObjectFit::Contain)
                .into_any_element(),
            (MsgKind::Text, _) => {
                // A text status sits on the colour its author picked.
                let background = super::message::record_of(row)
                    .message
                    .as_option()
                    .and_then(|message| message.extended_text_message.as_option())
                    .and_then(|text| text.background_argb)
                    .unwrap_or(STATUS_COLORS[0]);
                div()
                    .size_full()
                    .rounded(R_XL)
                    .bg(argb_to_color(background))
                    .p(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .max_w(px(560.))
                            .text_size(px(28.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gpui::white())
                            .child(SharedString::from(row.text.clone().unwrap_or_default())),
                    )
                    .into_any_element()
            }
            (kind, Some(path)) if kind.is_media() => {
                let path = std::path::PathBuf::from(path);
                div()
                    .id("status-open-media")
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .text_color(t.muted_foreground)
                    .child(icon("play").size(px(40.)))
                    .child("Open in the default player")
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_with_system(&path)))
                    .into_any_element()
            }
            (kind, None) if kind.is_media() => div()
                .text_color(t.muted_foreground)
                .child("Downloading…")
                .into_any_element(),
            _ => div()
                .text_color(t.muted_foreground)
                .child(preview_label(row.kind, row.text.as_deref()))
                .into_any_element(),
        };
        let caption = row
            .text
            .clone()
            .filter(|text| !text.is_empty() && row.kind != MsgKind::Text);
        let picture = if group.sender == "me" {
            self.me_jid.as_deref().and_then(crate::avatars::get)
        } else {
            crate::avatars::get(&group.sender)
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(bars)
            .child(
                div()
                    .px(px(16.))
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(avatar(&group.sender, &group.name, 32., picture, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from(group.name.clone())),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.muted_foreground)
                                    .child(super::widgets::header_time(row.ts)),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        icon_button("status-prev", "chevron-left", cx)
                            .on_click(cx.listener(|this, _, _, cx| this.step_status(-1, cx))),
                    )
                    .child(
                        icon_button("status-next", "chevron-right", cx)
                            .on_click(cx.listener(|this, _, _, cx| this.step_status(1, cx))),
                    )
                    .child(
                        icon_button("status-close", "x", cx).on_click(cx.listener(|this, _, _, cx| {
                            this.status_open = None;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(body),
            )
            .when_some(caption, |this, caption| {
                this.child(
                    div()
                        .px(px(24.))
                        .pb(px(12.))
                        .flex()
                        .justify_center()
                        .text_size(px(14.))
                        .text_color(t.fg(0.9))
                        .child(SharedString::from(caption)),
                )
            })
            .child(self.status_footer(group, row, cx))
            .into_any_element()
    }

    fn settings_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        let heading = |text: &'static str, detail: &'static str| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .mb(px(12.))
                .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(text))
                .child(div().text_size(px(14.)).text_color(t.muted_foreground).child(detail))
        };
        let rule = || div().h(px(1.)).my(px(28.)).bg(t.border);
        let dark = t.dark;
        let choice = |id: &'static str, label: &'static str, active: bool| {
            let rest = t.fg(0.08);
            let hover = t.fg(0.12);
            div()
                .id(id)
                .h(px(32.))
                .px(px(14.))
                .rounded(R_LG)
                .flex()
                .flex_shrink_0()
                .items_center()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .when(active, |this| this.bg(t.primary).text_color(t.primary_foreground))
                .when(!active, |this| {
                    this.bg(rest)
                        .text_color(t.foreground)
                        .hover(move |style| style.bg(hover))
                })
                .child(label)
        };
        let number = self.me_jid.as_deref().map(display_number).unwrap_or_default();
        let folder = crate::paths::data_dir();
        let folder_text = folder.display().to_string();
        let confirm = self.confirm_logout;

        div()
            .id("settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .max_w(px(672.))
                    .mx_auto()
                    .px(px(20.))
                    .pt(px(24.))
                    .pb(px(64.))
                    .flex()
                    .flex_col()
                    .child(heading("Profile", "How other people see you on WhatsApp."))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .child(avatar(
                                "me",
                                &self.me_name,
                                56.,
                                self.me_jid.as_deref().and_then(crate::avatars::get),
                                cx,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(SharedString::from(self.me_name.clone())),
                                    )
                                    .child(div().text_size(px(13.)).text_color(t.muted_foreground).child(number)),
                            ),
                    )
                    .child(self.profile_editor(cx))
                    .child(rule())
                    .child(heading("Appearance", "The look follows diskurt's modern design."))
                    .child(self.style_row(cx))
                    .children(self.backdrop_row(cx))
                    .child(self.palette_row(cx))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(choice("theme-dark", "Dark", dark).on_click(cx.listener(|_, _, _, cx| {
                                super::theme::install(true, cx);
                                cx.refresh_windows();
                            })))
                            .child(choice("theme-light", "Light", !dark).on_click(cx.listener(|_, _, _, cx| {
                                super::theme::install(false, cx);
                                cx.refresh_windows();
                            }))),
                    )
                    .child(rule())
                    .child(heading("Windows", "How the app behaves on this PC."))
                    .child(self.windows_rows(cx))
                    .child(rule())
                    .child(heading(
                        "Storage",
                        "Chats, the linked session and downloaded media are kept in this folder.",
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.))
                                    .text_color(t.muted_foreground)
                                    .child(SharedString::from(folder_text)),
                            )
                            .child(choice("open-folder", "Open folder", false).on_click(cx.listener(
                                move |_, _, _, cx| cx.open_with_system(&folder),
                            ))),
                    )
                    .child(rule())
                    .child(heading(
                        "Account",
                        "Logging out unlinks this device. Your chats stay on your phone.",
                    ))
                    .child(
                        div().flex().child(
                            div()
                                .id("logout")
                                .h(px(32.))
                                .px(px(14.))
                                .rounded(R_LG)
                                .flex()
                                .items_center()
                                .text_size(px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .bg(Tokens::with_alpha(t.destructive, if confirm { 1.0 } else { 0.14 }))
                                .text_color(if confirm { gpui::white() } else { t.destructive })
                                .child(if confirm { "Click again to log out" } else { "Log out" })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if this.confirm_logout {
                                        this.backend.send(Command::Logout);
                                        this.confirm_logout = false;
                                    } else {
                                        this.confirm_logout = true;
                                    }
                                    cx.notify();
                                })),
                        ),
                    )
                    .children(self.account_sections(cx))
                    .when_some(self.last_error.clone(), |this, error| {
                        this.child(rule())
                            .child(heading(
                                "Last problem",
                                "The most recent error reported by the connection.",
                            ))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(t.muted_foreground)
                                    .child(SharedString::from(error)),
                            )
                    }),
            )
            .into_any_element()
    }

    fn empty_panel(&self, cx: &App) -> AnyElement {
        let t = tokens(cx);
        let (title, detail): (&str, &str) = match (&self.conn, &self.qr) {
            (ConnState::Pairing, Some(_)) => (
                "Link this device",
                "On your phone: WhatsApp → Settings → Linked devices → Link a device, then scan this code.",
            ),
            (ConnState::LoggedOut, _) => (
                "Logged out",
                "This device was unlinked. Restart the app to link it again.",
            ),
            _ => (
                "WhatsApp for Windows",
                "Select a chat to start messaging. Your messages are end-to-end encrypted.",
            ),
        };
        // The pairing code as a picture: black modules on a white card.
        let qr = match (&self.conn, &self.qr) {
            (ConnState::Pairing, Some(code)) => qrcode::QrCode::new(code.as_bytes()).ok().map(|qr| {
                let svg = qr
                    .render::<qrcode::render::svg::Color>()
                    .min_dimensions(264, 264)
                    .dark_color(qrcode::render::svg::Color("#000000"))
                    .light_color(qrcode::render::svg::Color("#ffffff"))
                    .build();
                Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()))
            }),
            _ => None,
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(16.))
            .child(match qr {
                Some(image) => div()
                    .p(px(14.))
                    .rounded(R_XL)
                    .bg(gpui::white())
                    .child(gpui::img(image).size(px(264.)))
                    .into_any_element(),
                None => div()
                    .size(px(44.))
                    .rounded(R_XL)
                    .bg(t.tint(0.10))
                    .border_1()
                    .border_color(t.tint(0.15))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(t.primary)
                    .child(icon("messages-square").size(px(20.)))
                    .into_any_element(),
            })
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.foreground)
                    .child(title),
            )
            .child(
                div()
                    .max_w(px(384.))
                    .text_size(px(14.))
                    .text_color(t.muted_foreground)
                    .child(detail),
            )
            .into_any_element()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::frame();
        let _perf = crate::perf::scope("shell.render");
        // Development aid: WA_FAKE_CALL=ringing, calling or connected shows the call card
        // without any call, for screenshots.
        if !self.fake_call_shown
            && let Ok(kind) = std::env::var("WA_FAKE_CALL")
        {
            use crate::events::CallUpdate;
            self.fake_call_shown = true;
            let peer = self.chats.first().map(|chat| chat.jid.clone()).unwrap_or_default();
            match kind.as_str() {
                "ringing" => self.on_call_update(
                    CallUpdate::Ringing {
                        call_id: "fake".to_string(),
                        from: peer,
                        video: false,
                    },
                    cx,
                ),
                other => {
                    self.on_call_update(CallUpdate::Calling { peer }, cx);
                    if other == "connected" {
                        self.on_call_update(CallUpdate::Connected, cx);
                    }
                }
            }
        }
        if std::mem::take(&mut self.profile_name_stale) {
            let name = self.me_name.clone();
            self.profile_name.update(cx, |input, cx| input.set_value(name, window, cx));
        }
        let t = tokens(cx).clone();
        let panel: AnyElement = match (self.section, self.status_open, &self.chat) {
            (Section::Settings, _, _) => self.settings_panel(cx),
            (Section::Status, _, _) if self.status_draft.is_some() => self.status_compose_panel(cx),
            (Section::Status, Some((person, item)), _) => self.status_panel(person, item, cx),
            (_, _, Some(chat)) => chat.clone().into_any_element(),
            _ => self.empty_panel(cx),
        };
        let chat_menu = self.chat_menu_overlay(window, cx);
        // Glass shows a blurred view of what is behind the window; the others are opaque.
        let backdrop = t.window_backdrop();
        if self.backdrop != Some(backdrop) {
            window.set_background_appearance(backdrop);
            self.backdrop = Some(backdrop);
        }
        // The liquid style paints soft colours behind everything, for the glass to bend.
        let mut ambient = div().absolute().inset_0().overflow_hidden();
        let viewport = window.viewport_size();
        for (x, y, size, color) in t.ambient() {
            ambient = ambient.child(
                div()
                    .absolute()
                    .left(viewport.width * x - px(size / 2.))
                    .top(viewport.height * y - px(size / 2.))
                    .size(px(size))
                    .rounded_full()
                    // Only the shadow is drawn: a wide, soft disc of colour.
                    .shadow(vec![gpui::BoxShadow {
                        color,
                        offset: gpui::point(px(0.), px(0.)),
                        blur_radius: px(size * 0.5),
                        spread_radius: px(0.),
                        inset: false,
                    }]),
            );
        }
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(t.rail)
            .text_color(t.foreground)
            .font_family(super::theme::FONT_UI)
            .text_size(px(14.))
            .when(t.liquid(), |this| this.child(ambient))
            .child(self.title_bar(window, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.rail(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .bg(t.sidebar)
                            .child(self.column(cx))
                            .child(
                                div().flex_1().min_w_0().p(px(8.)).when(t.liquid(), |this| this.pt(px(0.))).child(
                                    div()
                                        .size_full()
                                        .rounded(if t.liquid() { px(20.) } else { R_XL })
                                        .border_1()
                                        .border_color(t.panel_border())
                                        .bg(t.surface(Surface::Panel, t.background))
                                        .shadow(t.panel_shadow())
                                        .overflow_hidden()
                                        .child(panel),
                                ),
                            ),
                    ),
            )
            .children(chat_menu)
            .children(self.call_card(cx))
            // The marker of a middle-button auto-scroll, where the button was pressed.
            .children(window.auto_scroll_anchor().map(|anchor| {
                let t = tokens(cx);
                deferred(
                    anchored()
                        .position(point(anchor.x - px(15.), anchor.y - px(15.)))
                        .child(
                            div()
                                .size(px(30.))
                                .rounded_full()
                                .border_1()
                                .border_color(t.border)
                                .bg(t.popover)
                                .shadow(t.shadow_raised())
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(t.muted_foreground)
                                .child(icon("chevrons-up-down")),
                        ),
                )
                .with_priority(20)
            }))
    }
}

impl Shell {
    /// A contact was clicked in the picker: start a chat, or toggle them for a new group.
    fn pick_contact(&mut self, jid: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.group_mode {
            self.start_chat_with(jid, window, cx);
            return;
        }
        match self.group_picks.iter().position(|picked| picked == jid) {
            Some(index) => {
                self.group_picks.remove(index);
            }
            None => self.group_picks.push(jid.to_string()),
        }
        cx.notify();
    }

    fn create_group(&mut self, cx: &mut Context<Self>) {
        let subject = self.group_name.read(cx).value().trim().to_string();
        if subject.is_empty() {
            self.last_error = Some("Give the group a name first.".to_string());
            cx.notify();
            return;
        }
        self.backend.send(Command::CreateGroup {
            subject,
            members: self.group_picks.clone(),
        });
        self.group_mode = false;
        self.picking = false;
        self.group_picks.clear();
        cx.notify();
    }
}

impl Shell {
    /// Plain wording for a privacy setting as the server names it.
    fn privacy_words(category: &str, value: &str) -> (String, String) {
        let label = match category {
            "Last" => "Last seen",
            "Online" => "Online",
            "Profile" => "Profile photo",
            "Status" => "Status",
            "GroupAdd" => "Who can add me to groups",
            "ReadReceipts" => "Read receipts",
            "About" => "About",
            "CallAdd" => "Who can call me",
            "Messages" => "Who can message me",
            other => other,
        };
        let who = match (category, value) {
            ("ReadReceipts", "All") => "On",
            ("ReadReceipts", "None") => "Off",
            (_, "All") => "Everyone",
            (_, "Contacts") => "My contacts",
            (_, "None") => "Nobody",
            (_, "ContactBlacklist") => "My contacts, except some",
            (_, "MatchLastSeen") => "Same as last seen",
            (_, "Known") => "People I know",
            (_, other) => other,
        };
        (label.to_string(), who.to_string())
    }

    /// The privacy and blocked-contacts part of the settings page.
    fn account_sections(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let t = tokens(cx).clone();
        let heading = |text: &'static str, detail: &'static str| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .mb(px(12.))
                .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(text))
                .child(div().text_size(px(14.)).text_color(t.muted_foreground).child(detail))
                .into_any_element()
        };
        let rule = || div().h(px(1.)).my(px(28.)).bg(t.border).into_any_element();
        let mut out = Vec::new();

        out.push(rule());
        out.push(heading(
            "Privacy",
            "Who can see your details. A change here applies to your whole account.",
        ));
        if self.privacy.is_empty() {
            out.push(
                div()
                    .text_size(px(13.))
                    .text_color(t.muted_foreground)
                    .child("Loading…")
                    .into_any_element(),
            );
        }
        for (slot, (category, value)) in self.privacy.iter().enumerate() {
            let (label, who) = Self::privacy_words(category, value);
            let choices = Self::privacy_choices(category);
            let mut row = div()
                .py(px(6.))
                .flex()
                .items_center()
                .gap(px(6.))
                .child(div().flex_1().text_size(px(14.)).child(label));
            if choices.is_empty() || !choices.contains(&value.as_str()) {
                // A value this app cannot set (a custom list made on the phone) is shown as it is.
                row = row.child(div().text_size(px(14.)).text_color(t.muted_foreground).child(who));
            }
            for (index, choice) in choices.iter().enumerate() {
                let active = choice == value;
                let (_, words) = Self::privacy_words(category, choice);
                let (category, choice) = (category.clone(), choice.to_string());
                let hover = t.fg(0.12);
                row = row.child(
                    div()
                        .id(("privacy", slot * 8 + index))
                        .h(px(28.))
                        .px(px(10.))
                        .rounded(R_MD)
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .when(active, |this| this.bg(t.tint(0.14)).text_color(t.primary))
                        .when(!active, |this| {
                            this.bg(t.fg(0.06))
                                .text_color(t.foreground)
                                .hover(move |style| style.bg(hover))
                        })
                        .child(words)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !active {
                                this.backend.send(Command::SetPrivacy {
                                    category: category.clone(),
                                    value: choice.clone(),
                                });
                            }
                            cx.notify();
                        })),
                );
            }
            out.push(row.into_any_element());
        }
        // The timer new chats start with.
        {
            let mut row = div()
                .py(px(6.))
                .flex()
                .items_center()
                .gap(px(6.))
                .child(div().flex_1().text_size(px(14.)).child("Default message timer for new chats"));
            for (index, (label, seconds)) in
                [("Off", 0u32), ("24 hours", 86_400), ("7 days", 604_800), ("90 days", 7_776_000)]
                    .into_iter()
                    .enumerate()
            {
                let hover = t.fg(0.12);
                row = row.child(
                    div()
                        .id(("default-timer", index))
                        .h(px(28.))
                        .px(px(10.))
                        .rounded(R_MD)
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .bg(t.fg(0.06))
                        .hover(move |style| style.bg(hover))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.backend.send(Command::SetDefaultTimer { seconds });
                            cx.notify();
                        })),
                );
            }
            out.push(row.into_any_element());
        }

        out.push(rule());
        out.push(heading("Blocked contacts", "People who cannot message or call you."));
        if self.blocked.is_empty() {
            out.push(
                div()
                    .text_size(px(13.))
                    .text_color(t.muted_foreground)
                    .child("Nobody is blocked.")
                    .into_any_element(),
            );
        }
        for (slot, person) in self.blocked.iter().enumerate() {
            let name = self
                .store
                .display_name(person)
                .unwrap_or_else(|_| display_number(person));
            let who = person.clone();
            let hover = t.fg(0.12);
            out.push(
                div()
                    .py(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(avatar(person, &name, 32., crate::avatars::get(person), cx))
                    .child(div().flex_1().text_size(px(14.)).child(SharedString::from(name)))
                    .child(
                        div()
                            .id(("unblock", slot))
                            .h(px(28.))
                            .px(px(10.))
                            .rounded(R_LG)
                            .flex()
                            .items_center()
                            .bg(t.fg(0.08))
                            .text_size(px(12.))
                            .cursor_pointer()
                            .hover(move |style| style.bg(hover))
                            .child("Unblock")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.backend.send(Command::Block {
                                    jid: who.clone(),
                                    blocked: false,
                                });
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            );
        }
        out
    }
}

impl Shell {
    /// The palette swatches in Settings.
    fn palette_row(&self, cx: &mut Context<Self>) -> AnyElement {
        use super::theme::{Palette, Tokens as PaletteTokens};
        let t = tokens(cx).clone();
        let current = super::theme::palette(cx);
        let mut row = div().flex().flex_wrap().gap(px(8.)).mb(px(12.));
        for (slot, palette) in Palette::ALL.into_iter().enumerate() {
            let sample = PaletteTokens::of(palette, t.dark);
            let active = palette == current;
            let hover = t.fg(0.12);
            row = row.child(
                div()
                    .id(("palette", slot))
                    .h(px(36.))
                    .px(px(10.))
                    .rounded(R_LG)
                    .border_1()
                    .border_color(if active { t.primary } else { gpui::transparent_black() })
                    .bg(t.fg(0.06))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(13.))
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .child(
                        // A two-tone swatch: the palette's surface and its accent.
                        div()
                            .size(px(18.))
                            .rounded_full()
                            .border_1()
                            .border_color(t.fg(0.2))
                            .bg(sample.background)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().size(px(8.)).rounded_full().bg(sample.primary)),
                    )
                    .child(palette.name())
                    .on_click(cx.listener(move |_, _, _, cx| {
                        super::theme::set_palette(palette, cx);
                        cx.refresh_windows();
                    })),
            );
        }
        row.into_any_element()
    }
}

impl Shell {
    /// The surface-style choices in Settings.
    fn style_row(&self, cx: &mut Context<Self>) -> AnyElement {
        use super::theme::Style;
        let t = tokens(cx).clone();
        let current = super::theme::style(cx);
        let mut row = div().flex().flex_wrap().gap(px(8.)).mb(px(12.));
        for (slot, style) in Style::ALL.into_iter().enumerate() {
            let active = style == current;
            let hover = t.fg(0.12);
            row = row.child(
                div()
                    .id(("style", slot))
                    .w(px(204.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(R_LG)
                    .border_1()
                    .border_color(if active { t.primary } else { t.fg(0.08) })
                    .bg(if active { t.tint(0.10) } else { t.fg(0.05) })
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .cursor_pointer()
                    .hover(move |this| this.bg(hover))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if active { t.primary } else { t.foreground })
                            .child(style.name()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.muted_foreground)
                            .child(style.summary()),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        super::theme::set_style(style, cx);
                        cx.refresh_windows();
                    })),
            );
        }
        row.into_any_element()
    }
}

impl Shell {
    /// Liquid Glass only: what is behind the window.
    fn backdrop_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use super::theme::Backdrop;
        let t = tokens(cx).clone();
        if !t.liquid() {
            return None;
        }
        let current = super::theme::backdrop(cx);
        let mut row = div().flex().flex_wrap().items_center().gap(px(8.)).mb(px(12.)).child(
            div()
                .w(px(96.))
                .text_size(px(13.))
                .text_color(t.muted_foreground)
                .child("Background"),
        );
        for (slot, backdrop) in Backdrop::ALL.into_iter().enumerate() {
            let active = backdrop == current;
            let hover = t.fg(0.12);
            row = row.child(
                div()
                    .id(("backdrop", slot))
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(R_LG)
                    .border_1()
                    .border_color(if active { t.primary } else { t.fg(0.08) })
                    .bg(if active { t.tint(0.10) } else { t.fg(0.05) })
                    .flex()
                    .items_center()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if active { t.primary } else { t.foreground })
                    .cursor_pointer()
                    .hover(move |this| this.bg(hover))
                    .child(backdrop.name())
                    .tooltip(tip(backdrop.summary()))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        super::theme::set_backdrop(backdrop, cx);
                        cx.refresh_windows();
                    })),
            );
        }
        Some(row.into_any_element())
    }
}

impl Shell {
    fn open_status_composer(&mut self, cx: &mut Context<Self>) {
        self.section = Section::Status;
        self.status_open = None;
        self.status_draft = Some(StatusDraft {
            background: 0,
            file: None,
        });
        cx.notify();
    }

    /// Posts the status being written and closes the composer.
    fn post_status(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.status_draft.take() else {
            return;
        };
        let text = self.status_text.read(cx).value().trim().to_string();
        if text.is_empty() && draft.file.is_none() {
            self.status_draft = Some(draft);
            return;
        }
        self.backend.send(Command::PostStatus {
            text,
            background: STATUS_COLORS[draft.background],
            file: draft.file,
            only_me: false,
        });
        self.status_text.update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    fn pick_status_file(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    let kind = crate::media_ops::attach_kind_for(&path);
                    if matches!(kind, crate::events::Attach::Image | crate::events::Attach::Video) {
                        if let Some(draft) = this.status_draft.as_mut() {
                            draft.file = Some(path);
                        }
                    } else {
                        this.last_error = Some("A status can be text, a photo or a video.".to_string());
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Sends what is typed under someone's status to their chat, quoting the status.
    fn send_status_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.status_reply.read(cx).value().trim().to_string();
        let Some((person, item)) = self.status_open else {
            return;
        };
        let Some((sender, id)) = self
            .statuses
            .get(person)
            .filter(|group| group.sender != "me")
            .and_then(|group| Some((group.sender.clone(), group.items.get(item)?.id.clone())))
        else {
            return;
        };
        if text.is_empty() {
            return;
        }
        self.backend.send(Command::ReplyStatus { sender, id, text });
        self.status_reply.update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    /// The panel for writing a status: text on a colour, or a photo or video.
    fn status_compose_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        let Some(draft) = self.status_draft.as_ref() else {
            return self.empty_panel(cx);
        };
        let has_text = !self.status_text.read(cx).value().trim().is_empty();
        let ready = has_text || draft.file.is_some();
        let surface = match &draft.file {
            Some(path) => {
                let is_picture = crate::media_ops::attach_kind_for(path) == crate::events::Attach::Image;
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(12.))
                    .child(if is_picture {
                        gpui::img(path.clone())
                            .max_w_full()
                            .max_h(px(420.))
                            .rounded(R_LG)
                            .object_fit(gpui::ObjectFit::Contain)
                            .into_any_element()
                    } else {
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.))
                            .text_color(t.muted_foreground)
                            .child(icon("video").size(px(40.)))
                            .child(SharedString::from(name))
                            .into_any_element()
                    })
                    .child(
                        div()
                            .w_full()
                            .max_w(px(520.))
                            .rounded(R_LG)
                            .border_1()
                            .border_color(t.border)
                            .px(px(8.))
                            .py(px(4.))
                            .child(Textarea::new(&self.status_text).appearance(false).bordered(false)),
                    )
            }
            None => div()
                .size_full()
                .rounded(R_XL)
                .bg(argb_to_color(STATUS_COLORS[draft.background]))
                .p(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .max_w(px(560.))
                        .text_size(px(24.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(gpui::white())
                        .child(Textarea::new(&self.status_text).appearance(false).bordered(false)),
                ),
        };
        let mut swatches = div().flex().items_center().gap(px(6.));
        if draft.file.is_none() {
            for (index, color) in STATUS_COLORS.iter().enumerate() {
                let active = index == draft.background;
                swatches = swatches.child(
                    div()
                        .id(("status-color", index))
                        .size(px(24.))
                        .rounded_full()
                        .bg(argb_to_color(*color))
                        .border_2()
                        .border_color(if active { t.foreground } else { gpui::transparent_black() })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(draft) = this.status_draft.as_mut() {
                                draft.background = index;
                            }
                            cx.notify();
                        })),
                );
            }
        }
        let has_file = draft.file.is_some();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(16.))
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("New status"))
                    .child(div().flex_1())
                    .child(
                        icon_button("status-compose-close", "x", cx).on_click(cx.listener(|this, _, _, cx| {
                            this.status_draft = None;
                            cx.notify();
                        })),
                    ),
            )
            .child(div().flex_1().min_h_0().px(px(24.)).pb(px(12.)).child(surface))
            .child(
                div()
                    .px(px(24.))
                    .pb(px(8.))
                    .text_size(px(12.))
                    .text_color(t.muted_foreground)
                    .child("Goes to your contacts, following the status privacy set on your phone. It disappears after 24 hours."),
            )
            .child(
                div()
                    .px(px(24.))
                    .pb(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(swatches)
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("status-pick-file")
                            .h(px(32.))
                            .px(px(12.))
                            .rounded(R_LG)
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .bg(t.fg(0.06))
                            .text_size(px(13.))
                            .cursor_pointer()
                            .hover(|style| style.bg(t.fg(0.10)))
                            .child(icon("image-plus").size(px(14.)))
                            .child(if has_file { "Change photo or video" } else { "Photo or video" })
                            .on_click(cx.listener(|this, _, _, cx| this.pick_status_file(cx))),
                    )
                    .when(has_file, |this| {
                        this.child(
                            icon_button("status-drop-file", "x", cx)
                                .tooltip(tip("Remove the photo or video"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(draft) = this.status_draft.as_mut() {
                                        draft.file = None;
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        div()
                            .id("status-post")
                            .h(px(32.))
                            .px(px(16.))
                            .rounded(R_LG)
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .when(ready, |this| {
                                this.bg(t.primary)
                                    .text_color(t.primary_foreground)
                                    .cursor_pointer()
                                    .shadow(t.glow())
                            })
                            .when(!ready, |this| this.bg(t.fg(0.06)).text_color(t.muted_foreground))
                            .child(icon("send-horizontal").size(px(14.)))
                            .child("Post")
                            .on_click(cx.listener(|this, _, window, cx| this.post_status(window, cx))),
                    ),
            )
            .into_any_element()
    }

    /// Under a status: a reply box and a heart for someone else's, who saw it and a
    /// delete button for my own.
    fn status_footer(&self, group: &StatusGroup, row: &MsgRow, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        if group.sender != "me" {
            let id = row.id.clone();
            return div()
                .px(px(24.))
                .pb(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(div().w_full().max_w(px(520.)).child(Input::new(&self.status_reply)))
                .child(
                    icon_button("status-like", "heart", cx)
                        .tooltip(tip("Like"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.backend.send(Command::React {
                                chat: "status@broadcast".to_string(),
                                id: id.clone(),
                                emoji: "\u{2764}\u{fe0f}".to_string(),
                            });
                            cx.notify();
                        })),
                )
                .child(
                    icon_button("status-reply-send", "send-horizontal", cx)
                        .tooltip(tip("Send reply"))
                        .on_click(cx.listener(|this, _, window, cx| this.send_status_reply(window, cx))),
                )
                .into_any_element();
        }
        // My own status: who has seen it.
        let cached = self
            .status_viewer_cache
            .borrow()
            .as_ref()
            .filter(|(id, _)| *id == row.id)
            .map(|(_, viewers)| viewers.clone());
        let viewers: Vec<(String, i64)> = match cached {
            Some(viewers) => viewers,
            None => {
                let viewers: Vec<(String, i64)> = self
                    .store
                    .receipts("status@broadcast", &row.id)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|(user, _, read, _)| Some((user, read?)))
                    .collect();
                *self.status_viewer_cache.borrow_mut() = Some((row.id.clone(), viewers.clone()));
                viewers
            }
        };
        let id = row.id.clone();
        let mut footer = div().px(px(24.)).pb(px(20.)).flex().flex_col().items_center().gap(px(8.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .id("status-viewers")
                        .h(px(32.))
                        .px(px(12.))
                        .rounded(R_LG)
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .bg(t.fg(0.06))
                        .text_size(px(13.))
                        .cursor_pointer()
                        .hover(|style| style.bg(t.fg(0.10)))
                        .child(icon("eye").size(px(14.)))
                        .child(format!("Seen by {}", viewers.len()))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.status_viewers = !this.status_viewers;
                            cx.notify();
                        })),
                )
                .child(
                    icon_button("status-delete", "trash", cx)
                        .tooltip(tip("Delete this status"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.backend.send(Command::DeleteStatus { id: id.clone() });
                            this.status_open = None;
                            cx.notify();
                        })),
                ),
        );
        if self.status_viewers && !viewers.is_empty() {
            let mut list = div()
                .id("status-viewer-list")
                .w(px(320.))
                .max_h(px(180.))
                .overflow_y_scroll()
                .rounded(R_LG)
                .border_1()
                .border_color(t.border)
                .p(px(6.))
                .flex()
                .flex_col();
            for (user, seen_at) in viewers.iter().take(200) {
                let name = self.title_of(user);
                list = list.child(
                    div()
                        .h(px(36.))
                        .px(px(8.))
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .gap(px(8.))
                        .child(avatar(user, &name, 24., crate::avatars::get(user), cx))
                        .child(div().flex_1().truncate().text_size(px(13.)).child(SharedString::from(name)))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(t.muted_foreground)
                                .child(list_time(*seen_at)),
                        ),
                );
            }
            footer = footer.child(list);
        }
        footer.into_any_element()
    }
}

impl Shell {
    /// The values this app can set for a privacy category, in the order shown.
    fn privacy_choices(category: &str) -> &'static [&'static str] {
        match category {
            "Last" | "Profile" | "Status" | "GroupAdd" | "About" => &["All", "Contacts", "None"],
            "Online" => &["All", "MatchLastSeen"],
            "ReadReceipts" => &["All", "None"],
            "CallAdd" => &["All", "Contacts", "Known"],
            "Messages" => &["All", "Contacts"],
            _ => &[],
        }
    }

    /// Settings: change the name, the About line and the profile photo.
    fn profile_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        let button = |id: &'static str, label: &'static str| {
            let hover = t.fg(0.12);
            div()
                .id(id)
                .h(px(32.))
                .px(px(12.))
                .rounded(R_LG)
                .flex()
                .flex_shrink_0()
                .items_center()
                .bg(t.fg(0.06))
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .child(label)
        };
        div()
            .mt(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().w(px(64.)).text_size(px(13.)).text_color(t.muted_foreground).child("Name"))
                    .child(div().flex_1().child(Input::new(&self.profile_name)))
                    .child(button("profile-save-name", "Save").on_click(cx.listener(|this, _, _, cx| {
                        let name = this.profile_name.read(cx).value().trim().to_string();
                        if name.is_empty() {
                            this.last_error = Some("The name cannot be empty.".to_string());
                        } else {
                            this.backend.send(Command::SetProfileName { name });
                        }
                        cx.notify();
                    }))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().w(px(64.)).text_size(px(13.)).text_color(t.muted_foreground).child("About"))
                    .child(div().flex_1().child(Input::new(&self.profile_about)))
                    .child(button("profile-save-about", "Save").on_click(cx.listener(|this, _, _, cx| {
                        let text = this.profile_about.read(cx).value().trim().to_string();
                        this.backend.send(Command::SetAbout { text });
                        cx.notify();
                    }))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().w(px(64.)).text_size(px(13.)).text_color(t.muted_foreground).child("Photo"))
                    .child(button("profile-photo-change", "Choose a photo").on_click(cx.listener(|_, _, _, cx| {
                        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some("Use as profile photo".into()),
                        });
                        cx.spawn(async move |this, cx| {
                            if let Ok(Ok(Some(paths))) = chosen.await
                                && let Some(path) = paths.into_iter().next()
                            {
                                let _ = this.update(cx, |this, cx| {
                                    this.backend.send(Command::SetProfilePhoto { path: Some(path) });
                                    cx.notify();
                                });
                            }
                        })
                        .detach();
                    })))
                    .child(button("profile-photo-remove", "Remove photo").on_click(cx.listener(|this, _, _, cx| {
                        this.backend.send(Command::SetProfilePhoto { path: None });
                        cx.notify();
                    }))),
            )
            .into_any_element()
    }
}

impl Shell {
    /// Settings: keep running behind the tray icon, and start with Windows.
    fn windows_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        let switch = |on: bool| {
            div()
                .w(px(32.))
                .h(px(18.))
                .rounded_full()
                .p(px(2.))
                .flex()
                .flex_shrink_0()
                .when(on, |this| this.bg(t.primary).justify_end())
                .when(!on, |this| this.bg(t.fg(0.18)))
                .child(div().size(px(14.)).rounded_full().bg(gpui::white()))
        };
        let row = |id: &'static str, title: &'static str, detail: &'static str, on: bool| {
            let hover = t.fg(0.05);
            div()
                .id(id)
                .py(px(8.))
                .px(px(8.))
                .mx(px(-8.))
                .rounded(R_LG)
                .flex()
                .items_center()
                .gap(px(12.))
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(14.)).child(title))
                        .child(div().text_size(px(12.)).text_color(t.muted_foreground).child(detail)),
                )
                .child(switch(on))
        };
        let keep = crate::tray::keep_running();
        let autostart = self.autostart;
        let notify_on = self.notify_on;
        div()
            .flex()
            .flex_col()
            .child(
                row(
                    "setting-notifications",
                    "Notifications for new messages",
                    "A Windows notification when a message arrives in a chat that is not in front. Muted chats never notify.",
                    notify_on,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.notify_on = !notify_on;
                    let _ = this.store.set_kv("notifications", if notify_on { "off" } else { "on" });
                    cx.notify();
                })),
            )
            .child(
                row(
                    "setting-background",
                    "Keep running when the window is closed",
                    "Messages keep arriving and notifications keep showing. Quit from the icon next to the clock.",
                    keep,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    crate::tray::set_keep_running(!keep);
                    let _ = this.store.set_kv("background", if keep { "off" } else { "on" });
                    cx.notify();
                })),
            )
            .child(
                row(
                    "setting-autostart",
                    "Start with Windows",
                    "Starts in the background when you sign in.",
                    autostart,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if crate::tray::set_start_with_windows(!autostart) {
                        this.autostart = !autostart;
                    } else {
                        this.last_error = Some("Windows did not accept the start-up change.".to_string());
                    }
                    cx.notify();
                })),
            )
            .into_any_element()
    }
}

impl Shell {
    /// Channels: find one by its link and follow it, or start a new one.
    fn channel_finder(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        let button = |id: &'static str, label: &'static str, strong: bool| {
            let hover = t.fg(0.12);
            div()
                .id(id)
                .h(px(32.))
                .px(px(12.))
                .rounded(R_LG)
                .flex()
                .flex_shrink_0()
                .items_center()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .when(strong, |this| this.bg(t.primary).text_color(t.primary_foreground))
                .when(!strong, |this| this.bg(t.fg(0.06)).hover(move |style| style.bg(hover)))
                .child(label)
        };
        let mut panel = div()
            .px(px(12.))
            .pb(px(10.))
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().flex_1().child(Input::new(&self.channel_link)))
                    .child(button("channel-find", "Find", false).on_click(cx.listener(|this, _, _, cx| {
                        let link = this.channel_link.read(cx).value().trim().to_string();
                        if !link.is_empty() {
                            this.channel_found = None;
                            this.backend.send(Command::LookupChannel { link });
                        }
                        cx.notify();
                    }))),
            );
        if let Some((link, name, description, followers, verified)) = self.channel_found.clone() {
            panel = panel.child(
                div()
                    .rounded(R_LG)
                    .border_1()
                    .border_color(t.border)
                    .bg(t.fg(0.04))
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from(name)),
                            )
                            .when(verified, |this| {
                                this.child(icon("circle-check").size(px(14.)).text_color(t.primary))
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.muted_foreground)
                            .child(format!("{} followers", super::message::short_count(followers as usize))),
                    )
                    .when(!description.is_empty(), |this| {
                        this.child(
                            div()
                                .max_h(px(54.))
                                .overflow_hidden()
                                .text_size(px(12.))
                                .text_color(t.fg(0.8))
                                .child(SharedString::from(description)),
                        )
                    })
                    .child(
                        div().flex().justify_end().child(button("channel-follow", "Follow", true).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.backend.send(Command::FollowChannel { link: link.clone() });
                                this.channel_found = None;
                                this.channel_panel = false;
                                this.channel_link.update(cx, |input, cx| input.set_value("", window, cx));
                                cx.notify();
                            }),
                        )),
                    ),
            );
        }
        panel = panel.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(div().flex_1().child(Input::new(&self.channel_name)))
                .child(button("channel-create", "Create", false).on_click(cx.listener(|this, _, window, cx| {
                    let name = this.channel_name.read(cx).value().trim().to_string();
                    if name.is_empty() {
                        return;
                    }
                    this.backend.send(Command::CreateChannel {
                        name,
                        description: String::new(),
                    });
                    this.channel_name.update(cx, |input, cx| input.set_value("", window, cx));
                    this.channel_panel = false;
                    cx.notify();
                }))),
        );
        panel.into_any_element()
    }
}

impl Shell {
    fn on_call_update(&mut self, update: crate::events::CallUpdate, cx: &mut Context<Self>) {
        use crate::events::CallUpdate;
        match update {
            CallUpdate::Ringing { call_id, from, video } => {
                let name = self.title_of(&from);
                self.call = Some(CallView {
                    call_id: Some(call_id),
                    peer: from,
                    name,
                    video,
                    phase: CallPhase::Ringing,
                    muted: false,
                    connected_at: None,
                });
                // A ringing call brings the window back from the tray.
                crate::tray::show_main_window();
            }
            CallUpdate::Calling { peer } => {
                let name = self.title_of(&peer);
                self.call = Some(CallView {
                    call_id: None,
                    peer,
                    name,
                    video: false,
                    phase: CallPhase::Calling,
                    muted: false,
                    connected_at: None,
                });
            }
            CallUpdate::Answered => {
                if let Some(call) = self.call.as_mut() {
                    call.phase = CallPhase::Connecting;
                    call.call_id = None;
                }
            }
            CallUpdate::Connected => {
                if let Some(call) = self.call.as_mut() {
                    call.phase = CallPhase::Connected;
                    call.connected_at.get_or_insert_with(std::time::Instant::now);
                }
                // The timer on the card moves once a second.
                cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
                        let running = this
                            .update(cx, |this, cx| {
                                cx.notify();
                                this.call.as_ref().is_some_and(|call| call.phase == CallPhase::Connected)
                            })
                            .unwrap_or(false);
                        if !running {
                            break;
                        }
                    }
                })
                .detach();
            }
            CallUpdate::Muted(muted) => {
                if let Some(call) = self.call.as_mut() {
                    call.muted = muted;
                }
            }
            CallUpdate::Ended { reason } => {
                if let Some(call) = self.call.as_mut() {
                    // The first reason stands; a second "ended" for the same call adds nothing.
                    if !matches!(call.phase, CallPhase::Ended(_)) {
                        call.phase = CallPhase::Ended(reason);
                        call.call_id = None;
                        // The card stays a moment, then goes.
                        cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(std::time::Duration::from_millis(2500)).await;
                            let _ = this.update(cx, |this, cx| {
                                if this.call.as_ref().is_some_and(|call| matches!(call.phase, CallPhase::Ended(_))) {
                                    this.call = None;
                                    cx.notify();
                                }
                            });
                        })
                        .detach();
                    }
                }
                // The calls list has a new entry.
                self.stale_chats = true;
            }
        }
        cx.notify();
    }

    /// The call card: who, what is happening, and the buttons that fit the moment.
    fn call_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call = self.call.as_ref()?;
        let t = tokens(cx).clone();
        let status = match &call.phase {
            CallPhase::Ringing if call.video => "Incoming video call (answers with voice only)".to_string(),
            CallPhase::Ringing => "Incoming voice call".to_string(),
            CallPhase::Calling => "Calling…".to_string(),
            CallPhase::Connecting => "Connecting…".to_string(),
            CallPhase::Connected => {
                let seconds = call.connected_at.map(|since| since.elapsed().as_secs()).unwrap_or(0);
                format!("{}:{:02}", seconds / 60, seconds % 60)
            }
            CallPhase::Ended(reason) => reason.clone(),
        };
        let round = |id: &'static str, glyph: &'static str, fill: gpui::Hsla, text: gpui::Hsla| {
            div()
                .id(id)
                .size(px(44.))
                .rounded_full()
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .bg(fill)
                .text_color(text)
                .cursor_pointer()
                .hover(|style| style.opacity(0.85))
                .child(icon(glyph).size(px(20.)))
        };
        let red: gpui::Hsla = gpui::rgb(0xe5484d).into();
        let green: gpui::Hsla = gpui::rgb(0x30a46c).into();
        let mut buttons = div().flex().items_center().gap(px(14.));
        match &call.phase {
            CallPhase::Ringing => {
                let (yes, no) = (call.call_id.clone().unwrap_or_default(), call.call_id.clone().unwrap_or_default());
                buttons = buttons
                    .child(
                        round("call-decline", "phone-off", red, gpui::white())
                            .tooltip(tip("Decline"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.backend.send(Command::DeclineCall { call_id: no.clone() });
                                cx.notify();
                            })),
                    )
                    .child(
                        round("call-accept", "phone", green, gpui::white())
                            .tooltip(tip("Answer"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.backend.send(Command::AnswerCall { call_id: yes.clone() });
                                cx.notify();
                            })),
                    );
            }
            CallPhase::Calling | CallPhase::Connecting | CallPhase::Connected => {
                let muted = call.muted;
                buttons = buttons
                    .child(
                        round(
                            "call-mute",
                            if muted { "mic-off" } else { "mic" },
                            if muted { t.foreground } else { t.fg(0.12) },
                            if muted { t.background } else { t.foreground },
                        )
                        .tooltip(tip(if muted { "Unmute" } else { "Mute" }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.backend.send(Command::MuteCall { muted: !muted });
                            cx.notify();
                        })),
                    )
                    .child(
                        round("call-end", "phone-off", red, gpui::white())
                            .tooltip(tip("End call"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.backend.send(Command::EndCall);
                                cx.notify();
                            })),
                    );
            }
            CallPhase::Ended(_) => {}
        }
        Some(
            // A full-width row that only centres the card; it takes no clicks itself.
            div()
                .absolute()
                .top(px(44.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("call-card")
                        .occlude()
                        .w(px(340.))
                        .p(px(16.))
                        .rounded(px(20.))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.popover_fill())
                        .shadow(t.shadow_dialog())
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(12.))
                        .child(avatar(&call.peer, &call.name, 64., crate::avatars::get(&call.peer), cx))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(SharedString::from(call.name.clone())),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(t.muted_foreground)
                                        .child(status),
                                ),
                        )
                        .child(buttons),
                )
                .into_any_element(),
        )
    }
}
