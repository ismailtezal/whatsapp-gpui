//! The open conversation: header, message list and composer, with the actions on a
//! message (reply, react, copy, star, edit, forward, delete) and on its media.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, ExternalPaths, FontWeight, ListAlignment,
    ListState, MouseButton, MouseDownEvent, PathPromptOptions, Pixels, Point, SharedString, Window,
    anchored, deferred, div, list, point, px,
};
use gpui_component::input::{InputEvent, Textarea, TextareaState};

use super::message::{
    MsgView, Quote, ReactionChip, RowHooks, day_separator, media_of, message_row, notice_of,
    record_of, unread_separator,
};
use super::theme::{HEADER_H, R_LG, R_MD, R_XL, Tokens, tokens};
use super::widgets::{avatar, day_label, icon, icon_button, local_day, tip};
use crate::backend::Backend;
use crate::content::{self, preview_label};
use crate::events::{Activity, Command};
use crate::media_ops::attach_kind_for;
use crate::model::{ChatKind, ChatRow, MsgKind, MsgRow};
use crate::richtext::{self, Mention};
use crate::store::Store;

const PAGE: usize = 80;
/// Messages from one author this close together share a header.
const GROUP_GAP_SECS: i64 = 7 * 60;
/// An own text message can be edited for this long after sending.
const EDIT_WINDOW_SECS: i64 = 15 * 60;

const QUICK_REACTIONS: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🙏"];

const EMOJI: &[&str] = &[
    "😀", "😃", "😄", "😁", "😆", "😅", "🤣", "😂", "🙂", "🙃", "😉", "😊", "😇", "🥰", "😍", "🤩",
    "😘", "😗", "😚", "😙", "😋", "😛", "😜", "🤪", "😝", "🤑", "🤗", "🤭", "🤫", "🤔", "🤐", "🤨",
    "😐", "😑", "😶", "😏", "😒", "🙄", "😬", "🤥", "😌", "😔", "😪", "🤤", "😴", "😷", "🤒", "🤕",
    "🤢", "🤮", "🤧", "🥵", "🥶", "🥴", "😵", "🤯", "🤠", "🥳", "😎", "🤓", "🧐", "😕", "😟", "🙁",
    "😮", "😯", "😲", "😳", "🥺", "😦", "😧", "😨", "😰", "😥", "😢", "😭", "😱", "😖", "😣", "😞",
    "😓", "😩", "😫", "🥱", "😤", "😡", "😠", "🤬", "😈", "👿", "💀", "💩", "🤡", "👻", "👽", "🤖",
    "👍", "👎", "👌", "✌️", "🤞", "🤟", "🤘", "🤙", "👈", "👉", "👆", "👇", "☝️", "✋", "🤚", "🖐️",
    "👋", "👏", "🙌", "👐", "🤲", "🤝", "🙏", "✍️", "💪", "👀", "🧠", "🫶", "❤️", "🧡", "💛", "💚",
    "💙", "💜", "🖤", "🤍", "🤎", "💔", "❣️", "💕", "💞", "💓", "💗", "💖", "💘", "💝", "💯", "💢",
    "🔥", "✨", "🎉", "🎊", "🎁", "🏆", "⭐", "🌟", "⚡", "☀️", "🌙", "🌈", "☁️", "❄️", "🌹", "🌸",
    "🍀", "🍎", "🍕", "🍔", "🍟", "🍰", "☕", "🍺", "🍷", "⚽", "🏀", "🎮", "🎵", "🎬", "📷", "💻",
    "📱", "⏰", "📌", "📎", "✅", "❌", "❗", "❓", "⚠️", "🚀", "🚗", "✈️", "🏠", "💰", "🔒", "🔑",
];

enum Item {
    Day(i64),
    Unread,
    Msg(Box<MsgView>),
}

#[derive(Clone)]
struct MenuState {
    id: String,
    at: Point<Pixels>,
}

#[derive(Clone, PartialEq)]
enum EmojiTarget {
    Composer,
    Message(String),
}

#[derive(Clone)]
struct EmojiState {
    target: EmojiTarget,
    at: Point<Pixels>,
}

