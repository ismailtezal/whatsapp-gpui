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
use super::theme::{HEADER_H, R_LG, R_MD, R_XL, Surface, Tokens, tokens};
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

/// Emoji the system font is sure to have: Windows 11 draws Unicode 15, not later ones.
fn drawable(emoji: &emojis::Emoji) -> bool {
    emoji.unicode_version().major() <= 15
}

pub struct ChatView {
    /// Heights of the floating top and bottom bars in the Liquid Glass style, measured
    /// when they are laid out, so the message list can leave room beneath them.
    insets: Rc<std::cell::Cell<(f32, f32)>>,
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
    audio: Option<crate::audio::AudioPlayer>,
    forwarding: Option<String>,
    forward_targets: Vec<ChatRow>,
    /// ID of the own message whose receipts are being shown.
    info_for: Option<String>,
    /// IDs of the pinned messages of this chat, newest pin first.
    /// Whether the info panel lists every member or only the first ten.
    members_expanded: bool,
    /// The identity and appearance stamp of each list item, to tell the list exactly
    /// which rows changed on a reload.
    item_keys: Vec<(String, u64)>,
    /// Emoji picker: the search box, the group shown, the emoji listed, and the recent ones.
    emoji_query: Entity<gpui_component::input::InputState>,
    emoji_group: usize,
    emoji_shown: Rc<Vec<&'static str>>,
    emoji_recent: Vec<&'static str>,
    /// The voice message being recorded, if any.
    recorder: Option<crate::voice::Recorder>,
    pins: Vec<String>,
    /// Wording of the first pinned message, read when the chat is loaded.
    pin_text: Option<String>,
    video: Option<VideoPlay>,
    /// A video to start on the next frame: (message ID, file, whether it loops).
    pending_video: Option<(String, PathBuf, bool)>,
    /// Files picked, dropped or pasted that wait for Enter before they are sent.
    pending: Vec<PathBuf>,
    /// When the typing indicator was last sent, to avoid sending it on every keystroke.
    typing_sent: Option<std::time::Instant>,
    /// People picked from the mention list: (name shown, JID).
    mention_map: Vec<(String, String)>,
    /// Where the sticker picker is anchored while it is open.
    stickers_at: Option<Point<Pixels>>,
    sticker_choices: Vec<PathBuf>,
    member_menu: Option<(String, Point<Pixels>)>,
    adding_members: bool,
    add_candidates: Vec<crate::model::ContactRow>,
    confirm_leave: bool,
    edit_field: Option<GroupField>,
    edit_input: Entity<gpui_component::input::InputState>,
    info_open: bool,
    /// For a channel: its role, followers and mute, as last read from the server.
    channel: Option<crate::events::ChannelInfo>,
    /// A group's switches as last read from the server (kept in the store).
    group_settings: Option<crate::events::GroupSettings>,
    /// The disappearing-message timer of this chat, in seconds.
    timer: i64,
    /// People waiting to be let into this group.
    join_requests: Vec<String>,
    /// Group members as (JID, name, rank).
    members: Vec<(String, String, i32)>,
    gallery: Vec<PathBuf>,
    search_open: bool,
    search: Entity<gpui_component::input::InputState>,
    search_hits: Vec<MsgRow>,
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
        let draft = chat.draft.clone().unwrap_or_default();
        let emoji_query = cx.new(|cx| gpui_component::input::InputState::new(window, cx).placeholder("Search emoji"));
        cx.subscribe_in(&emoji_query, window, |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                this.refresh_emoji(cx);
                cx.notify();
            }
        })
        .detach();
        // Recently used emoji are remembered across runs.
        let emoji_recent: Vec<&'static str> = store
            .kv("emoji-recent")
            .ok()
            .flatten()
            .unwrap_or_default()
            .split(' ')
            .filter_map(|text| emojis::get(text).map(|emoji| emoji.as_str()))
            .collect();
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 14)
                .submit_on_enter(true)
                .placeholder(placeholder)
                .default_value(draft)
        });
        cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| match event {
            InputEvent::PressEnter { shift: false, .. } => this.send(window, cx),
            InputEvent::Change => {
                this.typing_tick(cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();

        let search = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx).placeholder("Search in this chat")
        });
        cx.subscribe_in(&search, window, |this, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.run_search(cx);
            }
        })
        .detach();

        let edit_input = cx.new(|cx| gpui_component::input::InputState::new(window, cx));
        let unread_at_open = chat.unread;
        let mut view = ChatView {
            backend,
            store,
            chat,
            me_name,
            me_jid,
            items: Vec::new(),
            list: ListState::new(0, ListAlignment::Bottom, px(1200.)),
            insets: Rc::new(std::cell::Cell::new((64., 84.))),
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
            audio: None,
            forwarding: None,
            forward_targets: Vec::new(),
            info_for: None,
            members_expanded: false,
            item_keys: Vec::new(),
            emoji_query,
            emoji_group: 1,
            emoji_shown: Rc::new(Vec::new()),
            emoji_recent,
            recorder: None,
            pins: Vec::new(),
            pin_text: None,
            video: None,
            pending_video: None,
            pending: Vec::new(),
            typing_sent: None,
            mention_map: Vec::new(),
            stickers_at: None,
            sticker_choices: Vec::new(),
            member_menu: None,
            adding_members: false,
            add_candidates: Vec::new(),
            confirm_leave: false,
            edit_field: None,
            edit_input,
            info_open: false,
            channel: None,
            group_settings: None,
            timer: 0,
            join_requests: Vec::new(),
            members: Vec::new(),
            gallery: Vec::new(),
            search_open: false,
            search,
            search_hits: Vec::new(),
        };
