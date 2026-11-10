//! One message, prepared for display and drawn as a flat row (avatar gutter, author line,
//! body), following the design's "flat message rows, not bubbles".

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{AnyElement, App, Div, FontWeight, Image, ImageFormat, SharedString, div, px};
use whatsapp_rust::buffa::Message as _;

use super::theme::{GUTTER, R_LG, R_MD, Tokens, tokens};
use super::widgets::{avatar, clock, duration, file_size, header_time, icon, tint_of};
use crate::content::{self, preview_label};
use crate::model::{Ack, MsgKind, MsgRow};
use crate::system::notice_text;
use crate::wa;

/// A reaction chip: emoji, how many, whether the account itself reacted.
#[derive(Clone, Debug)]
pub struct ReactionChip {
    pub emoji: String,
    pub count: usize,
    pub mine: bool,
}

/// The message a reply points at, as one line.
#[derive(Clone, Debug)]
pub struct Quote {
    pub author: String,
    pub author_seed: String,
    pub text: String,
}

/// Media details needed to draw a message without decoding the record again.
#[derive(Clone, Default)]
pub struct Media {
    pub thumb: Option<Arc<Image>>,
    pub width: u32,
    pub height: u32,
    pub seconds: u32,
    pub file_name: Option<String>,
    pub file_size: u64,
    pub mime: Option<String>,
    pub page_count: Option<u32>,
}

/// A stored message with everything the row needs resolved.
#[derive(Clone)]
pub struct MsgView {
    pub row: MsgRow,
    pub author: String,
    pub author_seed: String,
    /// First message of a run from the same author: shows the avatar and name.
    pub head: bool,
    pub quote: Option<Quote>,
    pub reactions: Vec<ReactionChip>,
    pub media: Media,
    /// Notice wording for system rows.
    pub notice: Option<String>,
    pub view_once: bool,
}

/// Extracts what a row needs from the stored record.
pub fn media_of(record: &wa::WebMessageInfo) -> (Media, bool) {
    let mut media = Media::default();
    let Some(message) = record.message.as_option() else {
        return (media, false);
    };
    let (base, view_once, _) = content::unwrap(message);
    let thumb = |bytes: &Option<Vec<u8>>| {
        bytes
            .as_ref()
            .filter(|b| !b.is_empty())
            .map(|b| Arc::new(Image::from_bytes(ImageFormat::Jpeg, b.clone())))
    };
    if let Some(m) = base.image_message.as_option() {
        media.thumb = thumb(&m.jpeg_thumbnail);
        media.width = m.width.unwrap_or(0);
        media.height = m.height.unwrap_or(0);
        media.file_size = m.file_length.unwrap_or(0);
        media.mime = m.mimetype.clone();
    } else if let Some(m) = base.video_message.as_option().or(base.ptv_message.as_option()) {
        media.thumb = thumb(&m.jpeg_thumbnail);
        media.width = m.width.unwrap_or(0);
        media.height = m.height.unwrap_or(0);
        media.seconds = m.seconds.unwrap_or(0);
        media.file_size = m.file_length.unwrap_or(0);
        media.mime = m.mimetype.clone();
    } else if let Some(m) = base.audio_message.as_option() {
        media.seconds = m.seconds.unwrap_or(0);
        media.file_size = m.file_length.unwrap_or(0);
        media.mime = m.mimetype.clone();
    } else if let Some(m) = base.document_message.as_option() {
        media.thumb = thumb(&m.jpeg_thumbnail);
        media.file_name = m.file_name.clone().or_else(|| m.title.clone());
        media.file_size = m.file_length.unwrap_or(0);
        media.mime = m.mimetype.clone();
        media.page_count = m.page_count;
    } else if let Some(m) = base.sticker_message.as_option() {
        media.width = m.width.unwrap_or(0);
        media.height = m.height.unwrap_or(0);
        media.mime = m.mimetype.clone();
    }
    (media, view_once)
}

/// Decodes the stored record of a row.
pub fn record_of(row: &MsgRow) -> wa::WebMessageInfo {
    wa::WebMessageInfo::decode_from_slice(&row.proto).unwrap_or_default()
}

/// The notice wording of a system row, if it has any.
pub fn notice_of(record: &wa::WebMessageInfo, name: &dyn Fn(&str) -> String) -> Option<String> {
    notice_text(record, name)
}