pub struct ChatView {
    backend: Backend,
    store: Arc<Store>,
    pub chat: ChatRow,
    me_name: String,
    me_jid: Option<String>,
    items: Vec<Item>,
    list: ListState,
    input: Entity<TextareaState>,
    names: HashMap<String, String>,
    /// How many messages are loaded (grows as older pages are fetched).
    loaded: usize,
    has_older: bool,
    /// Unread count when the chat was opened: where the "New" line sits.
    unread_at_open: i64,
    reply_to: Option<MsgRow>,
    /// ID of the own message being edited in the composer.
    editing: Option<String>,
    activity: Option<(String, Activity)>,
    presence: Option<String>,
    /// Media already asked for in this view, so a failed download is not retried in a loop.
    requested: HashSet<String>,
    open_when_ready: Option<String>,
    menu: Option<MenuState>,
    emoji: Option<EmojiState>,
    viewer: Option<PathBuf>,
    notice: Option<String>,
}

impl ChatView {
    pub fn new(
        backend: Backend,
        chat: ChatRow,
        me_name: String,
        me_jid: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = backend.store().clone();
        let placeholder = format!("Message {}", chat.title);
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 14)
                .submit_on_enter(true)
                .placeholder(placeholder)
        });
        cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| match event {
            InputEvent::PressEnter { shift: false, .. } => this.send(window, cx),
            InputEvent::Change => {
                this.typing_changed(cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();

        let unread_at_open = chat.unread;
        let mut view = ChatView {
            backend,
            store,
            chat,
            me_name,
            me_jid,
            items: Vec::new(),
            list: ListState::new(0, ListAlignment::Bottom, px(1200.)),
            input,
            names: HashMap::new(),
            loaded: PAGE,
            has_older: true,
            unread_at_open,
            reply_to: None,
            editing: None,
            activity: None,
            presence: None,
            requested: HashSet::new(),
            open_when_ready: None,
            menu: None,
            emoji: None,
            viewer: None,
            notice: None,
        };
        view.reload(cx);
        view.backend.send(Command::MarkRead {
            chat: view.chat.jid.clone(),
        });
        if view.chat.kind == ChatKind::User {
            view.backend.send(Command::WatchPresence {
                jid: view.chat.jid.clone(),
            });
        }
        view
    }

    pub fn focus_input(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn name_of(&mut self, jid: &str) -> String {
        if jid == "me" {
            return "You".to_string();
        }
        if let Some(name) = self.names.get(jid) {
            return name.clone();
        }
        let name = self
            .store
            .display_name(jid)
            .unwrap_or_else(|_| crate::model::display_number(jid));
        self.names.insert(jid.to_string(), name.clone());
        name
    }

    fn author_of(&mut self, row: &MsgRow) -> (String, String) {
        if row.from_me {
            ("me".to_string(), self.me_name.clone())
        } else {
            let seed = row.sender.clone().unwrap_or_else(|| self.chat.jid.clone());
            let name = self.name_of(&seed);
            (seed, name)
        }
    }

    fn quote_of(&mut self, original: &MsgRow) -> Quote {
        let (author_seed, author) = self.author_of(original);
        Quote {
            author,
            author_seed,
            text: preview_label(original.kind, original.text.as_deref()),
        }
    }

    /// Re-reads the loaded window of messages from the store and rebuilds the rows.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let rows = self
            .store
            .messages(&self.chat.jid, None, self.loaded)
            .unwrap_or_default();
        self.has_older = rows.len() >= self.loaded;
        let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
        let mut reactions: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for (id, sender, emoji) in self.store.reactions(&self.chat.jid, &ids).unwrap_or_default() {
            reactions.entry(id).or_default().push((sender, emoji));
        }
        let by_id: HashMap<String, MsgRow> = rows.iter().map(|r| (r.id.clone(), r.clone())).collect();

        // The "New" line goes before the first of the last `unread_at_open` incoming messages.
        let mut unread_from: Option<usize> = None;
        if self.unread_at_open > 0 {
            let mut remaining = self.unread_at_open;
            for (index, row) in rows.iter().enumerate().rev() {
                if row.from_me {
                    break;
                }
                unread_from = Some(index);
                remaining -= 1;
                if remaining == 0 {
                    break;
                }
            }
        }

        let mut items = Vec::with_capacity(rows.len() + 8);
        let mut previous: Option<&MsgRow> = None;
        let mut previous_day = i64::MIN;
        for (index, row) in rows.iter().enumerate() {
            let day = local_day(row.ts);
            let new_day = day != previous_day;
            if new_day {
                items.push(Item::Day(row.ts));
                previous_day = day;
            }
            let unread_here = unread_from == Some(index);
            if unread_here {
                items.push(Item::Unread);
            }

            let record = record_of(row);
            let (media, view_once) = media_of(&record);
            let (author_seed, author) = self.author_of(row);
            let notice = if row.kind == MsgKind::System {
                let store = self.store.clone();
                let me = self.me_jid.clone();
                let resolver = move |jid: &str| -> String {
                    let bare = content::bare_jid(jid);
                    let canonical = store.canonical(&bare).unwrap_or(bare);
                    if me.as_deref() == Some(canonical.as_str()) {
                        return "You".to_string();
                    }
                    store
                        .display_name(&canonical)
                        .unwrap_or_else(|_| crate::model::display_number(&canonical))
                };
                notice_of(&record, &resolver)
            } else {
                None
            };

            let quote = match row.quoted_id.as_deref() {
                Some(quoted_id) => Some(match by_id.get(quoted_id) {
                    Some(original) => self.quote_of(original),
                    None => match self.store.message(&self.chat.jid, quoted_id).ok().flatten() {
                        Some(original) => self.quote_of(&original),
                        None => Quote {
                            author: String::new(),
                            author_seed: String::new(),
                            text: "Original message is not on this device".to_string(),
                        },
                    },
                }),
                None => None,
            };

            // Formatting, links and mentions of the text or caption.
            let rich = row.text.as_deref().filter(|t| !t.is_empty()).map(|text| {
                let mentioned = record
                    .message
                    .as_option()
                    .and_then(content::digest)
                    .map(|d| d.mentions)
                    .unwrap_or_default();
                let mentions: Vec<Mention> = mentioned
                    .iter()
                    .map(|jid| {
                        let bare = content::bare_jid(jid);
                        let canonical = self.store.canonical(&bare).unwrap_or(bare.clone());
                        let name = if self.me_jid.as_deref() == Some(canonical.as_str()) {
                            self.me_name.clone()
                        } else {
                            self.name_of(&canonical)
                        };
                        Mention {
                            user: bare.split('@').next().unwrap_or_default().to_string(),
                            jid: canonical,
                            name,
                        }
                    })
                    .collect();
                richtext::parse(text, &mentions)
            });

            let same_run = previous.is_some_and(|p| {
                p.kind != MsgKind::System
                    && p.from_me == row.from_me
                    && p.sender == row.sender
                    && row.ts - p.ts < GROUP_GAP_SECS
            });
            let head = !same_run || new_day || unread_here || quote.is_some();

            let mut chips: Vec<ReactionChip> = Vec::new();
            if let Some(list) = reactions.get(&row.id) {
                for (sender, emoji) in list {
                    match chips.iter_mut().find(|c| c.emoji == *emoji) {
                        Some(chip) => {
                            chip.count += 1;
                            chip.mine |= sender == "me";
                        }
                        None => chips.push(ReactionChip {
                            emoji: emoji.clone(),
                            count: 1,
                            mine: sender == "me",
                        }),
                    }
                }
            }

            // Photos and stickers load by themselves, like the official client; larger
            // media waits for a click.
            if matches!(row.kind, MsgKind::Image | MsgKind::Sticker)
                && row.media_path.is_none()
                && !view_once
                && self.requested.insert(row.id.clone())
            {
                self.backend.send(Command::DownloadMedia {
                    chat: self.chat.jid.clone(),
                    id: row.id.clone(),
                });
            }

            let avatar = if row.from_me {
                self.me_jid.as_deref().and_then(crate::avatars::get)
            } else {
                crate::avatars::get(&author_seed)
            };

            items.push(Item::Msg(Box::new(MsgView {
                row: row.clone(),
                author,
                author_seed,
                head,
                quote,
                reactions: chips,
                media,
                notice,
                view_once,
                rich,
                avatar,
            })));
            previous = Some(row);
        }

        let old_count = self.list.item_count();
        self.items = items;
        if old_count == 0 {
            self.list.reset(self.items.len());
        } else {
            self.list.splice(0..old_count, self.items.len());
        }
        cx.notify();
    }

    fn load_older(&mut self, cx: &mut Context<Self>) {
        if self.has_older {
            self.loaded += PAGE;
            self.reload(cx);
        }
    }

    fn row(&self, id: &str) -> Option<&MsgRow> {
        self.items.iter().find_map(|item| match item {
            Item::Msg(view) if view.row.id == id => Some(&view.row),
            _ => None,
        })
    }

    pub fn set_activity(&mut self, sender: String, activity: Activity, cx: &mut Context<Self>) {
        self.activity = match activity {
            Activity::Idle => None,
            other => Some((sender, other)),
        };
        cx.notify();
    }

    pub fn set_presence(&mut self, online: bool, last_seen: Option<i64>, cx: &mut Context<Self>) {
        self.presence = Some(if online {
            "online".to_string()
        } else {
            match last_seen {
                Some(ts) => format!("last seen {}", super::widgets::header_time(ts).to_lowercase()),
                None => String::new(),
            }
        });
        cx.notify();
    }

    /// A download finished: open it if the user was waiting for it.
    pub fn media_ready(&mut self, id: &str, path: Option<String>, error: Option<String>, cx: &mut Context<Self>) {
        if self.open_when_ready.as_deref() == Some(id) {
            self.open_when_ready = None;
            match path {
                Some(path) => self.show_file(id, PathBuf::from(path), cx),
                None => {
                    self.notice = Some(format!(
                        "Could not download: {}",
                        error.unwrap_or_else(|| "unknown error".to_string())
                    ))
                }
            }
        }
        cx.notify();
    }

    fn show_file(&mut self, id: &str, path: PathBuf, cx: &mut Context<Self>) {
        let is_picture = self.row(id).is_some_and(|r| r.kind == MsgKind::Image);
        if is_picture {
            self.viewer = Some(path);
        } else {
            cx.open_with_system(&path);
        }
        cx.notify();
    }

    fn open_media(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(row) = self.row(id).cloned() else {
            return;
        };
        match row.media_path.as_ref().map(PathBuf::from).filter(|p| p.exists()) {
            Some(path) => self.show_file(id, path, cx),
            None => {
                self.open_when_ready = Some(id.to_string());
                self.notice = Some("Downloading…".to_string());
                self.backend.send(Command::DownloadMedia {
                    chat: self.chat.jid.clone(),
                    id: id.to_string(),
                });
                cx.notify();
            }
        }
    }

    fn typing_changed(&mut self, cx: &mut Context<Self>) {
        let has_text = !self.input.read(cx).value().trim().is_empty();
        self.backend.send(Command::Typing {
            chat: self.chat.jid.clone(),
            activity: if has_text {
                Activity::Typing
            } else {
                Activity::Idle
            },
        });
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        match self.editing.take() {
            Some(id) => self.backend.send(Command::Edit {
                chat: self.chat.jid.clone(),
                id,
                text: text.to_string(),
            }),
            None => self.backend.send(Command::SendText {
                chat: self.chat.jid.clone(),
                text: text.to_string(),
                reply_to: self.reply_to.take().map(|r| r.id),
                mentions: Vec::new(),
            }),
        }
        self.input.update(cx, |input, cx| input.set_value("", window, cx));
        self.unread_at_open = 0;
        cx.notify();
    }

    fn send_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths.into_iter().filter(|p| p.is_file()) {
            self.backend.send(Command::SendFile {
                chat: self.chat.jid.clone(),
                kind: attach_kind_for(&path),
                path,
                caption: None,
                reply_to: self.reply_to.take().map(|r| r.id),
            });
        }
        self.notice = Some("Sending…".to_string());
        cx.notify();
    }

    fn pick_files(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Send".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await {
                let _ = this.update(cx, |this, cx| this.send_files(paths, cx));
            }
        })
        .detach();
    }

    fn react(&mut self, id: &str, emoji: &str, cx: &mut Context<Self>) {
        self.backend.send(Command::React {
            chat: self.chat.jid.clone(),
            id: id.to_string(),
            emoji: emoji.to_string(),
        });
        self.menu = None;
        self.emoji = None;
        cx.notify();
    }

    fn pick_emoji(&mut self, emoji: &str, window: &mut Window, cx: &mut Context<Self>) {
        match self.emoji.take().map(|state| state.target) {
            Some(EmojiTarget::Message(id)) => self.react(&id, emoji, cx),
            _ => {
                let emoji = emoji.to_string();
                self.input.update(cx, |input, cx| input.insert(emoji, window, cx));
            }
        }
        cx.notify();
    }

    // ---- rows ------------------------------------------------------------------------------

    fn render_item(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        let Some(item) = self.items.get(index) else {
            return div().into_any_element();
        };
        let view = match item {
            Item::Day(ts) => return day_separator(day_label(*ts), cx),
            Item::Unread => return unread_separator(cx),
            Item::Msg(view) => view,
        };
        let id = view.row.id.clone();
        let entity = cx.entity().downgrade();
        let media_id = id.clone();
        let hooks = RowHooks {
            media: Some(Rc::new(move |_, _window, cx| {
                let _ = entity.update(cx, |this, cx| this.open_media(&media_id, cx));
            })),
        };
        let row = message_row(view, &hooks, cx);
        if matches!(view.row.kind, MsgKind::System | MsgKind::Revoked) {
            return row;
        }

        let hover = if t.dark { t.accent } else { t.muted };
        let mut toolbar = div()
            .absolute()
            .top(px(-14.))
            .right(px(16.))
            .invisible()
            .group_hover("row", |style| style.visible())
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(2.))
            .rounded(R_LG)
            .border_1()
            .border_color(t.border)
            .bg(t.popover)
            .shadow(t.shadow_raised());
        for (slot, emoji) in QUICK_REACTIONS.iter().take(3).enumerate() {
            let react_id = id.clone();
            toolbar = toolbar.child(
                div()
                    .id(("quick", index * 8 + slot))
                    .size(px(28.))
                    .rounded(R_MD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(16.))
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .child(*emoji)
                    .on_click(cx.listener(move |this, _, _, cx| this.react(&react_id, emoji, cx))),
            );
        }
        let tool = |name: &'static str, slot: usize| {
            div()
                .id(("tool", index * 8 + slot))
                .size(px(28.))
                .rounded(R_MD)
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.muted_foreground)
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .child(icon(name))
        };
        let reply_id = id.clone();
        let more_id = id.clone();
        let emoji_id = id.clone();
        toolbar = toolbar
            .child(tool("smile-plus", 0).tooltip(tip("React")).on_click(cx.listener(
                move |this, event: &gpui::ClickEvent, _, cx| {
                    this.emoji = Some(EmojiState {
                        target: EmojiTarget::Message(emoji_id.clone()),
                        at: event.position(),
                    });
                    cx.notify();
                },
            )))
            .child(tool("corner-up-left", 1).tooltip(tip("Reply")).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.reply_to = this.row(&reply_id).cloned();
                    this.editing = None;
                    this.focus_input(window, cx);
                    cx.notify();
                },
            )))
            .child(tool("ellipsis", 2).tooltip(tip("More")).on_click(cx.listener(
                move |this, event: &gpui::ClickEvent, _, cx| {
                    this.menu = Some(MenuState {
                        id: more_id.clone(),
                        at: event.position(),
                    });
                    cx.notify();
                },
            )));

        let menu_id = id.clone();
        div()
            .id(("row", index))
            .relative()
            .group("row")
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.menu = Some(MenuState {
                        id: menu_id.clone(),
                        at: event.position,
                    });
                    cx.notify();
                }),
            )
            .child(row)
            .child(toolbar)
            .into_any_element()
    }

    // ---- overlays --------------------------------------------------------------------------

    /// A floating panel at a point, with a window-sized backdrop that closes it.
    fn overlay(
        &self,
        at: Point<Pixels>,
        panel: impl IntoElement,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let size = window.viewport_size();
        let close = cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.menu = None;
            this.emoji = None;
            cx.notify();
        });
        let close_right = cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.menu = None;
            this.emoji = None;
            cx.notify();
        });
        deferred(
            anchored().position(point(px(0.), px(0.))).child(
                div()
                    .w(size.width)
                    .h(size.height)
                    .occlude()
                    .on_mouse_down(MouseButton::Left, close)
                    .on_mouse_down(MouseButton::Right, close_right)
                    .child(
                        anchored()
                            .position(at)
                            .snap_to_window_with_margin(px(8.))
                            .child(panel),
                    ),
            ),
        )
        .with_priority(10)
        .into_any_element()
    }

    fn menu_overlay(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.clone()?;
        let row = self.row(&menu.id)?.clone();
        let t = tokens(cx).clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let item = |slot: usize, name: &'static str, label: &'static str, danger: bool| {
            let accent = t.accent;
            let danger_fill = Tokens::with_alpha(t.destructive, if t.dark { 0.2 } else { 0.1 });
            div()
                .id(("menu-item", slot))
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
        let separator = || div().h(px(1.)).mx(px(-6.)).my(px(6.)).bg(t.border);

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

        // Quick reactions across the top, as in the official client.
        let mut quick = div().flex().items_center().justify_between().px(px(2.)).pb(px(4.));
        for (slot, emoji) in QUICK_REACTIONS.iter().enumerate() {
            let id = row.id.clone();
            let hover = t.accent;
            quick = quick.child(
                div()
                    .id(("menu-quick", slot))
                    .size(px(30.))
                    .rounded(R_MD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(18.))
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .child(*emoji)
                    .on_click(cx.listener(move |this, _, _, cx| this.react(&id, emoji, cx))),
            );
        }
        panel = panel.child(quick).child(separator());

        let id = row.id.clone();
        panel = panel.child(item(0, "corner-up-left", "Reply", false).on_click(cx.listener(
            move |this, _, window, cx| {
                this.reply_to = this.row(&id).cloned();
                this.editing = None;
                this.menu = None;
                this.focus_input(window, cx);
                cx.notify();
            },
        )));

        if let Some(text) = row.text.clone().filter(|t| !t.is_empty()) {
            panel = panel.child(item(1, "copy", "Copy", false).on_click(cx.listener(
                move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                    this.menu = None;
                    cx.notify();
                },
            )));
        }

        let id = row.id.clone();
        let starred = row.starred;
        panel = panel.child(
            item(2, "star", if starred { "Unstar" } else { "Star" }, false).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.backend.send(Command::Star {
                        chat: this.chat.jid.clone(),
                        id: id.clone(),
                        starred: !starred,
                    });
                    this.menu = None;
                    cx.notify();
                },
            )),
        );

        if row.kind.is_media() {
            let id = row.id.clone();
            panel = panel.child(item(3, "download", "Open", false).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.menu = None;
                    this.open_media(&id, cx);
                },
            )));
            if let Some(path) = row.media_path.clone() {
                panel = panel.child(item(4, "external-link", "Show in folder", false).on_click(
                    cx.listener(move |this, _, _, cx| {
                        cx.reveal_path(std::path::Path::new(&path));
                        this.menu = None;
                        cx.notify();
                    }),
                ));
            }
        }

        if row.from_me && row.kind == MsgKind::Text && now - row.ts < EDIT_WINDOW_SECS {
            let id = row.id.clone();
            let text = row.text.clone().unwrap_or_default();
            panel = panel.child(item(5, "pencil", "Edit", false).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.editing = Some(id.clone());
                    this.reply_to = None;
                    this.menu = None;
                    let text = text.clone();
                    this.input.update(cx, |input, cx| input.set_value(text, window, cx));
                    this.focus_input(window, cx);
                    cx.notify();
                },
            )));
        }

        panel = panel.child(separator());
        let id = row.id.clone();
        panel = panel.child(item(6, "trash", "Delete for me", true).on_click(cx.listener(
            move |this, _, _, cx| {
                this.backend.send(Command::DeleteForMe {
                    chat: this.chat.jid.clone(),
                    id: id.clone(),
                });
                this.menu = None;
                cx.notify();
            },
        )));
        if row.from_me {
            let id = row.id.clone();
            panel = panel.child(item(7, "trash", "Delete for everyone", true).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.backend.send(Command::Revoke {
                        chat: this.chat.jid.clone(),
                        id: id.clone(),
                    });
                    this.menu = None;
                    cx.notify();
                },
            )));
        }

        Some(self.overlay(menu.at, panel, window, cx))
    }

    fn emoji_overlay(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.emoji.clone()?;
        let t = tokens(cx).clone();
        let hover = t.accent;
        let mut grid = div().flex().flex_wrap().p(px(6.));
        for (slot, emoji) in EMOJI.iter().enumerate() {
            grid = grid.child(
                div()
                    .id(("emoji", slot))
                    .size(px(36.))
                    .rounded(R_MD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(22.))
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .child(*emoji)
                    .on_click(cx.listener(move |this, _, window, cx| this.pick_emoji(emoji, window, cx))),
            );
        }
        let panel = div()
            .w(px(336.))
            .h(px(380.))
            .rounded(R_XL)
            .border_1()
            .border_color(t.border)
            .bg(t.popover)
            .shadow(t.shadow_popover())
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(12.))
                    .pt(px(8.))
                    .pb(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.muted_foreground)
                    .child("Emoji"),
            )
            .child(div().id("emoji-scroll").flex_1().min_h_0().overflow_y_scroll().child(grid));
        // Open upward from the composer, downward from a message.
        let at = if state.target == EmojiTarget::Composer {
            point(state.at.x - px(300.), state.at.y - px(400.))
        } else {
            state.at
        };
        Some(self.overlay(at, panel, window, cx))
    }

    fn viewer_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let path = self.viewer.clone()?;
        let external = path.clone();
        Some(
            div()
                .id("viewer")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .bg(gpui::hsla(0., 0., 0., 0.88))
                .flex()
                .items_center()
                .justify_center()
                .p(px(32.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.viewer = None;
                    cx.notify();
                }))
                .child(
                    gpui::img(path)
                        .max_w_full()
                        .max_h_full()
                        .object_fit(gpui::ObjectFit::Contain),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(12.))
                        .right(px(12.))
                        .flex()
                        .gap(px(4.))
                        .text_color(gpui::white())
                        .child(
                            icon_button("viewer-open", "external-link", cx)
                                .tooltip(tip("Open in default app"))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.stop_propagation();
                                    cx.open_with_system(&external);
                                })),
                        )
                        .child(icon_button("viewer-close", "x", cx).tooltip(tip("Close"))),
                )
                .into_any_element(),
        )
    }

    // ---- header and composer ---------------------------------------------------------------

    fn header(&self, cx: &App) -> impl IntoElement {
        let t = tokens(cx);
        let subtitle = match (&self.activity, &self.presence) {
            (Some((sender, activity)), _) => {
                let verb = if *activity == Activity::Recording {
                    "recording audio…"
                } else {
                    "typing…"
                };
                if self.chat.kind == ChatKind::User {
                    verb.to_string()
                } else {
                    format!("{} is {verb}", self.names.get(sender).cloned().unwrap_or_default())
                }
            }
            (None, Some(presence)) => presence.clone(),
            (None, None) => String::new(),
        };
        let typing = self.activity.is_some();
        div()
            .h(HEADER_H)
            .flex_shrink_0()
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_b_1()
            .border_color(Tokens::with_alpha(t.border, t.border.a * 0.7))
            .child(avatar(
                &self.chat.jid,
                &self.chat.title,
                28.,
                crate::avatars::get(&self.chat.jid),
                cx,
            ))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.foreground)
                            .truncate()
                            .child(SharedString::from(self.chat.title.clone())),
                    )
                    .when(!subtitle.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(px(11.))
                                .text_color(if typing { t.primary } else { t.muted_foreground })
                                .child(subtitle),
                        )
                    }),
            )
            .child(div().flex_1())
            .when_some(self.notice.clone(), |this, notice| {
                this.child(
                    div()
                        .px(px(10.))
                        .h(px(24.))
                        .rounded_full()
                        .bg(t.fg(0.06))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .text_color(t.muted_foreground)
                        .child(notice),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(icon_button("call-video", "video", cx).tooltip(tip("Video call")))
                    .child(icon_button("call-voice", "phone", cx).tooltip(tip("Voice call")))
                    .child(icon_button("chat-search", "search", cx).tooltip(tip("Search")))
                    .child(icon_button("chat-more", "ellipsis-vertical", cx).tooltip(tip("Menu"))),
            )
    }

    fn composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx).clone();
        let has_text = !self.input.read(cx).value().trim().is_empty();
        let banner = if self.editing.is_some() {
            Some(("pencil", "Editing message".to_string(), String::new()))
        } else {
            self.reply_to.as_ref().map(|reply| {
                (
                    "corner-up-left",
                    "Replying to".to_string(),
                    preview_label(reply.kind, reply.text.as_deref()),
                )
            })
        };
        div().px(px(16.)).pb(px(20.)).flex_shrink_0().child(
            div()
                .rounded(R_XL)
                .border_1()
                .border_color(t.fg(0.08))
                .bg(t.card)
                .shadow(t.shadow_raised())
                .overflow_hidden()
                .flex()
                .flex_col()
                .when_some(banner, |this, (glyph, label, detail)| {
                    this.child(
                        div()
                            .h(px(32.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .border_b_1()
                            .border_color(t.border)
                            .text_size(px(12.))
                            .text_color(t.muted_foreground)
                            .child(icon(glyph).size(px(12.)))
                            .child(label)
                            .child(div().truncate().text_color(t.foreground).child(detail))
                            .child(div().flex_1())
                            .child(
                                div()
                                    .id("cancel-banner")
                                    .cursor_pointer()
                                    .child(icon("x").size(px(14.)))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if this.editing.take().is_some() {
                                            this.input.update(cx, |input, cx| input.set_value("", window, cx));
                                        }
                                        this.reply_to = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                })
                .child(
                    div()
                        .p(px(6.))
                        .flex()
                        .items_end()
                        .gap(px(4.))
                        .child(
                            icon_button("attach", "plus", cx)
                                .tooltip(tip("Attach files"))
                                .on_click(cx.listener(|this, _, _, cx| this.pick_files(cx))),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .py(px(1.))
                                .child(Textarea::new(&self.input).appearance(false).bordered(false)),
                        )
                        .child(
                            icon_button("emoji", "smile", cx)
                                .tooltip(tip("Emoji"))
                                .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                                    this.emoji = Some(EmojiState {
                                        target: EmojiTarget::Composer,
                                        at: event.position(),
                                    });
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("send")
                                .size(px(32.))
                                .flex_shrink_0()
                                .rounded(R_LG)
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .when(has_text, |this| this.bg(t.primary).text_color(t.primary_foreground))
                                .when(!has_text, |this| this.text_color(t.muted_foreground))
                                .child(icon(if has_text { "send-horizontal" } else { "mic" }))
                                .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
                        ),
                ),
        )
    }
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx).clone();
        let view = cx.entity();
        let older = self.has_older;
        let menu = self.menu_overlay(window, cx);
        let emoji = self.emoji_overlay(window, cx);
        let viewer = self.viewer_overlay(cx);
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(t.background)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.send_files(paths.paths().to_vec(), cx);
            }))
            .child(self.header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .when(older, |this| {
                        this.child(
                            div().flex().justify_center().py(px(6.)).child(
                                div()
                                    .id("load-older")
                                    .px(px(12.))
                                    .h(px(28.))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(t.border)
                                    .flex()
                                    .items_center()
                                    .text_size(px(12.))
                                    .text_color(t.muted_foreground)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(t.fg(0.06)))
                                    .child("Load earlier messages")
                                    .on_click(cx.listener(|this, _, _, cx| this.load_older(cx))),
                            ),
                        )
                    })
                    .child(
                        list(self.list.clone(), move |index, _window, cx| {
                            view.update(cx, |this, cx| this.render_item(index, cx))
                        })
                        .flex_1()
                        .min_h_0()
                        .pb(px(20.)),
                    ),
            )
            .child(self.composer(cx))
            .children(viewer)
            .children(menu)
            .children(emoji)
    }
}
