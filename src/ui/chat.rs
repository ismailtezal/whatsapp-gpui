//! The open conversation: header, message list and composer.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, ListAlignment, ListState, SharedString, Window,
    div, list, px,
};
use gpui_component::input::{InputEvent, Textarea, TextareaState};

use super::message::{
    MsgView, Quote, ReactionChip, day_separator, media_of, message_row, notice_of, record_of,
    unread_separator,
};
use super::theme::{HEADER_H, R_LG, R_XL, Tokens, tokens};
use super::widgets::{avatar, day_label, icon, icon_button, local_day, tip};
use crate::backend::Backend;
use crate::content::preview_label;
use crate::events::{Activity, Command};
use crate::model::{ChatKind, ChatRow, MsgKind, MsgRow};
use crate::store::Store;

const PAGE: usize = 80;
/// Messages from one author this close together share a header.
const GROUP_GAP_SECS: i64 = 7 * 60;

enum Item {
    Day(i64),
    Unread,
    Msg(Box<MsgView>),
}

pub struct ChatView {
    backend: Backend,
    store: Arc<Store>,
    pub chat: ChatRow,
    me_name: String,
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
    activity: Option<(String, Activity)>,
    presence: Option<String>,
}

impl ChatView {
    pub fn new(
        backend: Backend,
        chat: ChatRow,
        me_name: String,
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
        cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { shift: false, .. } = event {
                this.send(window, cx);
            }
        })
        .detach();

        let unread_at_open = chat.unread;
        let mut view = ChatView {
            backend,
            store,
            chat,
            me_name,
            items: Vec::new(),
            list: ListState::new(0, ListAlignment::Bottom, px(1200.)),
            input,
            names: HashMap::new(),
            loaded: PAGE,
            has_older: true,
            unread_at_open,
            reply_to: None,
            activity: None,
            presence: None,
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
        let by_id: HashMap<&str, &MsgRow> = rows.iter().map(|r| (r.id.as_str(), r)).collect();

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
            let author_seed = if row.from_me {
                "me".to_string()
            } else {
                row.sender.clone().unwrap_or_else(|| self.chat.jid.clone())
            };
            let author = if row.from_me {
                self.me_name.clone()
            } else {
                self.name_of(&author_seed)
            };
            let notice = if row.kind == MsgKind::System {
                let names = &mut self.names;
                let store = &self.store;
                let resolver = |jid: &str| -> String {
                    let bare = crate::content::bare_jid(jid);
                    let canonical = store.canonical(&bare).unwrap_or(bare);
                    store
                        .display_name(&canonical)
                        .unwrap_or_else(|_| crate::model::display_number(&canonical))
                };
                let _ = names;
                notice_of(&record, &resolver)
            } else {
                None
            };

            let quote = row.quoted_id.as_deref().map(|quoted_id| match by_id.get(quoted_id) {
                Some(original) => {
                    let seed = if original.from_me {
                        "me".to_string()
                    } else {
                        original.sender.clone().unwrap_or_else(|| self.chat.jid.clone())
                    };
                    let author = if original.from_me {
                        self.me_name.clone()
                    } else {
                        self.name_of(&seed)
                    };
                    Quote {
                        author,
                        author_seed: seed,
                        text: preview_label(original.kind, original.text.as_deref()),
                    }
                }
                None => match self.store.message(&self.chat.jid, quoted_id).ok().flatten() {
                    Some(original) => {
                        let seed = if original.from_me {
                            "me".to_string()
                        } else {
                            original.sender.clone().unwrap_or_else(|| self.chat.jid.clone())
                        };
                        let author = if original.from_me {
                            self.me_name.clone()
                        } else {
                            self.name_of(&seed)
                        };
                        Quote {
                            author,
                            author_seed: seed,
                            text: preview_label(original.kind, original.text.as_deref()),
                        }
                    }
                    None => Quote {
                        author: String::new(),
                        author_seed: String::new(),
                        text: "Original message is not on this device".to_string(),
                    },
                },
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
            })));
            previous = Some(row);
        }

        let old_count = self.list.item_count();
        self.items = items;
        if old_count == 0 {
            self.list.reset(self.items.len());
        } else {
            // Replacing everything keeps the view pinned to the bottom when it was there.
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

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.backend.send(Command::SendText {
            chat: self.chat.jid.clone(),
            text: text.to_string(),
            reply_to: self.reply_to.take().map(|r| r.id),
            mentions: Vec::new(),
        });
        self.input.update(cx, |input, cx| input.set_value("", window, cx));
        self.unread_at_open = 0;
        cx.notify();
    }

    fn render_item(&mut self, index: usize, cx: &mut App) -> AnyElement {
        match self.items.get(index) {
            Some(Item::Day(ts)) => day_separator(day_label(*ts), cx),
            Some(Item::Unread) => unread_separator(cx),
            Some(Item::Msg(view)) => message_row(view, cx),
            None => div().into_any_element(),
        }
    }

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
            .child(avatar(&self.chat.jid, &self.chat.title, 28., None, cx))
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
                .when_some(self.reply_to.clone(), |this, reply| {
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
                            .child(icon("corner-up-left").size(px(12.)))
                            .child("Replying to")
                            .child(
                                div()
                                    .truncate()
                                    .text_color(t.foreground)
                                    .child(preview_label(reply.kind, reply.text.as_deref())),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .id("cancel-reply")
                                    .cursor_pointer()
                                    .child(icon("x").size(px(14.)))
                                    .on_click(cx.listener(|this, _, _, cx| {
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
                        .child(icon_button("attach", "plus", cx).tooltip(tip("Attach")))
                        .child(
                            div().flex_1().min_w_0().py(px(1.)).child(
                                Textarea::new(&self.input).appearance(false).bordered(false),
                            ),
                        )
                        .child(icon_button("emoji", "smile", cx).tooltip(tip("Emoji")))
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
                                    this.bg(t.primary).text_color(t.primary_foreground)
                                })
                                .when(!has_text, |this| this.text_color(t.muted_foreground))
                                .child(icon(if has_text { "send-horizontal" } else { "mic" }))
                                .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
                        ),
                ),
        )
    }
}

impl Render for ChatView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx).clone();
        let view = cx.entity();
        let older = self.has_older;
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.background)
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
    }
}