fn ticks(ack: Ack, t: &Tokens) -> AnyElement {
    let (name, color) = match ack {
        Ack::Error => ("circle-alert", t.destructive),
        Ack::Pending => ("clock", t.muted_foreground),
        Ack::Sent => ("check", t.muted_foreground),
        Ack::Delivered => ("check-check", t.muted_foreground),
        Ack::Read | Ack::Played => ("check-check", t.link),
    };
    icon(name).size(px(14.)).text_color(color).into_any_element()
}

fn kind_icon(kind: MsgKind) -> &'static str {
    use MsgKind::*;
    match kind {
        Image => "image",
        Video | VideoNote => "video",
        Gif => "film",
        Audio => "music",
        Voice => "mic",
        Document => "file-text",
        Sticker | StickerPack => "sticker",
        Location | LiveLocation => "map-pin",
        Contact | Contacts => "contact",
        Poll | PollResult => "chart-bar",
        Event => "calendar",
        GroupInvite => "users",
        NewsletterInvite => "megaphone",
        Call => "phone",
        Revoked => "ban",
        Album => "image",
        Payment | Order | Product => "file",
        _ => "message-square",
    }
}

/// A bordered card with an icon tile, a title and a secondary line (files, audio, contacts).
fn info_card(icon_name: &str, title: String, detail: String, t: &Tokens) -> Div {
    div()
        .max_w(px(384.))
        .rounded(R_LG)
        .border_1()
        .border_color(t.border)
        .bg(t.card)
        .p(px(10.))
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .size(px(36.))
                .flex_shrink_0()
                .rounded(R_MD)
                .bg(t.muted)
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.muted_foreground)
                .child(icon(icon_name)),
        )
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.foreground)
                        .truncate()
                        .child(title),
                )
                .when(!detail.is_empty(), |this| {
                    this.child(
                        div()
                            .text_size(px(12.))
                            .text_color(Tokens::with_alpha(t.muted_foreground, 0.85))
                            .truncate()
                            .child(detail),
                    )
                }),
        )
}

/// Fits a picture into the design's 480 x 340 box without upscaling past its real size.
fn fit(width: u32, height: u32) -> (f32, f32) {
    let (w, h) = if width == 0 || height == 0 {
        (320., 240.)
    } else {
        (width as f32, height as f32)
    };
    let scale = (480. / w).min(340. / h).min(1.);
    ((w * scale).max(64.), (h * scale).max(64.))
}

fn picture(view: &MsgView, t: &Tokens, overlay: Option<&'static str>) -> Div {
    let (w, h) = fit(view.media.width, view.media.height);
    let mut frame = div()
        .w(px(w))
        .h(px(h))
        .rounded(R_LG)
        .border_1()
        .border_color(t.border)
        .bg(t.muted)
        .overflow_hidden()
        .relative()
        .flex()
        .items_center()
        .justify_center();
    let source = view
        .row
        .media_path
        .as_ref()
        .filter(|_| view.row.kind == MsgKind::Image)
        .map(|path| gpui::img(std::path::PathBuf::from(path)))
        .or_else(|| view.media.thumb.clone().map(gpui::img));
    if let Some(image) = source {
        frame = frame.child(
            image
                .absolute()
                .top_0()
                .left_0()
                .w(px(w))
                .h(px(h))
                .object_fit(gpui::ObjectFit::Cover),
        );
    }
    if let Some(name) = overlay {
        frame = frame.child(
            div()
                .size(px(44.))
                .rounded_full()
                .bg(gpui::hsla(0., 0., 0., 0.55))
                .flex()
                .items_center()
                .justify_center()
                .text_color(gpui::white())
                .child(icon(name).size(px(20.))),
        );
    }
    if view.media.seconds > 0 {
        frame = frame.child(
            div()
                .absolute()
                .bottom(px(6.))
                .left(px(6.))
                .px(px(6.))
                .py(px(1.))
                .rounded(R_MD)
                .bg(gpui::hsla(0., 0., 0., 0.6))
                .text_color(gpui::white())
                .text_size(px(11.))
                .child(duration(view.media.seconds)),
        );
    }
    frame
}

fn body_text(text: &str, t: &Tokens) -> Div {
    div()
        .text_size(px(14.))
        .line_height(px(20.3))
        .text_color(t.fg(0.9))
        .child(SharedString::from(text.to_string()))
}