if view.chat.kind == ChatKind::Newsletter {
            view.backend.send(Command::LoadChannel {
                chat: view.chat.jid.clone(),
                older: false,
            });
        }
        view.reload(cx);
        if view.chat.kind != ChatKind::User {
            // Members are needed for @-mentions; ask the server for the current list.
            view.backend.send(Command::RefreshGroup {
                chat: view.chat.jid.clone(),
            });
        }
        view.backend.send(Command::MarkRead {
            chat: view.chat.jid.clone(),
        });
        // Development aid: start with the info panel open.
        // Development aid: start playing the newest downloaded GIF of this chat.
        if std::env::var("WA_PLAY_GIF").is_ok() {
            let newest = view.items.iter().rev().find_map(|item| match item {
                Item::Msg(msg) if msg.row.kind == MsgKind::Gif => {
                    msg.row.media_path.clone().map(|path| (msg.row.id.clone(), path))
                }
                _ => None,
            });
            if let Some((id, path)) = newest {
                view.pending_video = Some((id, PathBuf::from(path), true));
            }
        }
        if std::env::var("WA_INFO").is_ok() {
            view.toggle_info(cx);
        }
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
        let _perf = crate::perf::scope("chat.reload");
        self.pins = self.store.pinned(&self.chat.jid).unwrap_or_default();
        // Read once per reload, not per frame.
        if self.chat.kind == ChatKind::Newsletter {
            self.channel = self
                .store
                .kv(&crate::events::ChannelInfo::key(&self.chat.jid))
                .ok()
                .flatten()
                .and_then(|text| crate::events::ChannelInfo::decode(&text));
        }
        self.timer = self.store.chat_timer(&self.chat.jid).unwrap_or(0);
        self.group_settings = self
            .store
            .kv(&crate::events::GroupSettings::key(&self.chat.jid))
            .ok()
            .flatten()
            .and_then(|text| crate::events::GroupSettings::decode(&text));
        self.pin_text = self
            .pins
            .first()
            .and_then(|id| self.store.message(&self.chat.jid, id).ok().flatten())
            .map(|message| preview_label(message.kind, message.text.as_deref()));
        if self.info_open || self.chat.kind != ChatKind::User {
            self.refresh_members();
        }
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
        // Channels report totals per emoji instead of who reacted.
        let mut reaction_totals: HashMap<String, Vec<(String, i64)>> = HashMap::new();
        if self.chat.kind == ChatKind::Newsletter {
            for (id, emoji, count) in self.store.reaction_counts(&self.chat.jid, &ids).unwrap_or_default() {
                reaction_totals.entry(id).or_default().push((emoji, count));
            }
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
        let mut skip_to = 0usize;
        let mut missing_previews: Vec<std::path::PathBuf> = Vec::new();
        let mut missing_views: Vec<std::path::PathBuf> = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            if index < skip_to {
                continue;
            }
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
            let (mut media, view_once) = media_of(&record);
            if matches!(row.kind, MsgKind::Video | MsgKind::Gif | MsgKind::VideoNote)
                && let Some(file) = &row.media_path
            {
                let preview = crate::thumbs::thumb_path(std::path::Path::new(file));
                if preview.exists() {
                    media.thumb_file = Some(preview);
                } else if self.requested.insert(format!("thumb:{}", row.id)) {
                    missing_previews.push(std::path::PathBuf::from(file));
                }
            }
            if row.kind == MsgKind::Image
                && let Some(file) = &row.media_path
            {
                let copy = crate::thumbs::view_path(std::path::Path::new(file));
                if copy.exists() {
                    media.display = Some(copy);
                } else if self.requested.insert(format!("view:{}", row.id)) {
                    missing_views.push(std::path::PathBuf::from(file));
                }
            }
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
            if let Some(totals) = reaction_totals.get(&row.id) {
                for (emoji, total) in totals {
                    match chips.iter_mut().find(|chip| chip.emoji == *emoji) {
                        // The total already counts my own reaction.
                        Some(chip) => chip.count = (*total).max(1) as usize,
                        None => chips.push(ReactionChip {
                            emoji: emoji.clone(),
                            count: (*total).max(1) as usize,
                            mine: false,
                        }),
                    }
                }
            }

            // Photos and stickers load by themselves, like the official client; larger
            // media waits for a click.
            // Photos, stickers, GIFs and smaller videos load by themselves.
            let auto = matches!(row.kind, MsgKind::Image | MsgKind::Sticker | MsgKind::Gif)
                || (row.kind == MsgKind::Video && media.file_size <= 16 * 1024 * 1024);
            if auto
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

            // Photos sent together (same author, within a minute and a half, no captions)
            // are shown as one grid; the ones after the first are folded into it.
            let mut album = Vec::new();
            if row.kind == MsgKind::Image
                && !view_once
                && row.text.as_deref().is_none_or(str::is_empty)
                && quote.is_none()
            {
                for next in rows.iter().skip(index + 1) {
                    let joins = next.kind == MsgKind::Image
                        && next.from_me == row.from_me
                        && next.sender == row.sender
                        && next.quoted_id.is_none()
                        && next.text.as_deref().is_none_or(str::is_empty)
                        && (next.ts - row.ts).abs() <= 90
                        && local_day(next.ts) == day;
                    if !joins || album.len() >= 9 {
                        break;
                    }
                    if next.media_path.is_none() && self.requested.insert(next.id.clone()) {
                        self.backend.send(Command::DownloadMedia {
                            chat: self.chat.jid.clone(),
                            id: next.id.clone(),
                        });
                    }
                    // Tiles show the display copy; the viewer opens the original by ID.
                    let shown = next.media_path.as_ref().map(|file| {
                        let file = std::path::Path::new(file);
                        let copy = crate::thumbs::view_path(file);
                        if !copy.exists() && self.requested.insert(format!("view:{}", next.id)) {
                            missing_views.push(file.to_path_buf());
                        }
                        crate::thumbs::display_path(file).to_string_lossy().into_owned()
                    });
                    album.push((next.id.clone(), shown));
                }
                skip_to = index + 1 + album.len();
            }

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
                group: self.chat.kind != ChatKind::User,
                album,
                extras: super::message::extras_of(&record),
                poll: if row.kind == MsgKind::Poll {
                    self.store.poll(&self.chat.jid, &row.id, &row.proto)
                } else {
                    None
                },
                rich,
                avatar,
            })));
            previous = Some(row);
        }

        // Videos downloaded before previews existed get theirs made in the background.
        if !missing_previews.is_empty() || !missing_views.is_empty() {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .spawn(async move {
                        for video in missing_previews {
                            let _ = crate::thumbs::make_video_thumb(&video);
                        }
                        for photo in missing_views {
                            let _ = crate::thumbs::make_view_copy(&photo);
                        }
                    })
                    .await;
                let _ = this.update(cx, |this, cx| this.reload(cx));
            })
            .detach();
        }

        // Tell the list only what changed. Replacing everything would throw away every
        // measured row height and move a scrolled-up view back to the top of the page
        // each time a tick, a reaction or a finished download touches the chat.
        let keys: Vec<(String, u64)> = items.iter().map(item_key).collect();
        let old = std::mem::take(&mut self.item_keys);
        self.items = items;
        if old.is_empty() {
            self.list.reset(keys.len());
        } else {
            let same = |a: &(String, u64), b: &(String, u64)| a.0 == b.0;
            let prefix = old.iter().zip(&keys).take_while(|(a, b)| same(a, b)).count();
            let room = old.len().min(keys.len()) - prefix;
            let suffix = old
                .iter()
                .rev()
                .zip(keys.iter().rev())
                .take(room)
                .take_while(|(a, b)| same(a, b))
                .count();
            // Rows that are still there but look different (a reaction added, a photo
            // downloaded) are measured again, one by one.
            for index in 0..prefix {
                if old[index].1 != keys[index].1 {
                    self.list.splice(index..index + 1, 1);
                }
            }
            for back in 1..=suffix {
                let (before, after) = (old.len() - back, keys.len() - back);
                if old[before].1 != keys[after].1 {
                    // Positions are still the old ones here: the middle is replaced last.
                    self.list.splice(before..before + 1, 1);
                }
            }
            let (removed, added) = (old.len() - prefix - suffix, keys.len() - prefix - suffix);
            if removed > 0 || added > 0 {
                self.list.splice(prefix..prefix + removed, added);
            }
        }
        self.item_keys = keys;
        cx.notify();
    }

    /// Shows an older page: from the local store while it has more, else by asking the phone.
    fn load_older(&mut self, cx: &mut Context<Self>) {
        self.loaded += PAGE;
        if self.has_older {
            self.reload(cx);
        } else if self.chat.kind == ChatKind::Newsletter {
            // A channel's earlier posts come from the server, a page at a time.
            self.backend.send(Command::LoadChannel {
                chat: self.chat.jid.clone(),
                older: true,
            });
            self.notice = Some("Getting earlier posts…".to_string());
            cx.notify();
        } else {
            self.backend.send(Command::LoadOlder {
                chat: self.chat.jid.clone(),
            });
            self.notice = Some("Asked your phone for earlier messages…".to_string());
            cx.notify();
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
        match self.row(id).map(|r| r.kind) {
            Some(MsgKind::Image) => self.viewer = Some(path),
            Some(MsgKind::Voice | MsgKind::Audio) => self.play_audio(id, &path, cx),
            Some(kind @ (MsgKind::Video | MsgKind::Gif | MsgKind::VideoNote)) => {
                self.pending_video = Some((id.to_string(), path, kind == MsgKind::Gif));
            }
            None if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("jpg" | "jpeg" | "png" | "webp")
            ) =>
            {
                self.viewer = Some(path)
            }
            _ => cx.open_with_system(&path),
        }
        cx.notify();
    }

    /// Plays a voice message or audio file in the app; a second click pauses it.
    fn play_audio(&mut self, id: &str, path: &std::path::Path, cx: &mut Context<Self>) {
        if self.audio.is_none() {
            match crate::audio::AudioPlayer::new() {
                Ok(player) => self.audio = Some(player),
                Err(error) => {
                    self.notice = Some(format!("Audio is not available: {error}"));
                    cx.open_with_system(path);
                    return;
                }
            }
        }
        let Some(player) = self.audio.as_mut() else {
            return;
        };
        let same = player.current() == Some(id);
        let outcome = if same && !player.finished() {
            player.toggle().map(|_| ())
        } else {
            player.play(id, path)
        };
        if let Err(error) = outcome {
            self.notice = Some(format!("Could not play this: {error}"));
            return;
        }
        self.notice = None;
        // Keep the progress bar moving while something plays.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let keep_going = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.audio.as_ref().is_some_and(|player| player.is_playing())
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
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

    #[allow(dead_code)]
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
        if !self.pending.is_empty() {
            // Staged files go out together; what is typed becomes the first one's caption.
            let caption = (!text.is_empty()).then(|| text.to_string());
            let reply = self.reply_to.take().map(|r| r.id);
            for (index, path) in std::mem::take(&mut self.pending).into_iter().enumerate() {
                self.backend.send(Command::SendFile {
                    chat: self.chat.jid.clone(),
                    kind: attach_kind_for(&path),
                    path,
                    caption: if index == 0 { caption.clone() } else { None },
                    reply_to: if index == 0 { reply.clone() } else { None },
                });
            }
            self.input.update(cx, |input, cx| input.set_value("", window, cx));
            self.notice = Some("Sending…".to_string());
            cx.notify();
            return;
        }
        if text.is_empty() {
            return;
        }
        if text.starts_with("/poll") {
            let text = text.to_string();
            if self.try_poll_command(&text) {
                self.input.update(cx, |input, cx| input.set_value("", window, cx));
            }
            cx.notify();
            return;
        }
        let (resolved, mentions) = {
            let typed = text.to_string();
            self.resolve_mentions(&typed)
        };
        match self.editing.take() {
            Some(id) => self.backend.send(Command::Edit {
                chat: self.chat.jid.clone(),
                id,
                text: text.to_string(),
            }),
            None => self.backend.send(Command::SendText {
                chat: self.chat.jid.clone(),
                text: resolved,
                reply_to: self.reply_to.take().map(|r| r.id),
                mentions,
            }),
        }
        self.input.update(cx, |input, cx| input.set_value("", window, cx));
        self.unread_at_open = 0;
        self.typing_sent = None;
        cx.notify();
    }

    fn send_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        // Files are staged for review; Enter (or the send button) sends them.
        self.pending.extend(paths.into_iter().filter(|p| p.is_file()));
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

    /// Works out which emoji the picker lists: the matches of the search, or the group.
    fn refresh_emoji(&mut self, cx: &mut Context<Self>) {
        let query = self.emoji_query.read(cx).value().trim().to_lowercase();
        let shown: Vec<&'static str> = if !query.is_empty() {
            emojis::iter()
                .filter(|emoji| drawable(emoji))
                .filter(|emoji| {
                    emoji.name().contains(&query) || emoji.shortcodes().any(|code| code.contains(&query))
                })
                .map(|emoji| emoji.as_str())
                .collect()
        } else if self.emoji_group == 0 {
            self.emoji_recent.clone()
        } else {
            emojis::Group::iter()
                .nth(self.emoji_group - 1)
                .map(|group| {
                    group
                        .emojis()
                        .filter(|emoji| drawable(emoji))
                        .map(|emoji| emoji.as_str())
                        .collect()
                })
                .unwrap_or_default()
        };
        self.emoji_shown = Rc::new(shown);
    }

    /// Puts an emoji at the front of the recent ones and saves them.
    fn remember_emoji(&mut self, emoji: &str) {
        let Some(emoji) = emojis::get(emoji).map(|emoji| emoji.as_str()) else {
            return;
        };
        self.emoji_recent.retain(|recent| *recent != emoji);
        self.emoji_recent.insert(0, emoji);
        self.emoji_recent.truncate(36);
        let _ = self.store.set_kv("emoji-recent", &self.emoji_recent.join(" "));
    }

    fn pick_emoji(&mut self, emoji: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_emoji(emoji);
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
        let _perf = crate::perf::scope("chat.item");
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
            audio: self.audio.as_ref().and_then(|player| player.playback()),
            rate: self.audio.as_ref().map(|player| player.rate() as f32).unwrap_or(1.),
            seek: Some({
                let entity = cx.entity().downgrade();
                Rc::new(move |id: String, fraction: f32, _: &mut Window, cx: &mut App| {
                    let _ = entity.update(cx, |this, cx| this.seek_audio(&id, fraction, cx));
                })
            }),
            speed: Some({
                let entity = cx.entity().downgrade();
                Rc::new(move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                    let _ = entity.update(cx, |this, cx| this.step_audio_speed(cx));
                }) as super::message::ClickFn
            }),
            media_id: Some({
                let entity = cx.entity().downgrade();
                Rc::new(move |picture: String, _: &mut Window, cx: &mut App| {
                    let _ = entity.update(cx, |this, cx| this.open_picture(&picture, cx));
                })
            }),
            open_chat: Some({
                let entity = cx.entity().downgrade();
                Rc::new(move |jid: String, _: &mut Window, cx: &mut App| {
                    let _ = entity.update(cx, |_, cx| cx.emit(ChatEvent::OpenChat(jid)));
                })
            }),
            poll_vote: Some({
                let entity = cx.entity().downgrade();
                let poll_id = id.clone();
                Rc::new(move |index: usize, _: &mut Window, cx: &mut App| {
                    let _ = entity.update(cx, |this, cx| this.vote(&poll_id, index, cx));
                })
            }),
            quote: view.row.quoted_id.clone().map(|quoted| {
                let entity = cx.entity().downgrade();
                Rc::new(move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                    let _ = entity.update(cx, |this, cx| this.jump_to(&quoted, cx));
                }) as super::message::ClickFn
            }),
        };
        let hover = if t.dark { t.accent } else { t.muted };
        let mut toolbar = div()
            .flex_none()
            .h(px(34.))
            .when(!std::env::var("WA_HOVER").is_ok_and(|on| !on.is_empty()), |bar| bar.invisible())
            .group_hover("row", |style| style.visible())
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(2.))
            .rounded(R_LG)
            .border_1()
            .border_color(t.border)
            .bg(t.popover_fill())
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

        // Notices and deleted messages have no actions; everything else gets the bar.
        let plain = matches!(view.row.kind, MsgKind::System | MsgKind::Revoked);
        let row = message_row(view, &hooks, (!plain).then(|| toolbar.into_any_element()), cx);
        if plain {
            return row;
        }
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
            .bg(t.popover_fill())
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

        if row.from_me {
            let info_id = row.id.clone();
            panel = panel.child(item(9, "info", "Message info", false).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.info_for = Some(info_id.clone());
                    this.menu = None;
                    cx.notify();
                },
            )));
        }

        let pin_id = row.id.clone();
        let is_pinned = self.pins.contains(&row.id);
        panel = panel.child(
            item(10, "pin", if is_pinned { "Unpin" } else { "Pin" }, false).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.backend.send(Command::PinMessage {
                        chat: this.chat.jid.clone(),
                        id: pin_id.clone(),
                        pinned: !is_pinned,
                    });
                    this.menu = None;
                    cx.notify();
                },
            )),
        );

        let forward_id = row.id.clone();
        panel = panel.child(item(8, "forward", "Forward", false).on_click(cx.listener(
            move |this, _, _, cx| this.start_forward(&forward_id, cx),
        )));

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
        // The list is worked out when the picker first opens.
        if self.emoji_shown.is_empty() && self.emoji_group != 0 {
            self.refresh_emoji(cx);
        }
        let t = tokens(cx).clone();
        let hover = t.accent;
        // Group tabs: recent first, then the standard groups.
        const TABS: [(&str, &str); 10] = [
            ("\u{1f552}", "Recent"),
            ("\u{1f600}", "Smileys"),
            ("\u{1f44b}", "People"),
            ("\u{1f43b}", "Animals and nature"),
            ("\u{1f354}", "Food and drink"),
            ("\u{2708}\u{fe0f}", "Travel and places"),
            ("\u{26bd}", "Activities"),
            ("\u{1f4a1}", "Objects"),
            ("\u{1f523}", "Symbols"),
            ("\u{1f3f3}\u{fe0f}", "Flags"),
        ];
        let searching = !self.emoji_query.read(cx).value().trim().is_empty();
        let mut tabs = div().px(px(6.)).pb(px(4.)).flex().flex_shrink_0();
        for (index, (glyph, name)) in TABS.iter().enumerate() {
            let active = !searching && index == self.emoji_group;
            tabs = tabs.child(
                div()
                    .id(("emoji-tab", index))
                    .flex_1()
                    .h(px(30.))
                    .rounded(R_MD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(16.))
                    .cursor_pointer()
                    .when(active, |this| this.bg(t.tint(0.14)))
                    .hover(move |style| style.bg(hover))
                    .child(*glyph)
                    .tooltip(tip(*name))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.emoji_group = index;
                        this.emoji_query.update(cx, |input, cx| input.set_value("", window, cx));
                        this.refresh_emoji(cx);
                        cx.notify();
                    })),
            );
        }
        // Nine to a row; only the rows on screen are built.
        const PER_ROW: usize = 9;
        let shown = self.emoji_shown.clone();
        let rows = shown.len().div_ceil(PER_ROW);
        let grid: AnyElement = if shown.is_empty() {
            div()
                .flex_1()
                .p(px(16.))
                .text_size(px(13.))
                .text_color(t.muted_foreground)
                .child(if searching {
                    "No emoji matches."
                } else {
                    "Emoji you use will show here."
                })
                .into_any_element()
        } else {
            gpui::uniform_list(
                "emoji-grid",
                rows,
                cx.processor(move |_this, range: std::ops::Range<usize>, _window, cx| {
                    range
                        .map(|row| {
                            let mut line = div().h(px(36.)).px(px(6.)).flex();
                            for (column, emoji) in shown.iter().skip(row * PER_ROW).take(PER_ROW).enumerate() {
                                let emoji: &'static str = emoji;
                                line = line.child(
                                    div()
                                        .id(("emoji", row * PER_ROW + column))
                                        .size(px(36.))
                                        .rounded(R_MD)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(22.))
                                        .cursor_pointer()
                                        .hover(move |style| style.bg(hover))
                                        .child(emoji)
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.pick_emoji(emoji, window, cx)
                                        })),
                                );
                            }
                            line
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .flex_1()
            .min_h_0()
            .into_any_element()
        };
        let panel = div()
            .w(px(336.))
            .h(px(420.))
            .rounded(R_XL)
            .border_1()
            .border_color(t.border)
            .bg(t.popover_fill())
            .shadow(t.shadow_popover())
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .p(px(8.))
                    .flex_shrink_0()
                    .child(gpui_component::input::Input::new(&self.emoji_query).cleanable(true)),
            )
            .child(tabs)
            .child(grid);
        // Open upward from the composer, downward from a message.
        let at = if state.target == EmojiTarget::Composer {
            point(state.at.x - px(300.), state.at.y - px(440.))
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

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx).clone();
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
            (None, None) => match &self.channel {
                Some(channel) => format!(
                    "Channel · {} followers",
                    super::message::short_count(channel.followers as usize)
                ),
                None => String::new(),
            },
        };
        let typing = self.activity.is_some();
        div()
            .h(HEADER_H)
            .flex_shrink_0()
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(10.))
            .when(!t.liquid(), |this| {
                this.border_b_1()
                    .border_color(Tokens::with_alpha(t.border, t.border.a * 0.7))
            })
            .when(t.liquid(), |this| {
                this.rounded(px(20.)).bg(t.surface(Surface::Bar, t.card))
            })
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
                    .child(
                        icon_button("call-video", "video", cx)
                            .tooltip(tip("Video call"))
                            .on_click(cx.listener(|this, _, _, cx| this.calls_unavailable(cx))),
                    )
                    .child(
                        icon_button("call-voice", "phone", cx)
                            .tooltip(tip("Voice call"))
                            .on_click(cx.listener(|this, _, _, cx| this.start_call(cx))),
                    )
                    .child(
                        icon_button("chat-search", "search", cx)
                            .tooltip(tip("Search in chat"))
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_search(window, cx))),
                    )
                    .child(
                        icon_button("chat-info", "info", cx)
                            .tooltip(tip("Chat info"))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_info(cx))),
                    ),
            )
    }

    fn composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx).clone();
        let has_text = !self.input.read(cx).value().trim().is_empty() || !self.pending.is_empty();
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
        let liquid = t.liquid();
        // A channel is read-only unless it is ours: the message box gives way to a bar.
        if self.chat.kind == ChatKind::Newsletter && !self.channel.as_ref().is_some_and(|c| c.can_post()) {
            return div()
                .px(px(if liquid { 10. } else { 16. }))
                .pb(px(if liquid { 10. } else { 20. }))
                .flex_shrink_0()
                .child(self.channel_bar(cx));
        }
        div()
            .px(px(if liquid { 10. } else { 16. }))
            .pb(px(if liquid { 10. } else { 20. }))
            .flex_shrink_0()
            .child(
            div()
                .rounded(if liquid { px(22.) } else { R_XL })
                .border_1()
                .border_color(if liquid { gpui::transparent_black() } else { t.fg(0.08) })
                .bg(t.surface(Surface::Bar, t.card))
                .when(!liquid, |this| this.shadow(t.shadow_raised()))
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
                .children(self.pending_tray(cx))
                .children(self.recording_row(cx))
                .child(
                    div()
                        .p(px(6.))
                        .flex()
                        .items_end()
                        .gap(px(4.))
                        // Hidden while recording (after `flex`, which would undo it).
                        .when(self.recorder.is_some(), |this| this.hidden())
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
                                .child(Textarea::new(&self.input).appearance(false).bordered(false).on_paste({
                                    let entity = cx.entity().downgrade();
                                    move |item, _window, cx| {
                                        entity
                                            .update(cx, |this, cx| this.stage_pasted(item, cx))
                                            .unwrap_or(false)
                                    }
                                })),
                        )
                        .child(
                            icon_button("stickers", "sticker", cx)
                                .tooltip(tip("Stickers"))
                                .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                                    this.sticker_choices = this
                                        .store
                                        .sticker_paths(40)
                                        .unwrap_or_default()
                                        .into_iter()
                                        .map(PathBuf::from)
                                        .filter(|path| path.exists())
                                        .collect();
                                    this.stickers_at = Some(event.position());
                                    cx.notify();
                                })),
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
                                .when(has_text, |this| {
                                    this.bg(t.primary).text_color(t.primary_foreground).shadow(t.glow())
                                })
                                .when(!has_text, |this| this.text_color(t.muted_foreground))
                                .child(icon(if has_text { "send-horizontal" } else { "mic" }))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if has_text {
                                        this.send(window, cx)
                                    } else {
                                        this.start_recording(cx)
                                    }
                                })),
                        ),
                ),
        )
    }
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _perf = crate::perf::scope("chat.render");
        if self.recorder.is_some() {
            // The timer and the level meter move while recording.
            window.request_animation_frame();
        }
        crate::perf::note(|| {
            let top = self.list.logical_scroll_top();
            format!(
                "scroll item {} of {} offset {:.0}",
                top.item_ix,
                self.list.item_count(),
                top.offset_in_item / px(1.)
            )
        });
        if crate::perf::auto_scroll() {
            // Measurement aid: keep the list moving so frame times can be read.
            self.list.scroll_by(px(crate::perf::scroll_step()));
            window.request_animation_frame();
        }
        let t = tokens(cx).clone();
        let view = cx.entity();
        let older = self.has_older;
        let menu = self.menu_overlay(window, cx);
        let emoji = self.emoji_overlay(window, cx);
        if let Some((id, path, looping)) = self.pending_video.take() {
            self.start_video(&id, path, looping, window, cx);
        }
        let viewer = self.viewer_overlay(cx);
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(t.background)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    // Escape closes whatever is open on top, one layer at a time.
                    if this.viewer.take().is_some()
                        || this.menu.take().is_some()
                        || this.emoji.take().is_some()
                        || this.stickers_at.take().is_some()
                        || this.forwarding.take().is_some()
                        || this.info_for.take().is_some()
                        || this.member_menu.take().is_some()
                        || this.edit_field.take().is_some()
                    {
                        cx.notify();
                    } else if this.adding_members || this.search_open {
                        this.adding_members = false;
                        this.search_open = false;
                        this.search_hits.clear();
                        cx.notify();
                    } else if this.reply_to.take().is_some() {
                        cx.notify();
                    }
                }
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.send_files(paths.paths().to_vec(), cx);
            }))
            .when(!t.liquid(), |this| {
                this.child(self.header(cx))
                    .children(self.search_bar(cx))
                    .children(self.pinned_bar(cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(if t.liquid() {
                        self.floating_column(older, view, &t, cx)
                    } else {
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(self.messages_area(older, view, &t, None, cx))
                            .children(self.mention_popup(cx))
                            .child(self.composer(cx))
                            .into_any_element()
                    })
                    .children(self.info_panel(cx)),
            )
            .children(viewer)
            .children(self.video_overlay(cx))
            .children(self.forward_overlay(window, cx))
            .children(self.info_overlay(window, cx))
            .children(self.sticker_overlay(window, cx))
            .children(self.group_edit_overlay(window, cx))
            .children(self.add_members_overlay(window, cx))
            .children(self.member_menu_overlay(window, cx))
            .children(menu)
            .children(emoji)
    }
}

impl ChatView {
    /// The message list, or the search results while a search is active.
    fn messages_area(
        &self,
        older: bool,
        view: Entity<Self>,
        t: &Tokens,
        floating: Option<(f32, f32)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = t.clone();
        // Room left for bars floating over the list (Liquid Glass style).
        let (pad_top, pad_bottom) = floating.unwrap_or((0., 20.));
        if self.search_open && !self.search_hits.is_empty() {
            let mut results = div()
                .id("chat-search-results")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(8.))
                .p(px(12.))
                .when(floating.is_some(), |this| this.pt(px(pad_top)).pb(px(pad_bottom)));
            for (slot, hit) in self.search_hits.iter().enumerate() {
                let id = hit.id.clone();
                let hover = t.fg(0.15);
                results = results.child(
                    div()
                        .id(("search-hit", slot))
                        .rounded(R_XL)
                        .border_1()
                        .border_color(t.border)
                        .bg(t.card)
                        .p(px(12.))
                        .cursor_pointer()
                        .hover(move |style| style.border_color(hover))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(t.muted_foreground)
                                .child(super::widgets::header_time(hit.ts)),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(t.fg(0.8))
                                .child(SharedString::from(preview_label(hit.kind, hit.text.as_deref()))),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.jump_to(&id, cx))),
                );
            }
            return results.into_any_element();
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .when(floating.is_none(), |this| {
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
                                    .child(if older {
                                "Load earlier messages"
                            } else {
                                "Get earlier messages from your phone"
                            })
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
                        .pt(px(pad_top))
                        .pb(px(pad_bottom)),
                    )
            .into_any_element()
    }

    /// The conversation column in the Liquid Glass style: the messages fill it, and the
    /// header, pinned bar and message box float over them as glass, so messages pass
    /// beneath and are bent by the glass.
    fn floating_column(
        &self,
        older: bool,
        view: Entity<Self>,
        t: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (top, bottom) = self.insets.get();
        let measure = |cell: Rc<std::cell::Cell<(f32, f32)>>, is_top: bool| {
            gpui::canvas(
                move |bounds, window, _cx| {
                    let height = bounds.size.height / px(1.);
                    let (top, bottom) = cell.get();
                    let old = if is_top { top } else { bottom };
                    if (old - height).abs() > 0.5 {
                        cell.set(if is_top { (height, bottom) } else { (top, height) });
                        window.request_animation_frame();
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full()
        };
        let glass_bar = |bar: AnyElement| {
            div()
                .rounded(px(16.))
                .overflow_hidden()
                .bg(t.surface(Surface::Bar, t.card))
                .child(bar)
        };
        let older_pill = div().flex().justify_center().child(
            div()
                .id("load-older")
                .px(px(12.))
                .h(px(26.))
                .rounded_full()
                .bg(t.surface(Surface::Chip, t.card))
                .flex()
                .items_center()
                .text_size(px(12.))
                .text_color(t.fg(0.8))
                .cursor_pointer()
                .child(if older {
                    "Load earlier messages"
                } else {
                    "Get earlier messages from your phone"
                })
                .on_click(cx.listener(|this, _, _, cx| this.load_older(cx))),
        );
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .child(self.messages_area(older, view, t, Some((top + 4., bottom + 6.)), cx)),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .p(px(10.))
                    .pb(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(self.header(cx))
                    .children(self.search_bar(cx).map(glass_bar))
                    .children(self.pinned_bar(cx).map(glass_bar))
                    .child(older_pill)
                    .child(measure(self.insets.clone(), true)),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .children(self.mention_popup(cx))
                    .child(self.composer(cx))
                    .child(measure(self.insets.clone(), false)),
            )
            .into_any_element()
    }

    /// The video button: video calls are not built yet.
    fn calls_unavailable(&mut self, cx: &mut Context<Self>) {
        self.notice = Some("Video calls are not available yet. Use the phone button for a voice call.".to_string());
        cx.notify();
    }

    /// The phone button: calls the person of this chat.
    fn start_call(&mut self, cx: &mut Context<Self>) {
        if self.chat.kind == ChatKind::User {
            self.backend.send(Command::StartCall {
                chat: self.chat.jid.clone(),
            });
        } else {
            self.notice = Some("Group calls are not available yet.".to_string());
        }
        cx.notify();
    }

    fn toggle_info(&mut self, cx: &mut Context<Self>) {
        self.info_open = !self.info_open;
        self.members_expanded = false;
        if self.info_open && self.chat.kind != ChatKind::User {
            // Ask the server for the current members; the panel refreshes when they arrive.
            self.backend.send(Command::RefreshGroup {
                chat: self.chat.jid.clone(),
            });
        }
        if self.info_open {
            let mut members = Vec::new();
            if self.chat.kind != ChatKind::User {
                for (jid, rank) in self.store.group_members(&self.chat.jid).unwrap_or_default() {
                    let name = if self.me_jid.as_deref() == Some(jid.as_str()) {
                        "You".to_string()
                    } else {
                        self.name_of(&jid)
                    };
                    members.push((jid, name, rank));
                }
            }
            self.members = members;
            self.gallery = self
                .store
                .photo_paths(&self.chat.jid, 9)
                .unwrap_or_default()
                .into_iter()
                .map(PathBuf::from)
                .filter(|path| path.exists())
                .map(|path| crate::thumbs::display_path(&path))
                .collect();
        }
        cx.notify();
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = !self.search_open;
        if self.search_open {
            self.search.update(cx, |input, cx| input.focus(window, cx));
        } else {
            self.search_hits.clear();
        }
        cx.notify();
    }

    fn run_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().to_string();
        self.search_hits = if query.trim().chars().count() >= 2 {
            self.store
                .search_messages(&query, Some(&self.chat.jid), 60)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        cx.notify();
    }

    /// Scrolls to a message, loading older pages first when it is not in view yet.
    fn jump_to(&mut self, id: &str, cx: &mut Context<Self>) {
        self.search_open = false;
        self.search_hits.clear();
        if self.row(id).is_none() {
            match self.store.newer_than(&self.chat.jid, id).ok().flatten() {
                Some(newer) => {
                    self.loaded = (newer as usize + 20).max(self.loaded);
                    self.reload(cx);
                }
                None => {
                    self.notice = Some("That message is not on this device".to_string());
                    cx.notify();
                    return;
                }
            }
        }
        let index = self
            .items
            .iter()
            .position(|item| matches!(item, Item::Msg(view) if view.row.id == id));
        if let Some(index) = index {
            self.list.scroll_to(gpui::ListOffset {
                item_ix: index,
                offset_in_item: px(0.),
            });
        }
        cx.notify();
    }

    fn search_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.search_open {
            return None;
        }
        let t = tokens(cx).clone();
        let count = self.search_hits.len();
        Some(
            div()
                .px(px(16.))
                .py(px(8.))
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(8.))
                .border_b_1()
                .border_color(Tokens::with_alpha(t.border, t.border.a * 0.7))
                .child(
                    div().flex_1().child(
                        gpui_component::input::Input::new(&self.search)
                            .prefix(icon("search").size(px(14.)).text_color(t.muted_foreground))
                            .cleanable(true),
                    ),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.muted_foreground)
                        .child(if count == 0 {
                            "Type to search".to_string()
                        } else {
                            format!("{count} result{}", if count == 1 { "" } else { "s" })
                        }),
                )
                .child(
                    icon_button("search-close", "x", cx)
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_search(window, cx))),
                )
                .into_any_element(),
        )
    }

    /// The contact or group details shown at the right of the conversation.
    fn info_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.info_open {
            return None;
        }
        let t = tokens(cx).clone();
        let is_person = self.chat.kind == ChatKind::User;
        let subtitle = if is_person {
            crate::model::display_number(&self.chat.jid)
        } else {
            format!("Group · {} members", self.members.len())
        };
        let about = if is_person {
            self.store
                .contact(&self.chat.jid)
                .ok()
                .flatten()
                .and_then(|contact| contact.about)
        } else {
            self.store
                .chat_details(&self.chat.jid)
                .ok()
                .flatten()
                .and_then(|(description, _, _)| description)
        };
        let section = |text: &'static str| {
            div()
                .px(px(8.))
                .pt(px(16.))
                .pb(px(6.))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.muted_foreground)
                .child(text)
        };
        let action = |id: &'static str, glyph: &'static str, label: &'static str| {
            let hover = t.fg(0.06);
            div()
                .id(id)
                .h(px(36.))
                .px(px(8.))
                .rounded(R_LG)
                .flex()
                .items_center()
                .gap(px(10.))
                .text_size(px(14.))
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .child(icon(glyph).text_color(t.muted_foreground))
                .child(label)
        };
        let muted = self.chat.mute_until != 0;
        let pinned = self.chat.pinned > 0;
        let archived = self.chat.archived;
        let jid = self.chat.jid.clone();

        let mut panel = div()
            .id("chat-info")
            .w(px(288.))
            .h_full()
            .flex_shrink_0()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(Tokens::with_alpha(t.border, t.border.a * 0.7))
            .px(px(8.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .child(
                div()
                    .pt(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(avatar(
                        &self.chat.jid,
                        &self.chat.title,
                        96.,
                        crate::avatars::get(&self.chat.jid),
                        cx,
                    ))
                    .child(
                        div()
                            .px(px(8.))
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(self.chat.title.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(t.muted_foreground)
                            .child(subtitle),
                    ),
            );
        if let Some(about) = about.filter(|a| !a.trim().is_empty()) {
            panel = panel
                .child(section(if is_person { "About" } else { "Description" }))
                .child(
                    div()
                        .px(px(8.))
                        .text_size(px(14.))
                        .text_color(t.fg(0.9))
                        .child(SharedString::from(about)),
                );
        }
        if !self.gallery.is_empty() {
            let mut grid = div().px(px(8.)).flex().flex_wrap().gap(px(6.));
            for (slot, path) in self.gallery.iter().enumerate() {
                let open = path.clone();
                grid = grid.child(
                    div()
                        .id(("gallery", slot))
                        .size(px(82.))
                        .rounded(R_MD)
                        .overflow_hidden()
                        .cursor_pointer()
                        .child(
                            gpui::img(path.clone())
                                .size(px(82.))
                                .object_fit(gpui::ObjectFit::Cover),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.viewer = Some(open.clone());
                            cx.notify();
                        })),
                );
            }
            panel = panel.child(section("Photos")).child(grid);
        }

        fn send(
            command: Command,
            cx: &mut Context<ChatView>,
        ) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
            cx.listener(move |this: &mut ChatView, _: &gpui::ClickEvent, _, cx| {
                // Reflect the change at once; the store and list follow from the event.
                match &command {
                    Command::Mute { until, .. } => this.chat.mute_until = if until.is_some() { -1 } else { 0 },
                    Command::Pin { pinned, .. } => this.chat.pinned = i64::from(*pinned),
                    Command::Archive { archived, .. } => this.chat.archived = *archived,
                    _ => {}
                }
                this.backend.send(command.clone());
                cx.notify();
            })
        }
        panel = panel
            .child(section("Actions"))
            .child(
                action("info-mute", "bell-off", if muted { "Unmute" } else { "Mute notifications" }).on_click(
                    send(
                        Command::Mute {
                            chat: jid.clone(),
                            until: if muted { None } else { Some(0) },
                        },
                        cx,
                    ),
                ),
            )
            .child(
                action("info-pin", "pin", if pinned { "Unpin chat" } else { "Pin chat" }).on_click(send(
                    Command::Pin {
                        chat: jid.clone(),
                        pinned: !pinned,
                    },
                    cx,
                )),
            )
            .child(
                action(
                    "info-archive",
                    "archive",
                    if archived { "Unarchive chat" } else { "Archive chat" },
                )
                .on_click(send(
                    Command::Archive {
                        chat: jid.clone(),
                        archived: !archived,
                    },
                    cx,
                )),
            );

        if is_person {
            panel = panel.children(self.person_actions(cx));
        }
        if !is_person {
            panel = panel.children(self.group_actions(cx));
        }
        panel = panel.children(self.settings_rows(cx));

        if !self.members.is_empty() {
            panel = panel.child(section("Members"));
            // Ten members at first, as WhatsApp shows them; a large group's full list is
            // built only when asked for.
            let shown = if self.members_expanded { usize::MAX } else { 10 };
            for (slot, (member, name, rank)) in self.members.iter().enumerate().take(shown) {
                let hover = t.fg(0.06);
                panel = panel.child(
                    div()
                        .id(("member", slot))
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener({
                                let member = member.clone();
                                move |this, event: &MouseDownEvent, _, cx| {
                                    this.member_menu = Some((member.clone(), event.position));
                                    cx.notify();
                                }
                            }),
                        )
                        .on_click(cx.listener({
                            let member = member.clone();
                            move |this, event: &gpui::ClickEvent, _, cx| {
                                this.member_menu = Some((member.clone(), event.position()));
                                cx.notify();
                            }
                        }))
                        .min_h(px(52.))
                        .px(px(8.))
                        .py(px(6.))
                        .rounded(R_LG)
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .gap(px(12.))
                        .hover(move |style| style.bg(hover))
                        .child(avatar(member, name, 38., crate::avatars::get(member), cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(SharedString::from(name.clone())),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(t.muted_foreground)
                                        .child(crate::model::display_number(member)),
                                ),
                        )
                        .when(*rank > 0, |this| {
                            this.child(
                                div()
                                    .px(px(6.))
                                    .h(px(18.))
                                    .rounded(px(4.))
                                    .bg(t.tint(0.12))
                                    .flex()
                                    .items_center()
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.primary)
                                    .child("Admin"),
                            )
                        }),
                );
            }
            let hidden = self.members.len().saturating_sub(shown);
            if hidden > 0 {
                let hover = t.fg(0.06);
                panel = panel.child(
                    div()
                        .id("members-all")
                        .h(px(40.))
                        .px(px(8.))
                        .rounded(R_MD)
                        .flex()
                        .items_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.primary)
                        .cursor_pointer()
                        .hover(move |style| style.bg(hover))
                        .child(format!("View all ({hidden} more)"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.members_expanded = true;
                            cx.notify();
                        })),
                );
            }
        }
        Some(panel.into_any_element())
    }
}

