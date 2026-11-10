//! Small building blocks shared across the interface: icons, avatars, badges, time text.

use gpui::prelude::*;
use gpui::{App, Div, Hsla, Pixels, SharedString, Stateful, div, hsla, px, svg};
use whatsapp_rust::chrono::{Datelike, Local, TimeZone};

use super::theme::{R_MD, Tokens, tokens};

/// A Lucide icon from `assets/icons`: 16px and the surrounding text colour unless set.
#[derive(IntoElement)]
pub struct Ico {
    name: SharedString,
    size: Pixels,
    color: Option<Hsla>,
}

impl Ico {
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn text_color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }
}

impl RenderOnce for Ico {
    fn render(self, window: &mut gpui::Window, _: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or_else(|| window.text_style().color);
        svg()
            .path(self.name)
            .size(self.size)
            .flex_shrink_0()
            .text_color(color)
    }
}

pub fn icon(name: &str) -> Ico {
    Ico {
        name: SharedString::from(format!("icons/{name}.svg")),
        size: px(16.),
        color: None,
    }
}

/// Up to two initials for an avatar fallback.
pub fn initials(title: &str) -> String {
    let mut letters = title
        .split_whitespace()
        .filter_map(|word| word.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .flat_map(char::to_uppercase)
        .collect::<String>();
    if letters.is_empty() {
        letters = title.chars().filter(|c| c.is_alphanumeric()).take(1).collect();
    }
    if letters.is_empty() {
        letters.push('#');
    }
    letters
}

fn hue_of(seed: &str) -> f32 {
    // FNV-1a: stable across runs, so a person keeps their colour.
    let mut hash: u32 = 0x811c9dc5;
    for byte in seed.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    (hash % 360) as f32 / 360.
}

/// Tint pair (fill, text) for an avatar fallback or a name colour.
pub fn tint_of(seed: &str, dark: bool) -> (Hsla, Hsla) {
    let hue = hue_of(seed);
    if dark {
        (hsla(hue, 0.42, 0.2, 1.), hsla(hue, 0.7, 0.78, 1.))
    } else {
        (hsla(hue, 0.6, 0.9, 1.), hsla(hue, 0.55, 0.36, 1.))
    }
}

/// Rounded-square avatar: the picture if one is on disk, else initials on a tint.
pub fn avatar(seed: &str, title: &str, size: f32, picture: Option<std::path::PathBuf>, cx: &App) -> Div {
    let t = tokens(cx);
    let radius = (size * 0.24).round().max(4.);
    let (fill, text) = tint_of(seed, t.dark);
    let base = div()
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(radius))
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center();
    match picture {
        Some(path) => base.child(
            gpui::img(path)
                .size(px(size))
                .rounded(px(radius))
                .object_fit(gpui::ObjectFit::Cover),
        ),
        None => base
            .bg(fill)
            .text_color(text)
            .text_size(px((size * 0.4).round().max(9.)))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(initials(title)),
    }
}

/// Unread / mention count pill.
pub fn count_badge(count: i64, muted: bool, cx: &App) -> Div {
    let t = tokens(cx);
    let label = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    div()
        .h(px(18.))
        .min_w(px(18.))
        .px(px(5.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(if muted { t.fg(0.16) } else { t.primary })
        .text_color(if muted { t.foreground } else { t.primary_foreground })
        .text_size(px(10.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(label)
}

/// Ghost icon button: 32 x 32, muted icon that brightens on hover.
pub fn icon_button(id: impl Into<gpui::ElementId>, name: &str, cx: &App) -> Stateful<Div> {
    let t = tokens(cx);
    let hover = if t.dark { t.fg(0.07) } else { t.accent };
    let fg = t.foreground;
    div()
        .id(id)
        .size(px(32.))
        .flex_shrink_0()
        .rounded(R_MD)
        .flex()
        .items_center()
        .justify_center()
        .text_color(t.muted_foreground)
        .cursor_pointer()
        .hover(move |style| style.bg(hover).text_color(fg))
        .child(icon(name))
}

/// Tooltip in the design's style.
pub fn tip(text: impl Into<SharedString>) -> impl Fn(&mut gpui::Window, &mut App) -> gpui::AnyView + 'static {
    let text: SharedString = text.into();
    move |window, cx| gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
}

fn local(ts: i64) -> Option<whatsapp_rust::chrono::DateTime<Local>> {
    Local.timestamp_opt(ts, 0).single()
}

/// `19:53`
pub fn clock(ts: i64) -> String {
    local(ts).map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

fn days_ago(ts: i64) -> Option<i64> {
    let then = local(ts)?.date_naive();
    let today = Local::now().date_naive();
    Some((today - then).num_days())
}

/// Chat-list time: the clock today, "Yesterday", a weekday this week, else the date.
pub fn list_time(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    let Some(time) = local(ts) else {
        return String::new();
    };
    match days_ago(ts) {
        Some(0) => time.format("%H:%M").to_string(),
        Some(1) => "Yesterday".to_string(),
        Some(d) if (2..7).contains(&d) => time.format("%A").to_string(),
        _ => time.format("%d.%m.%Y").to_string(),
    }
}

/// Date separator label: "Today", "Yesterday", or a long date.
pub fn day_label(ts: i64) -> String {
    let Some(time) = local(ts) else {
        return String::new();
    };
    match days_ago(ts) {
        Some(0) => "Today".to_string(),
        Some(1) => "Yesterday".to_string(),
        _ if time.year() == Local::now().year() => time.format("%A, %-d %B").to_string(),
        _ => time.format("%-d %B %Y").to_string(),
    }
}

/// Message header time: "Today at 19:53", "Yesterday at 08:10", "12.03.2026 19:53".
pub fn header_time(ts: i64) -> String {
    let Some(time) = local(ts) else {
        return String::new();
    };
    match days_ago(ts) {
        Some(0) => format!("Today at {}", time.format("%H:%M")),
        Some(1) => format!("Yesterday at {}", time.format("%H:%M")),
        _ => time.format("%d.%m.%Y %H:%M").to_string(),
    }
}

/// Day number since the epoch in local time, for "did the day change" checks.
pub fn local_day(ts: i64) -> i64 {
    local(ts)
        .map(|t| t.date_naive().num_days_from_ce() as i64)
        .unwrap_or(0)
}

/// "1:05" for a duration in seconds.
pub fn duration(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// "1.4 MB"
pub fn file_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024. && unit < UNITS.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[allow(dead_code)]
fn _unused(_: &Tokens) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_take_first_letters_of_two_words() {
        assert_eq!(initials("ada lovelace"), "AL");
        assert_eq!(initials("Solo"), "S");
        assert_eq!(initials("+90 555"), "95");
        assert_eq!(initials(""), "#");
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(file_size(512), "512 B");
        assert_eq!(file_size(1536), "1.5 KB");
        assert_eq!(duration(65), "1:05");
    }
}