/// The content of a message below its header line.
fn content(view: &MsgView, t: &Tokens) -> Div {
    use MsgKind::*;
    let row = &view.row;
    let text = row.text.as_deref().filter(|s| !s.is_empty());
    let mut column = div().flex().flex_col().gap(px(4.)).min_w_0();

    if view.view_once && row.kind.is_media() {
        return column.child(info_card(
            "timer",
            "View once message".to_string(),
            "Open it on your phone".to_string(),
            t,
        ));
    }

    match row.kind {
        Text => {
            if let Some(text) = text {
                column = column.child(body_text(text, t));
            }
        }
        Image => {
            column = column.child(picture(view, t, None));
            if let Some(text) = text {
                column = column.child(body_text(text, t));
            }
        }
        Video | VideoNote => {
            column = column.child(picture(view, t, Some("play")));
            if let Some(text) = text {
                column = column.child(body_text(text, t));
            }
        }
        Gif => {
            column = column.child(picture(view, t, Some("film")));
            if let Some(text) = text {
                column = column.child(body_text(text, t));
            }
        }
        Sticker => {
            let side = 160.;
            let frame = div().size(px(side)).flex().items_center().justify_center();
            column = column.child(match &row.media_path {
                Some(path) => frame.child(
                    gpui::img(std::path::PathBuf::from(path))
                        .size(px(side))
                        .object_fit(gpui::ObjectFit::Contain),
                ),
                None => frame
                    .rounded(R_LG)
                    .bg(t.fg(0.04))
                    .text_color(t.muted_foreground)
                    .child(icon("sticker").size(px(28.))),
            });
        }
        Voice | Audio => {
            let title = if row.kind == Voice {
                "Voice message"
            } else {
                "Audio"
            };
            column = column.child(info_card(
                if row.kind == Voice { "mic" } else { "music" },
                title.to_string(),
                duration(view.media.seconds),
                t,
            ));
        }
        Document => {
            let name = view
                .media
                .file_name
                .clone()
                .unwrap_or_else(|| "Document".to_string());
            let mut detail = file_size(view.media.file_size);
            if let Some(pages) = view.media.page_count.filter(|p| *p > 0) {
                detail = format!("{pages} pages · {detail}");
            }
            column = column.child(info_card("file-text", name.clone(), detail, t));
            if let Some(caption) = text.filter(|c| *c != name) {
                column = column.child(body_text(caption, t));
            }
        }
        Revoked => {
            column = column.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(14.))
                    .italic()
                    .text_color(t.muted_foreground)
                    .child(icon("ban").size(px(14.)))
                    .child("This message was deleted"),
            );
        }
        Unsupported => {
            column = column.child(
                div()
                    .text_size(px(13.))
                    .italic()
                    .text_color(t.muted_foreground)
                    .child("This message type is not shown yet. Open it on your phone."),
            );
        }
        other => {
            let label = preview_label(other, None);
            column = column.child(info_card(
                kind_icon(other),
                text.map(str::to_string).unwrap_or_else(|| label.clone()),
                if text.is_some() { label } else { String::new() },
                t,
            ));
        }
    }

    if !view.reactions.is_empty() {
        let mut chips = div().flex().flex_wrap().gap(px(4.)).mt(px(2.));
        for chip in &view.reactions {
            let (border, fill, color) = if chip.mine {
                (t.tint(0.45), t.tint(0.12), t.primary)
            } else {
                (gpui::transparent_black(), t.fg(0.06), t.muted_foreground)
            };
            chips = chips.child(
                div()
                    .h(px(28.))
                    .px(px(8.))
                    .rounded_full()
                    .border_1()
                    .border_color(border)
                    .bg(fill)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().text_size(px(14.)).child(chip.emoji.clone()))
                    .when(chip.count > 1, |this| {
                        this.child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(color)
                                .child(chip.count.to_string()),
                        )
                    }),
            );
        }
        column = column.child(chips);
    }
    column
}