impl ChatView {
    /// The account name and number arrived (or changed) after this view was opened.
    pub fn set_me(&mut self, name: String, jid: Option<String>, cx: &mut Context<Self>) {
        self.me_name = name;
        self.me_jid = jid;
        self.reload(cx);
    }

    fn start_forward(&mut self, id: &str, cx: &mut Context<Self>) {
        self.forwarding = Some(id.to_string());
        self.forward_targets = self.store.chats(false).unwrap_or_default();
        self.menu = None;
        cx.notify();
    }

    /// A row of a picker: picture and name, 44px tall.
    fn pick_row(
        &self,
        id: impl Into<gpui::ElementId>,
        jid: &str,
        title: &str,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let hover = tokens(cx).accent;
        div()
            .id(id)
            .h(px(44.))
            .px(px(8.))
            .rounded(R_MD)
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .child(avatar(jid, title, 28., crate::avatars::get(jid), cx))
            .child(
                div()
                    .truncate()
                    .text_size(px(14.))
                    .child(SharedString::from(title.to_string())),
            )
    }

    /// The chat picker for forwarding a message.
    fn forward_overlay(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.forwarding.clone()?;
        let t = tokens(cx).clone();
        let size = window.viewport_size();
        // Only the rows on screen are built, however many chats there are.
        let rows = gpui::uniform_list(
            "forward-list",
            self.forward_targets.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .filter_map(|slot| {
                        let chat = this.forward_targets.get(slot)?;
                        let to = chat.jid.clone();
                        let message = id.clone();
                        Some(
                            div().h(px(46.)).px(px(6.)).child(
                                this.pick_row(("forward-to", slot), &chat.jid, &chat.title, cx)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.backend.send(Command::Forward {
                                            from_chat: this.chat.jid.clone(),
                                            id: message.clone(),
                                            to_chats: vec![to.clone()],
                                        });
                                        this.forwarding = None;
                                        this.notice = Some("Forwarded".to_string());
                                        cx.notify();
                                    })),
                            ),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .flex_1()
        .min_h_0()
        .py(px(6.));
        let panel = div()
            .w(px(420.))
            .h(px(520.))
            .rounded(super::theme::R_2XL)
            .border_1()
            .border_color(t.border)
            .bg(t.popover_fill())
            .shadow(t.shadow_dialog())
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(t.border)
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Forward to"),
            )
            .child(rows);
        let close = cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.forwarding = None;
            cx.notify();
        });
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .w(size.width)
                        .h(size.height)
                        .occlude()
                        .bg(gpui::hsla(0., 0., 0., 0.55))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(MouseButton::Left, close)
                        .child(panel),
                ),
            )
            .with_priority(10)
            .into_any_element(),
        )
    }
}

