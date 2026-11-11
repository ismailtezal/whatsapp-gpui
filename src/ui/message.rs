//! One message, prepared for display and drawn as a flat row (avatar gutter, author line,
//! body), following the design's "flat message rows, not bubbles".

use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Div, FontStyle, FontWeight, HighlightStyle, Image, ImageFormat,
    InteractiveText, SharedString, StrikethroughStyle, StyledText, UnderlineStyle, Window, div, px,
};
use whatsapp_rust::buffa::Message as _;

use super::theme::{GUTTER, R_LG, R_MD, Tokens, tokens};
use super::widgets::{avatar, clock, duration, file_size, header_time, icon, tint_of};
use crate::content::{self, preview_label};
use crate::model::{Ack, MsgKind, MsgRow};
use crate::richtext::{Rich, SpanStyle};
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
    /// A preview picture on disk (made for downloaded videos).
    pub thumb_file: Option<std::path::PathBuf>,
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
    /// The text with formatting, links and mentions resolved.
    pub rich: Option<Rich>,
    pub avatar: Option<std::path::PathBuf>,
    pub poll: Option<crate::polls::PollView>,
    pub extras: Extras,
    /// Further photos shown in the same grid as this one: (message ID, file on disk).
    pub album: Vec<(String, Option<String>)>,
}

/// What a click on a part of a row does.
pub type ClickFn = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Handlers the conversation view attaches to a row.
#[derive(Clone, Default)]
pub struct RowHooks {
    /// Open (or download) the media of this message.
    pub media: Option<ClickFn>,
    /// The clip the audio player has loaded, if any.
    pub audio: Option<crate::audio::Playback>,
    /// Jump to the message this one replies to.
    pub quote: Option<ClickFn>,
    /// Vote for the option at this index of a poll.
    pub poll_vote: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>>,
    /// Open the chat with this JID.
    pub open_chat: Option<Rc<dyn Fn(String, &mut Window, &mut App)>>,
    /// Open the photo of the message with this ID (for album tiles).
    pub media_id: Option<Rc<dyn Fn(String, &mut Window, &mut App)>>,
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
        .or_else(|| view.media.thumb_file.clone().map(gpui::img))
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

/// Message text with its formatting, clickable links and mention pills.
fn body_text(view: &MsgView, fallback: &str, t: &Tokens) -> Div {
    let base = div()
        .text_size(px(14.))
        .line_height(px(20.3))
        .text_color(t.fg(0.9));
    let Some(rich) = view.rich.as_ref().filter(|r| !r.spans.is_empty()) else {
        let text = view.rich.as_ref().map(|r| r.text.as_str()).unwrap_or(fallback);
        return base.child(SharedString::from(text.to_string()));
    };
    // Highlights must be in order and must not overlap: split the text at every span
    // boundary and merge the styles that cover each piece.
    let mut cuts: Vec<usize> = rich
        .spans
        .iter()
        .flat_map(|s| [s.range.start, s.range.end])
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut highlights = Vec::new();
    for pair in cuts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let mut style = HighlightStyle::default();
        let mut any = false;
        for span in rich.spans.iter().filter(|s| s.range.start <= start && end <= s.range.end) {
            any = true;
            match &span.style {
                SpanStyle::Bold => style.font_weight = Some(FontWeight::SEMIBOLD),
                SpanStyle::Italic => style.font_style = Some(FontStyle::Italic),
                SpanStyle::Strike => {
                    style.strikethrough = Some(StrikethroughStyle {
                        thickness: px(1.),
                        color: None,
                    })
                }
                SpanStyle::Code => {
                    style.background_color = Some(t.fg(0.08));
                    style.color = Some(t.foreground);
                }
                SpanStyle::Link(_) => {
                    style.color = Some(t.link);
                    style.underline = Some(UnderlineStyle {
                        thickness: px(1.),
                        color: Some(Tokens::with_alpha(t.link, 0.5)),
                        wavy: false,
                    });
                }
                SpanStyle::Mention(_) => {
                    style.color = Some(t.mention_text());
                    style.background_color = Some(t.tint(0.16));
                    style.font_weight = Some(FontWeight::MEDIUM);
                }
            }
        }
        if any {
            highlights.push((start..end, style));
        }
    }
    let styled = StyledText::new(SharedString::from(rich.text.clone())).with_highlights(highlights);
    let links: Vec<(std::ops::Range<usize>, String)> = rich
        .spans
        .iter()
        .filter_map(|s| match &s.style {
            SpanStyle::Link(url) => Some((s.range.clone(), url.clone())),
            _ => None,
        })
        .collect();
    if links.is_empty() {
        return base.child(styled);
    }
    let ranges: Vec<std::ops::Range<usize>> = links.iter().map(|(r, _)| r.clone()).collect();
    let urls: Vec<String> = links.into_iter().map(|(_, url)| url).collect();
    base.child(
        InteractiveText::new(("text", view.row.rowid as usize), styled).on_click(
            ranges,
            move |index, _window, cx| {
                if let Some(url) = urls.get(index) {
                    cx.open_url(url);
                }
            },
        ),
    )
}

