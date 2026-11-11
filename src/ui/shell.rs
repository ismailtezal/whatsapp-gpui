//! The window: title bar, rail, the column for the current section, and the floating
//! main panel.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, Image, ImageFormat, MouseButton, MouseDownEvent,
    Pixels, Point, SharedString, SystemNotification, Task, Window, WindowControlArea, anchored,
    deferred, div, point, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use super::chat::{ChatEvent, ChatView};
use super::theme::{COLUMN_W, HEADER_H, R_LG, R_MD, R_XL, RAIL_W, TITLE_BAR_H, Tokens, tokens};
use super::widgets::{avatar, count_badge, duration, icon, icon_button, list_time, tip};
use crate::backend::Backend;
use crate::content::preview_label;
use crate::events::{Command, ConnState, UiEvent};
use crate::model::{Ack, ChatKind, ChatRow, ContactRow, MsgKind, MsgRow, display_number};
use crate::store::Store;
use crate::store_more::CallRow;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Chats,
    Unread,
    Groups,
    Archived,
    Calls,
    Status,
    Channels,
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
            Section::Starred => "Starred messages",
            Section::Settings => "Settings",
        }
    }
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
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let group_name = cx.new(|cx| InputState::new(window, cx).placeholder("Group name"));
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
                window.activate_window();
                let _ = shell.update(cx, |this, cx| this.open_chat(&chat, window, cx));
            });
        });

        let events = backend.events();
        let pump = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let alive = this.update(cx, |this, cx| this.on_event(event, cx)).is_ok();
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
        // Statuses live for 24 hours.
        let rows = self.store.status_updates(now_secs() - 24 * 3600).unwrap_or_default();
        let mut groups: Vec<StatusGroup> = Vec::new();
        for row in rows {
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
        groups.sort_by_key(|g| std::cmp::Reverse(g.items.last().map(|r| r.ts).unwrap_or(0)));
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
        self.section = section;
        self.picking = false;
        self.status_open = None;
        self.confirm_logout = false;
        self.reload();
        self.run_search();
        cx.notify();
    }

    fn on_event(&mut self, event: UiEvent, cx: &mut Context<Self>) {
        match event {
            UiEvent::Connection(state) => {
                if state == ConnState::Online {
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
                }
                if let Some(view) = &self.chat {
                    let (name, jid) = (self.me_name.clone(), self.me_jid.clone());
                    view.update(cx, |view, cx| view.set_me(name, jid, cx));
                }
            }
            UiEvent::ChatsChanged => self.reload(),
            UiEvent::HistoryProgress { .. } => {
                // History arrived (first sync or an answer to "get earlier messages").
                self.reload();
                if let Some(view) = &self.chat {
                    view.update(cx, |view, cx| view.reload(cx));
                }
            }
            UiEvent::ChatChanged { chat } => {
                if self.selected.as_deref() == Some(chat.as_str())
                    && let Some(view) = &self.chat
                {
                    view.update(cx, |view, cx| view.reload(cx));
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
                if !open_and_focused && !muted && chat != "status@broadcast" {
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
            UiEvent::IncomingCall { from, video, .. } => {
                let name = self
                    .store
                    .display_name(&from)
                    .unwrap_or_else(|_| display_number(&from));
                cx.show_system_notification(SystemNotification {
                    tag: SharedString::from(format!("call-{from}")),
                    title: name.into(),
                    body: format!(
                        "Incoming {} call. Answer on your phone: calls are not available in this app yet.",
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

    fn visible_chats(&self) -> Vec<&ChatRow> {
        let query = self.query.trim().to_lowercase();
        self.chats
            .iter()
            .filter(|chat| match self.section {
                Section::Unread => chat.unread > 0 || chat.marked_unread,
                Section::Groups => matches!(chat.kind, ChatKind::Group | ChatKind::Community),
                Section::Channels => chat.kind == ChatKind::Newsletter,
                Section::Chats => chat.kind != ChatKind::Newsletter,
                _ => true,
            })
            .filter(|chat| query.is_empty() || chat.title.to_lowercase().contains(&query))
            .collect()
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
            .bg(t.rail)
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
                .when(selected, |this| this.bg(t.tint(0.12)).text_color(t.primary))
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
            .w(RAIL_W)
            .h_full()
            .flex_shrink_0()
            .bg(t.rail)
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
            let text = preview_label(preview.kind, preview.text.as_deref());
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
        let title = self
            .store
            .chat(&row.chat)
            .ok()
            .flatten()
            .map(|c| c.title)
            .unwrap_or_else(|| display_number(&row.chat));
        let who = if row.from_me { "You: " } else { "" };
        let detail = div()
            .truncate()
            .child(SharedString::from(format!(
                "{who}{}",
                preview_label(row.kind, row.text.as_deref())
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
        let title = match self.store.chat(&call.peer).ok().flatten() {
            Some(chat) => chat.title,
            None => self
                .store
                .display_name(&call.peer)
                .unwrap_or_else(|_| display_number(&call.peer)),
        };
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

    fn rows(&self, cx: &mut Context<Self>) -> (Vec<AnyElement>, &'static str) {
        let t = tokens(cx).clone();
        let label = |text: &'static str| {
            div()
                .px(px(16.))
                .pt(px(10.))
                .pb(px(4.))
                .flex_shrink_0()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(Tokens::with_alpha(t.sidebar_foreground, 0.6))
                .child(text)
                .into_any_element()
        };
        if self.picking {
            let query = self.query.trim().to_lowercase();
            let rows = self
                .contacts
                .iter()
                .filter(|c| {
                    query.is_empty()
                        || c.name.as_deref().unwrap_or_default().to_lowercase().contains(&query)
                        || c.jid.contains(&query)
                })
                .enumerate()
                .map(|(slot, contact)| {
                    let name = contact.name.clone().unwrap_or_else(|| display_number(&contact.jid));
                    let detail = div()
                        .truncate()
                        .child(contact.about.clone().unwrap_or_else(|| display_number(&contact.jid)))
                        .into_any_element();
                    let jid = contact.jid.clone();
                    let picked = self.group_picks.contains(&contact.jid);
                    self.list_row(
                        SharedString::from(format!("contact-{slot}")),
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
                })
                .collect();
            return (rows, "No contacts match.");
        }
        match self.section {
            Section::Calls => (
                self.calls
                    .iter()
                    .enumerate()
                    .map(|(slot, call)| self.call_row(slot, call, cx))
                    .collect(),
                "No calls yet.",
            ),
            Section::Starred => (
                self.starred
                    .iter()
                    .enumerate()
                    .map(|(slot, row)| self.message_hit_row(slot, row, cx))
                    .collect(),
                "No starred messages. Right-click a message and choose Star.",
            ),
            Section::Status => {
                let rows = self
                    .statuses
                    .iter()
                    .enumerate()
                    .map(|(index, group)| {
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
                            this.request_status_media();
                            cx.notify();
                        }))
                        .into_any_element()
                    })
                    .collect();
                (rows, "No status updates in the last 24 hours.")
            }
            Section::Settings => (Vec::new(), ""),
            _ => {
                let mut rows: Vec<AnyElement> = Vec::new();
                let chats = self.visible_chats();
                let searching = !self.query.trim().is_empty();
                if searching && !chats.is_empty() {
                    rows.push(label("Chats"));
                }
                rows.extend(chats.into_iter().map(|chat| self.chat_row(chat, cx)));
                if !self.hits.is_empty() {
                    rows.push(label("Messages"));
                    rows.extend(
                        self.hits
                            .iter()
                            .enumerate()
                            .map(|(slot, row)| self.message_hit_row(slot, row, cx)),
                    );
                }
                (rows, "No chats here.")
            }
        }
    }

    fn column(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
        let (rows, empty_text) = self.rows(cx);
        let empty = rows.is_empty();
        let title = if self.picking {
            "New chat"
        } else {
            self.section.title()
        };

        div()
            .w(COLUMN_W)
            .h_full()
            .flex_shrink_0()
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
            .child(
                div()
                    .id("column-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .pb(px(8.))
                    .children(rows)
                    .when(empty && !empty_text.is_empty(), |this| {
                        this.child(
                            div()
                                .p(px(24.))
                                .text_size(px(13.))
                                .text_color(t.muted_foreground)
                                .child(empty_text),
                        )
                    }),
            )
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
            .bg(t.popover)
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
            (MsgKind::Text, _) => div()
                .max_w(px(520.))
                .text_size(px(26.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground)
                .child(SharedString::from(row.text.clone().unwrap_or_default()))
                .into_any_element(),
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
                        .pb(px(20.))
                        .flex()
                        .justify_center()
                        .text_size(px(14.))
                        .text_color(t.fg(0.9))
                        .child(SharedString::from(caption)),
                )
            })
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
                    .child(rule())
                    .child(heading("Appearance", "The look follows diskurt's modern design."))
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
        let t = tokens(cx).clone();
        let panel: AnyElement = match (self.section, self.status_open, &self.chat) {
            (Section::Settings, _, _) => self.settings_panel(cx),
            (Section::Status, Some((person, item)), _) => self.status_panel(person, item, cx),
            (_, _, Some(chat)) => chat.clone().into_any_element(),
            _ => self.empty_panel(cx),
        };
        let chat_menu = self.chat_menu_overlay(window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.rail)
            .text_color(t.foreground)
            .font_family(super::theme::FONT_UI)
            .text_size(px(14.))
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
                                div().flex_1().min_w_0().p(px(8.)).child(
                                    div()
                                        .size_full()
                                        .rounded(R_XL)
                                        .border_1()
                                        .border_color(t.fg(0.10))
                                        .bg(t.background)
                                        .shadow(t.shadow_panel())
                                        .overflow_hidden()
                                        .child(panel),
                                ),
                            ),
                    ),
            )
            .children(chat_menu)
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
            "Who can see your details. Shown here as set on your account; change them on your phone.",
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
        for (category, value) in &self.privacy {
            let (label, who) = Self::privacy_words(category, value);
            out.push(
                div()
                    .py(px(8.))
                    .flex()
                    .items_center()
                    .child(div().flex_1().text_size(px(14.)).child(label))
                    .child(div().text_size(px(14.)).text_color(t.muted_foreground).child(who))
                    .into_any_element(),
            );
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