impl ChatView {
    /// Re-reads the member list shown in the info panel.
    fn refresh_members(&mut self) {
        let mut members = Vec::new();
        if self.chat.kind != ChatKind::User {
            for (jid, rank) in self.store.group_members(&self.chat.jid).unwrap_or_default() {
                let name = if self.me_jid.as_deref() == Some(jid.as_str()) {
                    "You".to_string()
                } else {
                    self.name_of(&jid)
                };
                members.push((jid, name, rank));
            }
        }
        self.members = members;
    }
}

impl ChatView {
    /// Picks (or un-picks) an option of a poll and sends the resulting vote.
    fn vote(&mut self, poll_id: &str, index: usize, cx: &mut Context<Self>) {
        let Some(poll) = self.items.iter().find_map(|item| match item {
            Item::Msg(view) if view.row.id == poll_id => view.poll.clone(),
            _ => None,
        }) else {
            return;
        };
        let Some(clicked) = poll.options.get(index) else {
            return;
        };
        let mut picked: Vec<String> = poll
            .options
            .iter()
            .filter(|option| option.mine)
            .map(|option| option.name.clone())
            .collect();
        if clicked.mine {
            picked.retain(|name| *name != clicked.name);
        } else if poll.selectable == 1 {
            picked = vec![clicked.name.clone()];
        } else {
            picked.push(clicked.name.clone());
        }
        self.backend.send(Command::VotePoll {
            chat: self.chat.jid.clone(),
            id: poll_id.to_string(),
            options: picked,
        });
        cx.notify();
    }

