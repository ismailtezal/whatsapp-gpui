//! The window: title bar, rail, conversation column and the floating main panel.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, SharedString, Task, Window, WindowControlArea,
    div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use super::chat::ChatView;
use super::theme::{COLUMN_W, HEADER_H, R_LG, R_XL, RAIL_W, TITLE_BAR_H, Tokens, tokens};
use super::widgets::{avatar, count_badge, icon, icon_button, list_time, tip};
use crate::backend::Backend;
use crate::content::preview_label;
use crate::events::{ConnState, UiEvent};
use crate::model::{Ack, ChatKind, ChatRow};
use crate::store::Store;

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

pub struct Shell {
    backend: Backend,
    store: Arc<Store>,
    conn: ConnState,
    me_name: String,
    me_jid: Option<String>,
    section: Section,
    chats: Vec<ChatRow>,
    archived_count: usize,
    selected: Option<String>,
    chat: Option<Entity<ChatView>>,
    search: Entity<InputState>,
    query: String,
    qr: Option<String>,
    last_error: Option<String>,
    _pump: Task<()>,
}

impl Shell {
    pub fn new(backend: Backend, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = backend.store().clone();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search or start a new chat"));
        cx.subscribe_in(&search, window, |this, state, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = state.read(cx).value().to_string();
                cx.notify();
            }
        })
        .detach();

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
            archived_count: 0,
            selected: None,
            chat: None,
            search,
            query: String::new(),
            qr: None,
            last_error: None,
            _pump: pump,
        };
        shell.reload_chats();
        // Development aid: open a chat by (part of) its title straight away.
        if let Ok(wanted) = std::env::var("WA_OPEN_CHAT") {
            let wanted = wanted.to_lowercase();
            let found = shell
                .chats
                .iter()
                .find(|c| c.title.to_lowercase().contains(&wanted))
                .map(|c| c.jid.clone());
            if let Some(jid) = found {
                shell.open_chat(&jid, window, cx);
            }
        }
        shell
    }

    fn reload_chats(&mut self) {
        let archived = self.section == Section::Archived;
        self.chats = self.store.chats(archived).unwrap_or_default();
        self.archived_count = if archived {
            self.chats.len()
        } else {
            self.store.chats(true).map(|c| c.len()).unwrap_or(0)
        };
    }

    fn on_event(&mut self, event: UiEvent, cx: &mut Context<Self>) {
        match event {
            UiEvent::Connection(state) => {
                if state == ConnState::Online {
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
            }
            UiEvent::ChatsChanged | UiEvent::HistoryProgress { .. } => self.reload_chats(),
            UiEvent::ChatChanged { chat } => {
                if self.selected.as_deref() == Some(chat.as_str())
                    && let Some(view) = &self.chat
                {
                    view.update(cx, |view, cx| view.reload(cx));
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
            }
            UiEvent::Error(message) => self.last_error = Some(message),
            _ => {}
        }
        cx.notify();
    }

    fn open_chat(&mut self, jid: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(jid) {
            return;
        }
        let Some(chat) = self.chats.iter().find(|c| c.jid == jid).cloned() else {
            return;
        };
        self.selected = Some(jid.to_string());
        let backend = self.backend.clone();
        let me_name = self.me_name.clone();
        let me_jid = self.me_jid.clone();
        let view = cx.new(|cx| ChatView::new(backend, chat, me_name, me_jid, window, cx));
        view.update(cx, |view, cx| view.focus_input(window, cx));
        self.chat = Some(view);
        self.reload_chats();
        cx.notify();
    }

    fn visible_chats(&self) -> Vec<&ChatRow> {
        let query = self.query.trim().to_lowercase();
        self.chats
            .iter()
            .filter(|chat| match self.section {
                Section::Unread => chat.unread > 0 || chat.marked_unread,
                Section::Groups => matches!(chat.kind, ChatKind::Group | ChatKind::Community),
                Section::Channels => chat.kind == ChatKind::Newsletter,
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
        let unread_total: i64 = self.chats.iter().filter(|c| c.mute_until == 0).map(|c| c.unread).sum();
        let tile = |section: Section, name: &'static str, label: &'static str, cx: &mut Context<Self>| {
            let selected = self.section == section;
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
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.section = section;
                    this.reload_chats();
                    cx.notify();
                }))
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
                    .tooltip(tip("Profile")),
            )
    }

    // ---- conversation column -----------------------------------------------------------

    fn chat_row(&self, chat: &ChatRow, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        let selected = self.selected.as_deref() == Some(chat.jid.as_str());
        let unread = chat.unread > 0 || chat.marked_unread;
        let muted = chat.mute_until != 0;
        let jid = chat.jid.clone();
        let hover = Tokens::with_alpha(t.sidebar_accent, 0.45);

        let preview: AnyElement = if let Some(draft) = chat.draft.as_deref().filter(|d| !d.is_empty()) {
            div()
                .flex()
                .gap(px(4.))
                .child(div().text_color(t.destructive).child("Draft:"))
                .child(div().truncate().child(draft.to_string()))
                .into_any_element()
        } else if let Some(preview) = &chat.preview {
            let text = preview_label(preview.kind, preview.text.as_deref());
            let prefix = if preview.from_me {
                None
            } else if chat.kind != ChatKind::User {
                preview.sender_name.clone().map(|name| format!("{name}:"))
            } else {
                None
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

        div()
            .id(SharedString::from(format!("chat-{}", chat.jid)))
            .h(px(60.))
            .mx(px(8.))
            .px(px(8.))
            .rounded(R_LG)
            .flex()
            .items_center()
            .gap(px(12.))
            .cursor_pointer()
            .when(selected, |this| this.bg(t.fg(0.09)))
            .when(!selected, |this| this.hover(move |style| style.bg(hover)))
            .on_click(cx.listener(move |this, _, window, cx| this.open_chat(&jid, window, cx)))
            .child(avatar(&chat.jid, &chat.title, 40., crate::avatars::get(&chat.jid), cx))
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
                                    .font_weight(if unread || selected {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .text_color(t.sidebar_foreground)
                                    .child(SharedString::from(chat.title.clone())),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(11.))
                                    .text_color(if unread && !muted { t.primary } else { t.muted_foreground })
                                    .child(list_time(chat.last_ts)),
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
                                    .text_color(if unread {
                                        Tokens::with_alpha(t.sidebar_foreground, 0.85)
                                    } else {
                                        t.muted_foreground
                                    })
                                    .child(preview),
                            )
                            .when(muted, |this| {
                                this.child(icon("bell-off").size(px(12.)).text_color(t.muted_foreground))
                            })
                            .when(chat.pinned > 0, |this| {
                                this.child(icon("pin").size(px(12.)).text_color(t.muted_foreground))
                            })
                            .when(chat.unread > 0, |this| this.child(count_badge(chat.unread, muted, cx)))
                            .when(chat.unread == 0 && chat.marked_unread, |this| {
                                this.child(div().size(px(10.)).rounded_full().bg(t.primary))
                            }),
                    ),
            )
            .into_any_element()
    }

    fn column(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx).clone();
        let title = match self.section {
            Section::Chats => "Chats",
            Section::Unread => "Unread",
            Section::Groups => "Groups",
            Section::Archived => "Archived",
            Section::Calls => "Calls",
            Section::Status => "Status",
            Section::Channels => "Channels",
            Section::Starred => "Starred messages",
            Section::Settings => "Settings",
        };
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
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.section = section;
                    this.reload_chats();
                    cx.notify();
                }))
        };
        let is_chat_list = matches!(
            self.section,
            Section::Chats | Section::Unread | Section::Groups | Section::Archived | Section::Channels
        );
        let rows: Vec<AnyElement> = if is_chat_list {
            self.visible_chats()
                .into_iter()
                .map(|chat| self.chat_row(chat, cx))
                .collect()
        } else {
            Vec::new()
        };
        let empty = rows.is_empty();

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
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.foreground)
                            .child(title),
                    )
                    .child(icon_button("new-chat", "square-pen", cx).tooltip(tip("New chat")))
                    .child(icon_button("list-menu", "ellipsis-vertical", cx).tooltip(tip("Menu"))),
            )
            .child(
                div().px(px(12.)).pb(px(8.)).child(
                    Input::new(&self.search)
                        .prefix(icon("search").size(px(14.)).text_color(t.muted_foreground))
                        .cleanable(true),
                ),
            )
            .when(is_chat_list, |this| {
                this.child(
                    div()
                        .px(px(12.))
                        .pb(px(8.))
                        .flex()
                        .gap(px(6.))
                        .child(filter(Section::Chats, "All", cx))
                        .child(filter(Section::Unread, "Unread", cx))
                        .child(filter(Section::Groups, "Groups", cx)),
                )
            })
            .child(
                div()
                    .id("chat-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .pb(px(8.))
                    .children(rows)
                    .when(empty, |this| {
                        this.child(
                            div()
                                .p(px(24.))
                                .text_size(px(13.))
                                .text_color(t.muted_foreground)
                                .child(if is_chat_list {
                                    "No chats here yet."
                                } else {
                                    "This section is not built yet."
                                }),
                        )
                    }),
            )
    }

    // ---- main panel --------------------------------------------------------------------

    fn empty_panel(&self, cx: &App) -> AnyElement {
        let t = tokens(cx);
        let (title, detail): (&str, String) = match (&self.conn, &self.qr) {
            (ConnState::Pairing, Some(_)) => (
                "Link this device",
                "On your phone: WhatsApp → Settings → Linked devices → Link a device.".to_string(),
            ),
            (ConnState::LoggedOut, _) => (
                "Logged out",
                "This device was unlinked. Restart the app to link it again.".to_string(),
            ),
            _ => (
                "WhatsApp for Windows",
                "Select a chat to start messaging. Your messages are end-to-end encrypted.".to_string(),
            ),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(16.))
            .child(
                div()
                    .size(px(44.))
                    .rounded(R_XL)
                    .bg(t.tint(0.10))
                    .border_1()
                    .border_color(t.tint(0.15))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(t.primary)
                    .child(icon("messages-square").size(px(20.))),
            )
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
        let panel: AnyElement = match &self.chat {
            Some(chat) => chat.clone().into_any_element(),
            None => self.empty_panel(cx),
        };
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
    }
}