/// Makes a media element clickable when the conversation attached a handler.
fn clickable(element: Div, id: (&'static str, usize), hook: &Option<ClickFn>) -> AnyElement {
    match hook {
        Some(hook) => {
            let hook = hook.clone();
            element
                .id(id)
                .cursor_pointer()
                .on_click(move |event, window, cx| hook(event, window, cx))
                .into_any_element()
        }
        None => element.into_any_element(),
    }
}

/// The content of a message below its header line.
fn content(view: &MsgView, hooks: &RowHooks, t: &Tokens) -> Div {
    let key = view.row.rowid as usize;
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
                column = column.child(body_text(view, text, t));
            }
        }
        Image => {
            column = column.child(if view.album.is_empty() {
                clickable(picture(view, t, None), ("media", key), &hooks.media)
            } else {
                album_grid(view, hooks, t)
            });
            if let Some(text) = text {
                column = column.child(body_text(view, text, t));
            }
        }
        Video | VideoNote => {
            column = column.child(clickable(picture(view, t, Some("play")), ("media", key), &hooks.media));
            if let Some(text) = text {
                column = column.child(body_text(view, text, t));
            }
        }
        Gif => {
            column = column.child(clickable(picture(view, t, Some("film")), ("media", key), &hooks.media));
            if let Some(text) = text {
                column = column.child(body_text(view, text, t));
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
            // A small player: play / pause, a progress track, elapsed and total time.
            let state = hooks.audio.as_ref().filter(|p| p.id == row.id);
            let playing = state.is_some_and(|p| p.playing);
            let total = state
                .map(|p| p.duration_secs)
                .filter(|d| *d > 0.)
                .unwrap_or(view.media.seconds as f64);
            let position = state.map(|p| p.position_secs).unwrap_or(0.);
            let fraction = if total > 0. {
                (position / total).clamp(0., 1.) as f32
            } else {
                0.
            };
            let times = if state.is_some() {
                format!("{} / {}", duration(position as u32), duration(total as u32))
            } else {
                duration(total as u32)
            };
            let card = div()
                .w(px(300.))
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
                        .rounded_full()
                        .bg(t.primary)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(t.primary_foreground)
                        .child(icon(if playing { "pause" } else { "play" })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div().h(px(4.)).rounded_full().bg(t.fg(0.12)).child(
                                div()
                                    .h_full()
                                    .w(gpui::relative(fraction))
                                    .rounded_full()
                                    .bg(t.primary),
                            ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(11.))
                                .text_color(t.muted_foreground)
                                .child(icon(if row.kind == Voice { "mic" } else { "music" }).size(px(11.)))
                                .child(times),
                        ),
                );
            column = column.child(clickable(card, ("media", key), &hooks.media));
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
            column = column.child(clickable(
                info_card("file-text", name.clone(), detail, t),
                ("media", key),
                &hooks.media,
            ));
            if let Some(caption) = text.filter(|c| *c != name) {
                column = column.child(body_text(view, caption, t));
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
                    .child(SharedString::from(
                        text.unwrap_or("This message type is not shown yet. Open it on your phone.").to_string(),
                    )),
            );
        }
        Poll if view.poll.is_some() => {
            let Some(poll) = view.poll.as_ref() else {
                return column;
            };
            // Bars are relative to the most-voted option.
            let most = poll.options.iter().map(|o| o.votes).max().unwrap_or(0).max(1);
            let mut card = div()
                .w(px(340.))
                .rounded(R_LG)
                .border_1()
                .border_color(t.border)
                .bg(t.card)
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.foreground)
                        .child(SharedString::from(poll.question.clone())),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(t.muted_foreground)
                        .child(if poll.selectable == 1 {
                            "Select one"
                        } else {
                            "Select one or more"
                        }),
                );
            for (index, option) in poll.options.iter().enumerate() {
                let fraction = option.votes as f32 / most as f32;
                let mine = option.mine;
                let row = div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .size(px(16.))
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(if mine { t.primary } else { t.fg(0.3) })
                                    .when(mine, |this| this.bg(t.primary))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(t.primary_foreground)
                                    .when(mine, |this| this.child(icon("check").size(px(10.)))),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(14.))
                                    .text_color(t.fg(0.9))
                                    .child(SharedString::from(option.name.clone())),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(t.muted_foreground)
                                    .child(option.votes.to_string()),
                            ),
                    )
                    .child(
                        div().h(px(4.)).rounded_full().bg(t.fg(0.10)).child(
                            div()
                                .h_full()
                                .w(gpui::relative(fraction))
                                .rounded_full()
                                .bg(t.primary),
                        ),
                    );
                card = card.child(match &hooks.poll_vote {
                    Some(hook) => {
                        let hook = hook.clone();
                        row.id(("poll-option", key * 16 + index))
                            .cursor_pointer()
                            .on_click(move |_, window, cx| hook(index, window, cx))
                            .into_any_element()
                    }
                    None => row.into_any_element(),
                });
            }
            column = column.child(card);
        }
        Location | LiveLocation => {
            let label = preview_label(row.kind, None);
            let place = text.map(str::to_string).unwrap_or_else(|| label.clone());
            let card = info_card(
                "map-pin",
                place,
                match (view.extras.lat, view.extras.lon) {
                    (Some(lat), Some(lon)) => format!("{lat:.5}, {lon:.5} · open in maps"),
                    _ => label,
                },
                t,
            );
            column = column.child(match (view.extras.lat, view.extras.lon) {
                (Some(lat), Some(lon)) => {
                    let url = format!("https://www.google.com/maps?q={lat},{lon}");
                    card.id(("place", key))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| cx.open_url(&url))
                        .into_any_element()
                }
                _ => card.into_any_element(),
            });
        }
        Contact => {
            let name = text.map(str::to_string).unwrap_or_else(|| "Contact".to_string());
            let number = view.extras.contact_number.clone();
            let card = info_card(
                "contact",
                name,
                match &number {
                    Some(number) => format!("+{number} · message"),
                    None => "Contact card".to_string(),
                },
                t,
            );
            column = column.child(match (number, &hooks.open_chat) {
                (Some(number), Some(hook)) => {
                    let hook = hook.clone();
                    card.id(("contact", key))
                        .cursor_pointer()
                        .on_click(move |_, window, cx| hook(format!("{number}@s.whatsapp.net"), window, cx))
                        .into_any_element()
                }
                _ => card.into_any_element(),
            });
        }
        Event => {
            let name = text.map(str::to_string).unwrap_or_else(|| "Event".to_string());
            let mut detail = String::new();
            let mut about = String::new();
            if let Some((start, place, description)) = &view.extras.event {
                if *start > 0 {
                    detail = header_time(*start);
                }
                if !place.is_empty() {
                    detail = if detail.is_empty() {
                        place.clone()
                    } else {
                        format!("{detail} · {place}")
                    };
                }
                about = description.clone();
            }
            column = column.child(info_card("calendar", name, detail, t));
            if !about.is_empty() {
                column = column.child(
                    div()
                        .max_w(px(384.))
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(t.fg(0.8))
                        .child(SharedString::from(about)),
                );
            }
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

    if let Some(link) = &view.extras.link {
        column = column.child(link_card(link, key, t));
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
pub fn message_row(view: &MsgView, hooks: &RowHooks, cx: &App) -> AnyElement {
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
        let line = div()
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
            .child(div().truncate().child(quote.text.clone()));
        outer = outer.child(clickable(line, ("quote", view.row.rowid as usize), &hooks.quote));
    }

    if view.head {
        let (_, name_color) = tint_of(&view.author_seed, t.dark);
        outer = outer
            .child(
                div()
                    .absolute()
                    .left(px(16.))
                    .top(px(if view.quote.is_some() { 22. } else { 2. }))
                    .child(avatar(&view.author_seed, &view.author, 32., view.avatar.clone(), cx)),
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

    let mut body = content(view, hooks, t);
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
        .w_full()
        .px(px(16.))
        .py(px(12.))
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
        .w_full()
        .px(px(16.))
        .py(px(4.))
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

/// The preview WhatsApp attaches to a message that contains a link.
#[derive(Clone)]
pub struct LinkPreview {
    pub url: String,
    pub title: String,
    pub description: String,
    pub thumb: Option<Arc<Image>>,
}

/// Details of the less common message kinds (location, contact card, link preview).
#[derive(Clone, Default)]
pub struct Extras {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// The WhatsApp number on a shared contact card, digits only.
    pub contact_number: Option<String>,
    pub link: Option<LinkPreview>,
    /// An event's start time, place and description.
    pub event: Option<(i64, String, String)>,
}

/// The WhatsApp number in a vCard: the digits of its `waid=` parameter.
pub fn vcard_number(vcard: &str) -> Option<String> {
    let start = vcard.find("waid=")? + "waid=".len();
    let digits: String = vcard[start..].chars().take_while(|c| c.is_ascii_digit()).collect();
    (!digits.is_empty()).then_some(digits)
}

pub fn extras_of(record: &wa::WebMessageInfo) -> Extras {
    let mut extras = Extras::default();
    let Some(message) = record.message.as_option() else {
        return extras;
    };
    let (base, _, _) = content::unwrap(message);
    if let Some(m) = base.location_message.as_option() {
        extras.lat = m.degrees_latitude;
        extras.lon = m.degrees_longitude;
    } else if let Some(m) = base.live_location_message.as_option() {
        extras.lat = m.degrees_latitude;
        extras.lon = m.degrees_longitude;
    } else if let Some(m) = base.contact_message.as_option() {
        extras.contact_number = m.vcard.as_deref().and_then(vcard_number);
    } else if let Some(m) = base.event_message.as_option() {
        extras.event = Some((
            m.start_time.unwrap_or(0),
            m.location
                .as_option()
                .and_then(|place| place.name.clone())
                .unwrap_or_default(),
            m.description.clone().unwrap_or_default(),
        ));
    } else if let Some(m) = base.extended_text_message.as_option() {
        let title = m.title.clone().unwrap_or_default();
        let url = m.matched_text.clone().unwrap_or_default();
        if !title.is_empty() && !url.is_empty() {
            extras.link = Some(LinkPreview {
                url,
                title,
                description: m.description.clone().unwrap_or_default(),
                thumb: m
                    .jpeg_thumbnail
                    .as_ref()
                    .filter(|bytes| !bytes.is_empty())
                    .map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes.clone()))),
            });
        }
    }
    extras
}

/// The card under a message that links somewhere: site title, description, thumbnail.
fn link_card(link: &LinkPreview, key: usize, t: &Tokens) -> AnyElement {
    let url = link.url.clone();
    div()
        .id(("link", key))
        .max_w(px(420.))
        .rounded(R_LG)
        .border_1()
        .border_l_2()
        .border_color(t.border)
        .bg(t.card)
        .p(px(12.))
        .flex()
        .gap(px(12.))
        .cursor_pointer()
        .on_click(move |_, _, cx| cx.open_url(&url))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .truncate()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.link)
                        .child(SharedString::from(link.title.clone())),
                )
                .when(!link.description.is_empty(), |this| {
                    this.child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .max_h(px(72.))
                            .overflow_hidden()
                            .text_color(t.fg(0.8))
                            .child(SharedString::from(link.description.clone())),
                    )
                })
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(t.muted_foreground)
                        .child(SharedString::from(link.url.clone())),
                ),
        )
        .children(link.thumb.clone().map(|thumb| {
            gpui::img(thumb)
                .size(px(64.))
                .flex_shrink_0()
                .rounded(R_MD)
                .object_fit(gpui::ObjectFit::Cover)
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_whatsapp_number_from_a_contact_card() {
        let vcard = "BEGIN:VCARD\nFN:Ada\nTEL;type=CELL;waid=15550001111:+1 555 000 1111\nEND:VCARD";
        assert_eq!(vcard_number(vcard).as_deref(), Some("15550001111"));
        assert_eq!(vcard_number("BEGIN:VCARD\nFN:No number\nEND:VCARD"), None);
    }
}

/// Several photos sent together, as a two-column grid of square tiles.
fn album_grid(view: &MsgView, hooks: &RowHooks, t: &Tokens) -> AnyElement {
    let first = (view.row.id.clone(), view.row.media_path.clone());
    let tile = 236.;
    let mut grid = div().w(px(tile * 2. + 4.)).flex().flex_wrap().gap(px(4.));
    for (slot, (id, path)) in std::iter::once(&first).chain(view.album.iter()).enumerate() {
        let mut frame = div()
            .size(px(tile))
            .rounded(R_LG)
            .border_1()
            .border_color(t.border)
            .bg(t.muted)
            .overflow_hidden();
        if let Some(path) = path {
            frame = frame.child(
                gpui::img(std::path::PathBuf::from(path))
                    .size(px(tile))
                    .object_fit(gpui::ObjectFit::Cover),
            );
        }
        grid = grid.child(match &hooks.media_id {
            Some(hook) => {
                let hook = hook.clone();
                let id = id.clone();
                frame
                    .id(("album", view.row.rowid as usize * 16 + slot))
                    .cursor_pointer()
                    .on_click(move |_, window, cx| hook(id.clone(), window, cx))
                    .into_any_element()
            }
            None => frame.into_any_element(),
        });
    }
    grid.into_any_element()
}