    /// `/poll Question | Option A | Option B` in the composer creates a poll.
    fn try_poll_command(&mut self, text: &str) -> bool {
        let Some(spec) = text.strip_prefix("/poll ") else {
            return false;
        };
        let parts: Vec<String> = spec
            .split('|')
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect();
        match parts.split_first() {
            Some((question, options)) if options.len() >= 2 => {
                self.backend.send(Command::CreatePoll {
                    chat: self.chat.jid.clone(),
                    question: question.clone(),
                    options: options.to_vec(),
                    multiple: false,
                });
                true
            }
            _ => {
                self.notice = Some("Poll: /poll Question | Option A | Option B".to_string());
                false
            }
        }
    }
}

/// What a conversation asks of the window around it.
#[derive(Clone, Debug)]
pub enum ChatEvent {
    /// Open the chat with this JID (from a contact card or a member row).
    OpenChat(String),
}

impl gpui::EventEmitter<ChatEvent> for ChatView {}

impl ChatView {
    /// The text left unsent in the composer, to keep as a draft.
    pub fn draft(&self, cx: &App) -> String {
        if self.editing.is_some() {
            return String::new();
        }
        self.input.read(cx).value().to_string()
    }

    /// Who has received and read one of our messages.
    fn info_overlay(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.info_for.clone()?;
        let t = tokens(cx).clone();
        let size = window.viewport_size();
        let receipts = self.store.receipts(&self.chat.jid, &id).unwrap_or_default();
        let stamp = |ts: Option<i64>| ts.filter(|ts| *ts > 0).map(super::widgets::header_time);
        let mut rows = div()
            .id("info-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(6.));
        if receipts.is_empty() {
            rows = rows.child(
                div()
                    .p(px(16.))
                    .text_size(px(13.))
                    .text_color(t.muted_foreground)
                    .child("No delivery or read receipts have arrived for this message yet."),
            );
        }
        for (user, delivered, read, played) in receipts {
            let name = self
                .store
                .display_name(&user)
                .unwrap_or_else(|_| crate::model::display_number(&user));
            let state = match (stamp(played), stamp(read), stamp(delivered)) {
                (Some(at), _, _) => format!("Played {at}"),
                (None, Some(at), _) => format!("Read {at}"),
                (None, None, Some(at)) => format!("Delivered {at}"),
                _ => "Sent".to_string(),
            };
            rows = rows.child(
                div()
                    .h(px(52.))
                    .px(px(8.))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(10.))
                    .child(avatar(&user, &name, 36., crate::avatars::get(&user), cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(SharedString::from(name)),
                            )
                            .child(div().text_size(px(12.)).text_color(t.muted_foreground).child(state)),
                    ),
            );
        }
        let panel = div()
            .w(px(420.))
            .h(px(440.))
            .rounded(super::theme::R_2XL)
            .border_1()
            .border_color(t.border)
            .bg(t.popover_fill())
            .shadow(t.shadow_dialog())
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(t.border)
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Message info"),
            )
            .child(rows);
        let close = cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.info_for = None;
            cx.notify();
        });
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .w(size.width)
                        .h(size.height)
                        .occlude()
                        .bg(gpui::hsla(0., 0., 0., 0.55))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(MouseButton::Left, close)
                        .child(panel),
                ),
            )
            .with_priority(10)
            .into_any_element(),
        )
    }
}

/// Which group text is being edited in the dialog.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupField {
    Subject,
    Description,
}

impl ChatView {
    fn i_am_admin(&self) -> bool {
        self.members
            .iter()
            .any(|(jid, _, rank)| *rank > 0 && self.me_jid.as_deref() == Some(jid.as_str()))
    }

    fn start_group_edit(&mut self, field: GroupField, window: &mut Window, cx: &mut Context<Self>) {
        let current = match field {
            GroupField::Subject => self.chat.title.clone(),
            GroupField::Description => self
                .store
                .chat_details(&self.chat.jid)
                .ok()
                .flatten()
                .and_then(|(description, _, _)| description)
                .unwrap_or_default(),
        };
        self.edit_input.update(cx, |input, cx| {
            input.set_value(current, window, cx);
            input.focus(window, cx);
        });
        self.edit_field = Some(field);
        cx.notify();
    }

    fn save_group_edit(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.edit_field.take() else {
            return;
        };
        let text = self.edit_input.read(cx).value().trim().to_string();
        match field {
            GroupField::Subject if !text.is_empty() => {
                self.chat.title = text.clone();
                self.backend.send(Command::SetSubject {
                    chat: self.chat.jid.clone(),
                    subject: text,
                });
            }
            GroupField::Description => self.backend.send(Command::SetDescription {
                chat: self.chat.jid.clone(),
                description: text,
            }),
            _ => {}
        }
        cx.notify();
    }

    fn change_member(&mut self, member: &str, change: crate::events::MemberChange, cx: &mut Context<Self>) {
        self.backend.send(Command::ChangeMembers {
            chat: self.chat.jid.clone(),
            members: vec![member.to_string()],
            change,
        });
        self.member_menu = None;
        self.adding_members = false;
        cx.notify();
    }

    /// A dimmed full-window layer with a centred dialog; clicking outside closes it.
    fn dialog(
        &self,
        panel: gpui::Div,
        window: &Window,
        close: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> AnyElement {
        let size = window.viewport_size();
        deferred(
            anchored().position(point(px(0.), px(0.))).child(
                div()
                    .w(size.width)
                    .h(size.height)
                    .occlude()
                    .bg(gpui::hsla(0., 0., 0., 0.55))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(MouseButton::Left, close)
                    .child(panel.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())),
            ),
        )
        .with_priority(10)
        .into_any_element()
    }

    fn dialog_panel(&self, title: &'static str, width: f32, cx: &App) -> gpui::Div {
        let t = tokens(cx);
        div()
            .w(px(width))
            .rounded(super::theme::R_2XL)
            .border_1()
            .border_color(t.border)
            .bg(t.popover_fill())
            .shadow(t.shadow_dialog())
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(t.border)
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
    }

    /// Rename the group or edit its description.
    fn group_edit_overlay(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let field = self.edit_field?;
        let t = tokens(cx).clone();
        let title = match field {
            GroupField::Subject => "Rename group",
            GroupField::Description => "Group description",
        };
        let panel = self.dialog_panel(title, 420., cx).child(
            div()
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(gpui_component::input::Input::new(&self.edit_input))
                .child(
                    div().flex().justify_end().child(
                        div()
                            .id("group-edit-save")
                            .h(px(32.))
                            .px(px(14.))
                            .rounded(R_LG)
                            .flex()
                            .items_center()
                            .bg(t.primary)
                            .text_color(t.primary_foreground)
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .child("Save")
                            .on_click(cx.listener(|this, _, _, cx| this.save_group_edit(cx))),
                    ),
                ),
        );
        let close = cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.edit_field = None;
            cx.notify();
        });
        Some(self.dialog(panel, window, close))
    }

    /// Pick a saved contact to add to the group.
    fn add_members_overlay(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.adding_members {
            return None;
        }
        let already: Vec<&str> = self.members.iter().map(|(jid, _, _)| jid.as_str()).collect();
        let candidates: Vec<usize> = self
            .add_candidates
            .iter()
            .enumerate()
            .filter(|(_, contact)| !already.contains(&contact.jid.as_str()))
            .map(|(index, _)| index)
            .collect();
        // Only the rows on screen are built, however many contacts there are.
        let rows = gpui::uniform_list(
            "add-members-list",
            candidates.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .filter_map(|slot| {
                        let contact = this.add_candidates.get(*candidates.get(slot)?)?;
                        let name = contact
                            .name
                            .clone()
                            .unwrap_or_else(|| crate::model::display_number(&contact.jid));
                        let member = contact.jid.clone();
                        Some(
                            div().h(px(46.)).px(px(6.)).child(
                                this.pick_row(("add-member", slot), &contact.jid, &name, cx).on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.change_member(&member, crate::events::MemberChange::Add, cx)
                                    }),
                                ),
                            ),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .h(px(440.))
        .py(px(6.));
        let panel = self.dialog_panel("Add a member", 420., cx).child(rows);
        let close = cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.adding_members = false;
            cx.notify();
        });
        Some(self.dialog(panel, window, close))
    }

    /// Right-click menu of a member in the info panel.
    fn member_menu_overlay(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::events::MemberChange;
        let (member, at) = self.member_menu.clone()?;
        let t = tokens(cx).clone();
        let rank = self
            .members
            .iter()
            .find(|(jid, _, _)| *jid == member)
            .map(|(_, _, rank)| *rank)
            .unwrap_or(0);
        let item = |slot: usize, name: &'static str, label: &'static str, danger: bool| {
            let accent = t.accent;
            let danger_fill = Tokens::with_alpha(t.destructive, if t.dark { 0.2 } else { 0.1 });
            div()
                .id(("member-menu-item", slot))
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
        let open = member.clone();
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
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                item(0, "message-square", "Message", false).on_click(cx.listener(move |this, _, _, cx| {
                    this.member_menu = None;
                    cx.emit(ChatEvent::OpenChat(open.clone()));
                    cx.notify();
                })),
            );
        if self.i_am_admin() && self.me_jid.as_deref() != Some(member.as_str()) {
            let (label, change) = if rank > 0 {
                ("Dismiss as admin", MemberChange::Demote)
            } else {
                ("Make group admin", MemberChange::Promote)
            };
            let target = member.clone();
            panel = panel.child(
                item(1, "shield", label, false)
                    .on_click(cx.listener(move |this, _, _, cx| this.change_member(&target, change, cx))),
            );
            let target = member.clone();
            panel = panel
                .child(div().h(px(1.)).mx(px(-6.)).my(px(6.)).bg(t.border))
                .child(item(2, "user-minus", "Remove from group", true).on_click(cx.listener(
                    move |this, _, _, cx| this.change_member(&target, MemberChange::Remove, cx),
                )));
        }
        let size = window.viewport_size();
        let close = cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.member_menu = None;
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
                        .child(anchored().position(at).snap_to_window_with_margin(px(8.)).child(panel)),
                ),
            )
            .with_priority(10)
            .into_any_element(),
        )
    }

    /// The group-only rows of the info panel's action list.
    fn group_actions(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let t = tokens(cx).clone();
        let action = |id: &'static str, glyph: &'static str, label: &'static str, danger: bool| {
            let hover = t.fg(0.06);
            div()
                .id(id)
                .h(px(36.))
                .px(px(8.))
                .rounded(R_LG)
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(10.))
                .text_size(px(14.))
                .cursor_pointer()
                .text_color(if danger { t.destructive } else { t.foreground })
                .hover(move |style| style.bg(hover))
                .child(icon(glyph).text_color(if danger { t.destructive } else { t.muted_foreground }))
                .child(label)
        };
        let mut rows = Vec::new();
        if self.i_am_admin() {
            rows.push(
                action("info-rename", "pencil", "Rename group", false)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_group_edit(GroupField::Subject, window, cx)
                    }))
                    .into_any_element(),
            );
            rows.push(
                action("info-describe", "file-text", "Edit description", false)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_group_edit(GroupField::Description, window, cx)
                    }))
                    .into_any_element(),
            );
            rows.push(
                action("info-photo", "image-plus", "Change group photo", false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let chosen = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some("Use as group photo".into()),
                        });
                        let chat = this.chat.jid.clone();
                        cx.spawn(async move |this, cx| {
                            if let Ok(Ok(Some(paths))) = chosen.await
                                && let Some(path) = paths.into_iter().next()
                            {
                                let _ = this.update(cx, |this, cx| {
                                    this.backend.send(Command::SetGroupPhoto {
                                        chat,
                                        path: Some(path),
                                    });
                                    cx.notify();
                                });
                            }
                        })
                        .detach();
                    }))
                    .into_any_element(),
            );
            rows.push(
                action("info-invite", "link", "Copy invite link", false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.backend.send(Command::GroupInvite {
                            chat: this.chat.jid.clone(),
                            reset: false,
                        });
                        this.notice = Some("Getting the invite link…".to_string());
                        cx.notify();
                    }))
                    .into_any_element(),
            );
            rows.push(
                action("info-invite-reset", "refresh-cw", "Reset invite link", false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.backend.send(Command::GroupInvite {
                            chat: this.chat.jid.clone(),
                            reset: true,
                        });
                        this.notice = Some("Making a new invite link…".to_string());
                        cx.notify();
                    }))
                    .into_any_element(),
            );
            rows.push(
                action("info-add", "user-plus", "Add a member", false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.adding_members = true;
                        this.add_candidates = this.store.contacts().unwrap_or_default();
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        rows.push(
            action(
                "info-leave",
                "log-out",
                if self.confirm_leave {
                    "Click again to leave"
                } else {
                    "Leave group"
                },
                true,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if this.confirm_leave {
                    this.backend.send(Command::LeaveGroup {
                        chat: this.chat.jid.clone(),
                    });
                    this.confirm_leave = false;
                } else {
                    this.confirm_leave = true;
                }
                cx.notify();
            }))
            .into_any_element(),
        );
        rows
    }
}