/// A centred-gutter notice line ("X added Y", "Missed voice call").
fn notice_row(view: &MsgView, text: &str, t: &Tokens) -> AnyElement {
    let (name, color) = if text.contains("Missed") {
        ("phone-missed", t.destructive)
    } else if text.contains("end-to-end") {
        ("lock", t.muted_foreground)
    } else if text.contains("removed") || text.contains("left") {
        ("user-minus", t.destructive)
    } else if text.contains("added") || text.contains("joined") {
        ("user-plus", t.online)
    } else {
        ("info", t.muted_foreground)
    };
    div()
        .w_full()
        .relative()
        .pl(GUTTER)
        .pr(px(48.))
        .py(px(4.))
        .mt(px(8.))
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(6.))
                .w(GUTTER)
                .flex()
                .justify_center()
                .text_color(color)
                .child(icon(name).size(px(14.))),
        )
        .child(
            div()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(t.muted_foreground)
                .child(SharedString::from(format!(
                    "{text}  ·  {}",
                    header_time(view.row.ts)
                ))),
        )
        .into_any_element()
}

/// Draws one message row.
pub fn message_row(view: &MsgView, cx: &App) -> AnyElement {
    let t = tokens(cx);
    let row = &view.row;

    if row.kind == MsgKind::System {
        return match view.notice.as_deref() {
            Some(text) => notice_row(view, text, t),
            None => div().into_any_element(),
        };
    }

    let hover = t.fg(0.03);
    let mut outer = div()
        .w_full()
        .relative()
        .pl(GUTTER)
        .pr(px(48.))
        .hover(move |style| style.bg(hover));
    outer = if view.head {
        outer.mt(px(12.)).pt(px(2.)).pb(px(1.))
    } else {
        outer.py(px(1.))
    };

    if let Some(quote) = &view.quote {
        let (_, name_color) = tint_of(&quote.author_seed, t.dark);
        outer = outer.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .mb(px(2.))
                .text_size(px(12.))
                .text_color(t.muted_foreground)
                .child(icon("corner-up-left").size(px(12.)))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(name_color)
                        .flex_shrink_0()
                        .child(quote.author.clone()),
                )
                .child(div().truncate().child(quote.text.clone())),
        );
    }

    if view.head {
        let (_, name_color) = tint_of(&view.author_seed, t.dark);
        outer = outer
            .child(
                div()
                    .absolute()
                    .left(px(16.))
                    .top(px(if view.quote.is_some() { 22. } else { 2. }))
                    .child(avatar(&view.author_seed, &view.author, 32., None, cx)),
            )
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .h(px(20.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if row.from_me { t.foreground } else { name_color })
                            .child(view.author.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.muted_foreground)
                            .child(header_time(row.ts)),
                    )
                    .when(row.from_me, |this| this.child(ticks(row.ack, t)))
                    .when(row.starred, |this| {
                        this.child(icon("star").size(px(11.)).text_color(t.idle))
                    })
                    .when(row.forwarded, |this| {
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .text_size(px(11.))
                                .italic()
                                .text_color(t.muted_foreground)
                                .child(icon("forward").size(px(11.)))
                                .child("Forwarded"),
                        )
                    }),
            );
    } else {
        // Continued message: the short time sits in the gutter.
        outer = outer.child(
            div()
                .absolute()
                .left_0()
                .top(px(4.))
                .w(GUTTER)
                .flex()
                .justify_center()
                .items_center()
                .gap(px(3.))
                .text_size(px(10.))
                .text_color(Tokens::with_alpha(t.muted_foreground, 0.7))
                .child(clock(row.ts))
                .when(row.from_me, |this| this.child(ticks(row.ack, t))),
        );
    }

    let mut body = content(view, t);
    if row.edited {
        body = body.child(
            div()
                .text_size(px(10.))
                .text_color(t.muted_foreground)
                .child("(edited)"),
        );
    }
    outer.child(body).into_any_element()
}

/// A date separator: a rule each side of a centred label.
pub fn day_separator(label: String, cx: &App) -> AnyElement {
    let t = tokens(cx);
    div()
        .mx(px(16.))
        .my(px(12.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(div().flex_1().h(px(1.)).bg(t.border))
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.muted_foreground)
                .child(label),
        )
        .child(div().flex_1().h(px(1.)).bg(t.border))
        .into_any_element()
}

/// The "New" line marking where unread messages start.
pub fn unread_separator(cx: &App) -> AnyElement {
    let t = tokens(cx);
    div()
        .mx(px(16.))
        .my(px(4.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .flex_1()
                .h(px(1.))
                .bg(Tokens::with_alpha(t.destructive, 0.6)),
        )
        .child(
            div()
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.destructive)
                .child("New"),
        )
        .into_any_element()
}