impl ChatView {
    /// The `@query` being typed at the end of the message box, if any.
    fn mention_query(&self, cx: &App) -> Option<String> {
        if self.chat.kind == ChatKind::User {
            return None;
        }
        let text = self.input.read(cx).value().to_string();
        let at = text.rfind('@')?;
        let before_ok = text[..at].chars().last().is_none_or(char::is_whitespace);
        let query = &text[at + 1..];
        (before_ok && !query.contains(char::is_whitespace)).then(|| query.to_lowercase())
    }

    /// Members matching the `@query`, best first.
    fn mention_candidates(&self, cx: &App) -> Vec<(String, String)> {
        let Some(query) = self.mention_query(cx) else {
            return Vec::new();
        };
        self.members
            .iter()
            .filter(|(jid, name, _)| {
                self.me_jid.as_deref() != Some(jid.as_str()) && name.to_lowercase().contains(&query)
            })
            .take(6)
            .map(|(jid, name, _)| (jid.clone(), name.clone()))
            .collect()
    }

    /// Replaces the `@query` being typed with the chosen person's name.
    fn pick_mention(&mut self, jid: &str, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let Some(at) = text.rfind('@') else {
            return;
        };
        let completed = format!("{}@{name} ", &text[..at]);
        self.input.update(cx, |input, cx| input.set_value(completed, window, cx));
        if !self.mention_map.iter().any(|(_, known)| known == jid) {
            self.mention_map.push((name.to_string(), jid.to_string()));
        }
        self.focus_input(window, cx);
        cx.notify();
    }

    /// Turns the `@Name` texts picked from the list into the `@number` form the protocol
    /// uses, and returns who was mentioned.
    fn resolve_mentions(&mut self, text: &str) -> (String, Vec<String>) {
        let mut out = text.to_string();
        let mut mentioned = Vec::new();
        for (name, jid) in std::mem::take(&mut self.mention_map) {
            let shown = format!("@{name}");
            if out.contains(&shown) {
                let user = jid.split('@').next().unwrap_or_default();
                out = out.replace(&shown, &format!("@{user}"));
                mentioned.push(jid);
            }
        }
        (out, mentioned)
    }

    /// The list of people shown above the message box while typing `@`.
    fn mention_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let candidates = self.mention_candidates(cx);
        if candidates.is_empty() {
            return None;
        }
        let t = tokens(cx).clone();
        let mut panel = div()
            .mx(px(16.))
            .mb(px(6.))
            .p(px(4.))
            .rounded(R_XL)
            .border_1()
            .border_color(t.border)
            .bg(t.popover_fill())
            .shadow(t.shadow_popover())
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(8.))
                    .pt(px(4.))
                    .pb(px(2.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.muted_foreground)
                    .child("Members"),
            );
        for (slot, (jid, name)) in candidates.into_iter().enumerate() {
            let hover = t.accent;
            let picked_jid = jid.clone();
            let picked_name = name.clone();
            panel = panel.child(
                div()
                    .id(("mention", slot))
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(R_MD)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(14.))
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .child(avatar(&jid, &name, 20., crate::avatars::get(&jid), cx))
                    .child(div().truncate().child(SharedString::from(name)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_mention(&picked_jid, &picked_name, window, cx)
                    })),
            );
        }
        Some(panel.into_any_element())
    }

    /// Stickers already on this PC, newest first, for the sticker picker.
    fn sticker_overlay(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let at = self.stickers_at?;
        let t = tokens(cx).clone();
        let hover = t.accent;
        let mut grid = div().flex().flex_wrap().gap(px(4.)).p(px(8.));
        for (slot, path) in self.sticker_choices.iter().enumerate() {
            let send = path.clone();
            grid = grid.child(
                div()
                    .id(("sticker", slot))
                    .size(px(72.))
                    .rounded(R_MD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .child(
                        gpui::img(path.clone())
                            .size(px(64.))
                            .object_fit(gpui::ObjectFit::Contain),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.backend.send(Command::SendFile {
                            chat: this.chat.jid.clone(),
                            path: send.clone(),
                            kind: crate::events::Attach::Sticker,
                            caption: None,
                            reply_to: this.reply_to.take().map(|r| r.id),
                        });
                        this.stickers_at = None;
                        cx.notify();
                    })),
            );
        }
        let panel = div()
            .w(px(336.))
            .h(px(360.))
            .rounded(R_XL)
            .border_1()
            .border_color(t.border)
            .bg(t.popover_fill())
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
                    .child(if self.sticker_choices.is_empty() {
                        "No stickers yet: stickers you receive appear here"
                    } else {
                        "Stickers"
                    }),
            )
            .child(div().id("sticker-scroll").flex_1().min_h_0().overflow_y_scroll().child(grid));
        let size = window.viewport_size();
        let close = cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.stickers_at = None;
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
                        .child(
                            anchored()
                                .position(point(at.x - px(300.), at.y - px(380.)))
                                .snap_to_window_with_margin(px(8.))
                                .child(panel),
                        ),
                ),
            )
            .with_priority(10)
            .into_any_element(),
        )
    }
}

impl ChatView {
    /// Opens a photo by message ID, including photos folded into an album grid.
    fn open_picture(&mut self, id: &str, cx: &mut Context<Self>) {
        let on_disk = self
            .store
            .message(&self.chat.jid, id)
            .ok()
            .flatten()
            .and_then(|message| message.media_path)
            .map(PathBuf::from)
            .filter(|path| path.exists());
        match on_disk {
            Some(path) => self.viewer = Some(path),
            None => {
                self.open_when_ready = Some(id.to_string());
                self.notice = Some("Downloading…".to_string());
                self.backend.send(Command::DownloadMedia {
                    chat: self.chat.jid.clone(),
                    id: id.to_string(),
                });
            }
        }
        cx.notify();
    }
}

impl ChatView {
    /// Tells the other side we are typing, at most once every few seconds, and that we
    /// stopped when the box is emptied.
    fn typing_tick(&mut self, cx: &mut Context<Self>) {
        const REPEAT: std::time::Duration = std::time::Duration::from_secs(8);
        let has_text = !self.input.read(cx).value().trim().is_empty();
        let now = std::time::Instant::now();
        let activity = match (has_text, self.typing_sent) {
            (true, Some(at)) if now.duration_since(at) < REPEAT => return,
            (true, _) => {
                self.typing_sent = Some(now);
                Activity::Typing
            }
            (false, Some(_)) => {
                self.typing_sent = None;
                Activity::Idle
            }
            (false, None) => return,
        };
        self.backend.send(Command::Typing {
            chat: self.chat.jid.clone(),
            activity,
        });
    }
}

impl ChatView {
    /// Saves pictures pasted from the clipboard and stages them like picked files.
    fn stage_pasted(&mut self, item: &gpui::ClipboardItem, cx: &mut Context<Self>) -> bool {
        let mut staged = Vec::new();
        for entry in item.entries() {
            match entry {
                gpui::ClipboardEntry::Image(image) => {
                    let extension = match image.format {
                        gpui::ImageFormat::Png => "png",
                        gpui::ImageFormat::Jpeg => "jpg",
                        gpui::ImageFormat::Webp => "webp",
                        gpui::ImageFormat::Gif => "gif",
                        _ => "png",
                    };
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let path = crate::paths::media_dir().join(format!("pasted-{stamp}-{}.{extension}", staged.len()));
                    if std::fs::write(&path, &image.bytes).is_ok() {
                        staged.push(path);
                    }
                }
                gpui::ClipboardEntry::ExternalPaths(paths) => {
                    staged.extend(paths.paths().iter().filter(|p| p.is_file()).cloned());
                }
                _ => {}
            }
        }
        if staged.is_empty() {
            return false;
        }
        self.pending.extend(staged);
        cx.notify();
        true
    }

    /// The files waiting to be sent, shown above the message box until Enter is pressed.
    fn pending_tray(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.pending.is_empty() {
            return None;
        }
        let t = tokens(cx).clone();
        let mut tray = div()
            .id("pending-tray")
            .p(px(8.))
            .flex()
            .gap(px(8.))
            .overflow_x_scroll()
            .border_b_1()
            .border_color(t.border);
        for (slot, path) in self.pending.iter().enumerate() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let is_picture = matches!(
                path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(),
                Some("jpg" | "jpeg" | "png" | "webp" | "gif")
            );
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            tray = tray.child(
                div()
                    .w(px(144.))
                    .flex_shrink_0()
                    .rounded(R_LG)
                    .border_1()
                    .border_color(t.border)
                    .bg(t.background)
                    .p(px(6.))
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .h(px(96.))
                            .rounded(R_MD)
                            .bg(t.muted)
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(t.muted_foreground)
                            .child(if is_picture {
                                gpui::img(path.clone())
                                    .w(px(132.))
                                    .h(px(96.))
                                    .object_fit(gpui::ObjectFit::Cover)
                                    .into_any_element()
                            } else {
                                icon("file-text").size(px(28.)).into_any_element()
                            }),
                    )
                    .child(div().truncate().text_size(px(12.)).child(SharedString::from(name)))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(t.muted_foreground)
                            .child(super::widgets::file_size(size)),
                    )
                    .child(
                        div()
                            .id(("pending-remove", slot))
                            .absolute()
                            .top(px(4.))
                            .right(px(4.))
                            .size(px(24.))
                            .rounded(R_MD)
                            .border_1()
                            .border_color(t.border)
                            .bg(t.popover_fill())
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(icon("x").size(px(14.)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if slot < this.pending.len() {
                                    this.pending.remove(slot);
                                }
                                cx.notify();
                            })),
                    ),
            );
        }
        Some(tray.into_any_element())
    }
}

/// A video or GIF playing in the viewer.
struct VideoPlay {
    /// The picture currently on screen.
    frame: Option<Arc<gpui::RenderImage>>,
    /// The newest decoded picture, handed over from the decoding thread: (BGRA, width, height).
    latest: Arc<std::sync::Mutex<Option<(Vec<u8>, u32, u32)>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    size: (u32, u32),
    error: Arc<std::sync::Mutex<Option<String>>>,
    /// Pause, jump and position, shared with the decoding thread.
    control: Arc<VideoControl>,
    /// A GIF: loops silently and has no controls.
    looping: bool,
    /// Length in seconds as the message states it, until the player reports its own.
    stated_secs: f64,
}

/// What the video controls and the decoding thread tell each other.
struct VideoControl {
    paused: std::sync::atomic::AtomicBool,
    /// The video has played to its end and waits for a jump back.
    ended: std::sync::atomic::AtomicBool,
    /// A jump asked for, in milliseconds; -1 when none is waiting.
    seek_ms: std::sync::atomic::AtomicI64,
    /// Time of the picture on screen, in milliseconds.
    position_ms: std::sync::atomic::AtomicU64,
}

impl ChatView {
    /// Starts playing a video file in the viewer. GIFs loop and have no sound.
    fn start_video(&mut self, id: &str, path: PathBuf, looping: bool, window: &mut Window, cx: &mut Context<Self>) {
        use std::sync::atomic::Ordering;
        use std::time::{Duration, Instant};
        self.stop_video(window);

        let latest = Arc::new(std::sync::Mutex::new(None));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let error = Arc::new(std::sync::Mutex::new(None));
        let control = Arc::new(VideoControl {
            paused: false.into(),
            ended: false.into(),
            seek_ms: (-1).into(),
            position_ms: 0.into(),
        });
        {
            let (latest, stop, error, path, control) =
                (latest.clone(), stop.clone(), error.clone(), path.clone(), control.clone());
            std::thread::Builder::new()
                .name("video-decode".into())
                .spawn(move || {
                    let open = |error: &Arc<std::sync::Mutex<Option<String>>>| match crate::video::VideoReader::open(&path) {
                        Ok(reader) => Some(reader),
                        Err(problem) => {
                            if let Ok(mut slot) = error.lock() {
                                *slot = Some(format!("{problem:#}"));
                            }
                            None
                        }
                    };
                    let Some(mut reader) = open(&error) else {
                        return;
                    };
                    let (width, height) = (reader.width, reader.height);
                    let step = Duration::from_millis(15);
                    // `base` is the moment the video time `offset` was on screen.
                    let mut base = Instant::now();
                    let mut offset = 0f64;
                    // Sits out a pause; returns how long it lasted.
                    let wait_while_paused = |control: &VideoControl, stop: &std::sync::atomic::AtomicBool| {
                        let began = Instant::now();
                        while control.paused.load(Ordering::Relaxed)
                            && !stop.load(Ordering::Relaxed)
                            && control.seek_ms.load(Ordering::Relaxed) < 0
                        {
                            std::thread::sleep(step);
                        }
                        began.elapsed()
                    };
                    loop {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        // A jump asked for from the controls.
                        let jump = control.seek_ms.swap(-1, Ordering::Relaxed);
                        if jump >= 0 {
                            let to = jump as f64 / 1000.;
                            if reader.seek(to).is_err() {
                                match open(&error) {
                                    Some(fresh) => reader = fresh,
                                    None => return,
                                }
                            }
                            base = Instant::now();
                            offset = to;
                            control.ended.store(false, Ordering::Relaxed);
                        }
                        if control.ended.load(Ordering::Relaxed) {
                            std::thread::sleep(Duration::from_millis(30));
                            continue;
                        }
                        if control.paused.load(Ordering::Relaxed) {
                            base += wait_while_paused(&control, &stop);
                            continue;
                        }
                        let Ok(Some(frame)) = reader.next_frame() else {
                            if looping {
                                match open(&error) {
                                    Some(fresh) => reader = fresh,
                                    None => return,
                                }
                                base = Instant::now();
                                offset = 0.;
                            } else {
                                control.ended.store(true, Ordering::Relaxed);
                            }
                            continue;
                        };
                        // Wait for the picture's time in short steps, so that stop, pause
                        // and jumps are noticed at once.
                        let mut interrupted = false;
                        loop {
                            let due = base + Duration::from_secs_f64((frame.at_secs - offset).max(0.));
                            let now = Instant::now();
                            if now >= due {
                                break;
                            }
                            if stop.load(Ordering::Relaxed) {
                                return;
                            }
                            if control.seek_ms.load(Ordering::Relaxed) >= 0 {
                                interrupted = true;
                                break;
                            }
                            if control.paused.load(Ordering::Relaxed) {
                                base += wait_while_paused(&control, &stop);
                                continue;
                            }
                            std::thread::sleep((due - now).min(step));
                        }
                        if interrupted {
                            continue;
                        }
                        control
                            .position_ms
                            .store((frame.at_secs.max(0.) * 1000.) as u64, Ordering::Relaxed);
                        if let Ok(mut slot) = latest.lock() {
                            *slot = Some((frame.bgra, width, height));
                        }
                    }
                })
                .ok();
        }

        // The sound track plays through the audio player, started at the same moment.
        if !looping {
            if self.audio.is_none() {
                self.audio = crate::audio::AudioPlayer::new().ok();
            }
            if let Some(player) = self.audio.as_mut() {
                let _ = player.play(id, &path);
            }
        }

        let stated_secs = self
            .items
            .iter()
            .find_map(|item| match item {
                Item::Msg(view) if view.row.id == id => Some(view.media.seconds as f64),
                _ => None,
            })
            .unwrap_or(0.);
        self.video = Some(VideoPlay {
            frame: None,
            latest,
            stop,
            size: (0, 0),
            error,
            control,
            looping,
            stated_secs,
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(30)).await;
                let alive = this
                    .update_in(cx, |this, window, cx| {
                        let Some(play) = this.video.as_mut() else {
                            return false;
                        };
                        let newest = play.latest.lock().ok().and_then(|mut slot| slot.take());
                        if let Some((bgra, width, height)) = newest
                            && let Some(buffer) = image::RgbaImage::from_raw(width, height, bgra)
                        {
                            let picture = Arc::new(gpui::RenderImage::new(vec![image::Frame::new(buffer)]));
                            play.size = (width, height);
                            // Each picture is its own texture; release the one it replaces.
                            if let Some(old) = play.frame.replace(picture) {
                                let _ = window.drop_image(old);
                            }
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn stop_video(&mut self, window: &mut Window) {
        if let Some(play) = self.video.take() {
            play.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(frame) = play.frame {
                let _ = window.drop_image(frame);
            }
            if let Some(player) = self.audio.as_mut() {
                player.stop();
            }
        }
    }

    /// The video viewer: the playing picture on a dark backdrop; a click closes it.
    fn video_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let play = self.video.as_ref()?;
        let problem = play.error.lock().ok().and_then(|slot| slot.clone());
        let body: AnyElement = match (&play.frame, problem) {
            (Some(frame), _) => {
                // Small clips are enlarged, large ones fitted, both keeping their shape.
                let (width, height) = (play.size.0.max(1) as f32, play.size.1.max(1) as f32);
                let scale = (760. / width).min(560. / height).min(4.);
                gpui::img(frame.clone())
                    .w(px(width * scale))
                    .h(px(height * scale))
                    .rounded(R_LG)
                    .object_fit(gpui::ObjectFit::Contain)
                    .into_any_element()
            }
            (None, Some(problem)) => div()
                .max_w(px(420.))
                .text_color(gpui::white())
                .child(SharedString::from(format!("This video cannot be played here: {problem}")))
                .into_any_element(),
            (None, None) => div().text_color(gpui::white()).child("Loading…").into_any_element(),
        };
        Some(
            div()
                .id("video-viewer")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .bg(gpui::hsla(0., 0., 0., 0.92))
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .flex_col()
                .gap(px(14.))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.stop_video(window);
                    cx.notify();
                }))
                .child(body)
                .children(self.video_controls(cx))
                .into_any_element(),
        )
    }

    /// Length of the playing video in seconds: what the player reports, or failing that
    /// what the message said.
    fn video_length(&self) -> f64 {
        let reported = self
            .audio
            .as_ref()
            .and_then(|player| player.playback())
            .map(|playback| playback.duration_secs)
            .unwrap_or(0.);
        if reported > 0. {
            reported
        } else {
            self.video.as_ref().map(|play| play.stated_secs).unwrap_or(0.)
        }
    }

    fn toggle_video_pause(&mut self, cx: &mut Context<Self>) {
        use std::sync::atomic::Ordering;
        let Some(play) = self.video.as_ref() else {
            return;
        };
        if play.control.ended.load(Ordering::Relaxed) {
            // Played to the end: start over.
            play.control.paused.store(false, Ordering::Relaxed);
            play.control.seek_ms.store(0, Ordering::Relaxed);
            if let Some(player) = self.audio.as_mut() {
                let _ = player.seek(0.);
                if !player.is_playing() {
                    let _ = player.toggle();
                }
            }
        } else {
            let pause = !play.control.paused.load(Ordering::Relaxed);
            play.control.paused.store(pause, Ordering::Relaxed);
            if let Some(player) = self.audio.as_mut()
                && player.is_playing() == pause
            {
                let _ = player.toggle();
            }
        }
        cx.notify();
    }

    /// Jumps to a fraction (0 to 1) of the playing video.
    fn seek_video(&mut self, fraction: f64, cx: &mut Context<Self>) {
        use std::sync::atomic::Ordering;
        let length = self.video_length();
        let Some(play) = self.video.as_ref() else {
            return;
        };
        if length <= 0. {
            return;
        }
        let to = (length * fraction).clamp(0., (length - 0.1).max(0.));
        play.control.seek_ms.store((to * 1000.) as i64, Ordering::Relaxed);
        play.control.position_ms.store((to * 1000.) as u64, Ordering::Relaxed);
        if let Some(player) = self.audio.as_mut() {
            let _ = player.seek(to);
        }
        cx.notify();
    }

    /// Play or pause, the time, a bar to jump with, and mute. Not shown for GIFs.
    fn video_controls(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use std::sync::atomic::Ordering;
        let play = self.video.as_ref()?;
        if play.looping || play.frame.is_none() {
            return None;
        }
        let length = self.video_length();
        let position = play.control.position_ms.load(Ordering::Relaxed) as f64 / 1000.;
        let paused = play.control.paused.load(Ordering::Relaxed) || play.control.ended.load(Ordering::Relaxed);
        let muted = self.audio.as_ref().is_some_and(|player| player.is_muted());
        let clock = |seconds: f64| {
            let whole = seconds.max(0.) as u64;
            format!("{}:{:02}", whole / 60, whole % 60)
        };
        let done = if length > 0. { (position / length).clamp(0., 1.) } else { 0. };
        // The bar is a row of thin slices; a click on one jumps there.
        const SLICES: usize = 120;
        let mut bar = div().id("video-track").w(px(420.)).h(px(16.)).flex().items_center().cursor_pointer();
        for slice in 0..SLICES {
            let filled = (slice as f64 + 0.5) / SLICES as f64 <= done;
            bar = bar.child(
                div()
                    .id(("video-slice", slice))
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(div().w_full().h(px(4.)).bg(if filled {
                        gpui::white()
                    } else {
                        gpui::hsla(0., 0., 1., 0.28)
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.seek_video((slice as f64 + 0.5) / SLICES as f64, cx);
                    })),
            );
        }
        let button = |id: &'static str, glyph: &'static str| {
            div()
                .id(id)
                .size(px(32.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(gpui::white())
                .cursor_pointer()
                .hover(|style| style.bg(gpui::hsla(0., 0., 1., 0.14)))
                .child(icon(glyph))
        };
        Some(
            div()
                .id("video-controls")
                .h(px(44.))
                .px(px(10.))
                .rounded_full()
                .bg(gpui::hsla(0., 0., 0.1, 0.8))
                .flex()
                .items_center()
                .gap(px(10.))
                .text_size(px(12.))
                .text_color(gpui::white())
                // A click on the controls must not close the viewer behind them.
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    button("video-toggle", if paused { "play" } else { "pause" }).on_click(cx.listener(
                        |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_video_pause(cx);
                        },
                    )),
                )
                .child(div().w(px(36.)).child(clock(position)))
                .child(bar)
                .child(div().w(px(36.)).child(clock(length)))
                .child(
                    button("video-mute", if muted { "volume-x" } else { "volume-2" }).on_click(cx.listener(
                        move |this, _, _, cx| {
                            cx.stop_propagation();
                            if let Some(player) = this.audio.as_mut() {
                                player.set_muted(!muted);
                            }
                            cx.notify();
                        },
                    )),
                )
                .into_any_element(),
        )
    }
}

impl ChatView {
    /// The newest pinned message, shown under the header; a click jumps to it.
    fn pinned_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.pins.first()?.clone();
        let t = tokens(cx).clone();
        // Looked up among the loaded messages first: no database query per frame.
        // The wording is read once when the chat is loaded, not on every frame.
        let text = self.pin_text.clone().unwrap_or_else(|| "Pinned message".to_string());
        let more = self.pins.len().saturating_sub(1);
        let hover = t.fg(0.03);
        Some(
            div()
                .id("pinned-bar")
                .px(px(16.))
                .h(px(36.))
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(8.))
                .border_b_1()
                .border_color(Tokens::with_alpha(t.border, t.border.a * 0.7))
                .text_size(px(13.))
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .child(icon("pin").size(px(14.)).text_color(t.primary))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(t.fg(0.9))
                        .child(SharedString::from(text)),
                )
                .when(more > 0, |this| {
                    this.child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.muted_foreground)
                            .child(format!("+{more} more")),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| this.jump_to(&id, cx)))
                .into_any_element(),
        )
    }

    /// The person-only rows of the info panel's action list.
    fn person_actions(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let t = tokens(cx).clone();
        let hover = t.fg(0.06);
        vec![
            div()
                .id("info-block")
                .h(px(36.))
                .px(px(8.))
                .rounded(R_LG)
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(10.))
                .text_size(px(14.))
                .cursor_pointer()
                .text_color(t.destructive)
                .hover(move |style| style.bg(hover))
                .child(icon("ban").text_color(t.destructive))
                .child(if self.confirm_leave {
                    "Click again to block"
                } else {
                    "Block contact"
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.confirm_leave {
                        this.backend.send(Command::Block {
                            jid: this.chat.jid.clone(),
                            blocked: true,
                        });
                        this.confirm_leave = false;
                        this.notice = Some("Blocked".to_string());
                    } else {
                        this.confirm_leave = true;
                    }
                    cx.notify();
                }))
                .into_any_element(),
        ]
    }
}

/// The identity of a list item and a stamp of everything that affects how tall it is.
fn item_key(item: &Item) -> (String, u64) {
    use std::hash::{Hash, Hasher};
    match item {
        Item::Day(ts) => (format!("day:{}", local_day(*ts)), 0),
        Item::Unread => ("unread".to_string(), 0),
        Item::Msg(view) => {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            view.row.proto.hash(&mut hasher);
            std::mem::discriminant(&view.row.kind).hash(&mut hasher);
            view.row.text.hash(&mut hasher);
            view.row.media_path.hash(&mut hasher);
            view.row.edited.hash(&mut hasher);
            view.head.hash(&mut hasher);
            view.notice.hash(&mut hasher);
            view.quote.is_some().hash(&mut hasher);
            view.media.display.is_some().hash(&mut hasher);
            view.media.thumb_file.is_some().hash(&mut hasher);
            view.album.len().hash(&mut hasher);
            for chip in &view.reactions {
                chip.emoji.hash(&mut hasher);
                chip.count.hash(&mut hasher);
            }
            (view.row.id.clone(), hasher.finish())
        }
    }
}

impl ChatView {
    /// Starts recording a voice message from the microphone.
    fn start_recording(&mut self, cx: &mut Context<Self>) {
        match crate::voice::Recorder::start() {
            Ok(recorder) => {
                self.recorder = Some(recorder);
                self.backend.send(Command::Typing {
                    chat: self.chat.jid.clone(),
                    activity: Activity::Recording,
                });
            }
            Err(error) => self.notice = Some(format!("Microphone: {error:#}")),
        }
        cx.notify();
    }

    /// Throws the recording away.
    fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        self.recorder = None;
        self.backend.send(Command::Typing {
            chat: self.chat.jid.clone(),
            activity: Activity::Idle,
        });
        cx.notify();
    }

    /// Stops recording and sends what was said as a voice message.
    fn finish_recording(&mut self, cx: &mut Context<Self>) {
        let Some(recorder) = self.recorder.take() else {
            return;
        };
        if let Some(error) = recorder.error() {
            self.notice = Some(format!("Microphone: {error}"));
        }
        let (samples, rate) = recorder.stop();
        let chat = self.chat.jid.clone();
        let reply_to = self.reply_to.take().map(|reply| reply.id);
        self.backend.send(Command::Typing {
            chat: chat.clone(),
            activity: Activity::Idle,
        });
        cx.spawn(async move |this, cx| {
            // Encoding takes a moment for a long recording: keep it off the interface.
            let made = cx
                .background_executor()
                .spawn(async move {
                    // The encoder runs in a child process, away from the database.
                    let recording = crate::voice::finish_isolated(&samples, rate)?;
                    let folder = crate::paths::media_dir();
                    std::fs::create_dir_all(&folder)?;
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|elapsed| elapsed.as_millis())
                        .unwrap_or(0);
                    let path = folder.join(format!("voice-{stamp}.ogg"));
                    recording.save(&path)?;
                    anyhow::Ok(path)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match made {
                    Ok(path) => this.backend.send(Command::SendFile {
                        chat,
                        path,
                        kind: crate::events::Attach::Voice,
                        caption: None,
                        reply_to,
                    }),
                    Err(error) => this.notice = Some(format!("Voice message: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The message box while recording: discard, a red dot, the time, a live level
    /// meter, and send.
    fn recording_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let recorder = self.recorder.as_ref()?;
        let t = tokens(cx).clone();
        let seconds = recorder.elapsed().as_secs();
        let mut meter = div()
            .flex_1()
            .min_w_0()
            .h(px(28.))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(2.))
            .overflow_hidden();
        for level in recorder.levels(64) {
            // Speech is quiet on a linear scale; the square root makes it visible.
            let height = 3. + level.clamp(0., 1.).sqrt() * 23.;
            meter = meter.child(
                div()
                    .w(px(3.))
                    .h(px(height))
                    .flex_shrink_0()
                    .rounded(px(1.5))
                    .bg(t.fg(0.55)),
            );
        }
        Some(
            div()
                .p(px(6.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    icon_button("record-discard", "trash", cx)
                        .tooltip(tip("Discard"))
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_recording(cx))),
                )
                .child(div().size(px(8.)).flex_shrink_0().rounded_full().bg(t.destructive))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(13.))
                        .text_color(t.foreground)
                        .child(format!("{}:{:02}", seconds / 60, seconds % 60)),
                )
                .child(meter)
                .child(
                    div()
                        .id("record-send")
                        .size(px(32.))
                        .flex_shrink_0()
                        .rounded(R_LG)
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .bg(t.primary)
                        .text_color(t.primary_foreground)
                        .shadow(t.glow())
                        .child(icon("send-horizontal"))
                        .tooltip(tip("Send voice message"))
                        .on_click(cx.listener(|this, _, _, cx| this.finish_recording(cx))),
                )
                .into_any_element(),
        )
    }
}

impl ChatView {
    /// The invite link asked for has arrived: put it on the clipboard.
    pub fn invite_link_arrived(&mut self, link: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(link));
        self.notice = Some("Invite link copied".to_string());
        cx.notify();
    }

    pub fn join_requests_arrived(&mut self, people: Vec<String>, cx: &mut Context<Self>) {
        self.join_requests = people;
        cx.notify();
    }

    /// Info panel: the disappearing-message timer, and for groups the admin switches
    /// and the people waiting to join.
    fn settings_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use crate::events::GroupSetting;
        let t = tokens(cx).clone();
        let mut rows = Vec::new();
        let label = |text: &'static str| {
            div()
                .px(px(8.))
                .pt(px(14.))
                .pb(px(4.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.muted_foreground)
                .child(text)
                .into_any_element()
        };
        let is_group = self.chat.kind == ChatKind::Group;
        let can_change = !is_group || self.i_am_admin();

        // Disappearing messages.
        rows.push(label("Disappearing messages"));
        let mut timer_row = div().px(px(8.)).flex().flex_wrap().gap(px(6.));
        for (index, (name, seconds)) in
            [("Off", 0u32), ("24 hours", 86_400), ("7 days", 604_800), ("90 days", 7_776_000)]
                .into_iter()
                .enumerate()
        {
            let active = self.timer == seconds as i64;
            let hover = t.fg(0.12);
            timer_row = timer_row.child(
                div()
                    .id(("timer", index))
                    .h(px(28.))
                    .px(px(10.))
                    .rounded(R_MD)
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .when(active, |this| this.bg(t.tint(0.14)).text_color(t.primary))
                    .when(!active, |this| this.bg(t.fg(0.06)).text_color(t.foreground))
                    .when(can_change && !active, |this| {
                        this.cursor_pointer().hover(move |style| style.bg(hover))
                    })
                    .child(name)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if can_change && !active {
                            this.backend.send(Command::SetTimer {
                                chat: this.chat.jid.clone(),
                                seconds,
                            });
                        }
                        cx.notify();
                    })),
            );
        }
        rows.push(timer_row.into_any_element());

        if !is_group {
            return rows;
        }
        let Some(settings) = self.group_settings else {
            return rows;
        };

        // The group's switches. Members see them; admins can flip them.
        rows.push(label("Group settings"));
        let switches: [(&'static str, bool, fn(bool) -> GroupSetting); 4] = [
            ("Only admins can edit group info", settings.locked, GroupSetting::Locked),
            ("Only admins can send messages", settings.announce, GroupSetting::Announce),
            ("Admins approve new members", settings.approval, GroupSetting::Approval),
            ("Members can add other people", settings.members_can_add, GroupSetting::MembersCanAdd),
        ];
        for (index, (name, on, make)) in switches.into_iter().enumerate() {
            let hover = t.fg(0.06);
            rows.push(
                div()
                    .id(("group-switch", index))
                    .h(px(36.))
                    .px(px(8.))
                    .rounded(R_LG)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(13.))
                    .when(can_change, |this| this.cursor_pointer().hover(move |style| style.bg(hover)))
                    .child(div().flex_1().child(name))
                    .child(
                        // A small switch: the knob sits right when on.
                        div()
                            .w(px(32.))
                            .h(px(18.))
                            .rounded_full()
                            .p(px(2.))
                            .flex()
                            .when(on, |this| this.bg(t.primary).justify_end())
                            .when(!on, |this| this.bg(t.fg(0.18)))
                            .child(div().size(px(14.)).rounded_full().bg(gpui::white())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if can_change {
                            this.backend.send(Command::SetGroupSetting {
                                chat: this.chat.jid.clone(),
                                setting: make(!on),
                            });
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }

        // People waiting to join, when admins approve members.
        if can_change && settings.approval {
            let hover = t.fg(0.06);
            rows.push(
                div()
                    .id("join-requests-refresh")
                    .h(px(36.))
                    .px(px(8.))
                    .rounded(R_LG)
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(13.))
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .child(icon("user-plus").text_color(t.muted_foreground))
                    .child(format!("Requests to join ({})", self.join_requests.len()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.backend.send(Command::JoinRequests {
                            chat: this.chat.jid.clone(),
                        });
                        cx.notify();
                    }))
                    .into_any_element(),
            );
            for (index, person) in self.join_requests.iter().take(50).enumerate() {
                let name = self
                    .names
                    .get(person)
                    .cloned()
                    .unwrap_or_else(|| crate::model::display_number(person));
                let (yes, no) = (person.clone(), person.clone());
                rows.push(
                    div()
                        .h(px(40.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(avatar(person, &name, 24., crate::avatars::get(person), cx))
                        .child(div().flex_1().truncate().text_size(px(13.)).child(SharedString::from(name)))
                        .child(
                            icon_button(("join-yes", index), "check", cx)
                                .tooltip(tip("Let in"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.backend.send(Command::AnswerJoinRequest {
                                        chat: this.chat.jid.clone(),
                                        person: yes.clone(),
                                        approve: true,
                                    });
                                    cx.notify();
                                })),
                        )
                        .child(
                            icon_button(("join-no", index), "x", cx)
                                .tooltip(tip("Turn down"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.backend.send(Command::AnswerJoinRequest {
                                        chat: this.chat.jid.clone(),
                                        person: no.clone(),
                                        approve: false,
                                    });
                                    cx.notify();
                                })),
                        )
                        .into_any_element(),
                );
            }
        }
        rows
    }
}

impl ChatView {
    /// Jumps within a voice message. Only the clip that is loaded can be jumped in; a
    /// click on another one's bars starts it instead.
    fn seek_audio(&mut self, id: &str, fraction: f32, cx: &mut Context<Self>) {
        let loaded = self.audio.as_ref().and_then(|player| player.playback()).filter(|p| p.id == id);
        match loaded {
            Some(playback) if playback.duration_secs > 0. => {
                if let Some(player) = self.audio.as_mut() {
                    let _ = player.seek(playback.duration_secs * fraction as f64);
                }
            }
            _ => self.open_media(id, cx),
        }
        cx.notify();
    }

    /// Steps the playback speed: 1, 1.5, 2, and back to 1.
    fn step_audio_speed(&mut self, cx: &mut Context<Self>) {
        if let Some(player) = self.audio.as_mut() {
            let next = match player.rate() {
                rate if rate >= 1.9 => 1.,
                rate if rate >= 1.4 => 2.,
                _ => 1.5,
            };
            player.set_rate(next);
        }
        cx.notify();
    }
}

impl ChatView {
    /// Under a channel we only follow: who may post, and mute and unfollow.
    fn channel_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx).clone();
        let muted = self.channel.as_ref().is_some_and(|channel| channel.muted);
        let followed = self.channel.is_some();
        let button = |id: &'static str, glyph: &'static str, label: &'static str| {
            let hover = t.fg(0.12);
            div()
                .id(id)
                .h(px(32.))
                .px(px(12.))
                .rounded(R_LG)
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(8.))
                .bg(t.fg(0.06))
                .text_size(px(13.))
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .child(icon(glyph).size(px(14.)))
                .child(label)
        };
        div()
            .h(px(52.))
            .px(px(14.))
            .rounded(if t.liquid() { px(22.) } else { R_XL })
            .border_1()
            .border_color(if t.liquid() { gpui::transparent_black() } else { t.fg(0.08) })
            .bg(t.surface(Surface::Bar, t.card))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .text_size(px(13.))
                    .text_color(t.muted_foreground)
                    .child("Only the channel's admins can post here."),
            )
            .when(followed, |this| {
                this.child(
                    button(
                        "channel-mute",
                        if muted { "bell" } else { "bell-off" },
                        if muted { "Unmute" } else { "Mute" },
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.backend.send(Command::MuteChannel {
                            chat: this.chat.jid.clone(),
                            muted: !muted,
                        });
                        cx.notify();
                    })),
                )
                .child(
                    button(
                        "channel-unfollow",
                        "log-out",
                        if self.confirm_leave { "Click again to unfollow" } else { "Unfollow" },
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.confirm_leave {
                            this.backend.send(Command::UnfollowChannel {
                                chat: this.chat.jid.clone(),
                            });
                            this.confirm_leave = false;
                        } else {
                            this.confirm_leave = true;
                        }
                        cx.notify();
                    })),
                )
            })
            .into_any_element()
    }
}
