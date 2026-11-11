//! Motion: every animation the app has, in one place so that they agree.
//!
//! The rules, after Emil Kowalski's notes on interface animation:
//! - Things that appear because of a click (menus, pickers, dialogs, viewers) enter
//!   with a strong ease-out, fast: 140 to 240 ms. They leave at once; the way out is
//!   never slower than the way in.
//! - Nothing enters from nothing: a small rise and a fade, never a jump from zero size.
//!   A surface enters from the side its trigger is on.
//! - What a key press opens, and what is seen hundreds of times a day (switching chat,
//!   hovering, list selection), does not animate at all.
//! - A press is answered at once (see `pressable`).
//! - Only position offsets and opacity move, never layout sizes.
//!
//! GPUI's animation wrapper already honours the system's "reduce motion" setting: the
//! end state is drawn straight away.

use std::time::Duration;

use gpui::{Animation, AnimationElement, AnimationExt, ElementId, IntoElement, Styled, px};

/// A strong ease-out for things entering: fast at first, so the response is instant.
/// `cubic-bezier(0.23, 1, 0.32, 1)`.
fn ease_out() -> impl Fn(f32) -> f32 {
    gpui_component::animation::cubic_bezier(0.23, 1., 0.32, 1.)
}

/// The curve of a drawer sliding in. `cubic-bezier(0.32, 0.72, 0, 1)`.
fn ease_drawer() -> impl Fn(f32) -> f32 {
    gpui_component::animation::cubic_bezier(0.32, 0.72, 0., 1.)
}

/// Development aid: WA_SLOWMO=<factor> stretches every animation, so that its curve,
/// its start and its end can be looked at in screenshots.
fn stretch(millis: u64) -> Duration {
    static FACTOR: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    let factor = *FACTOR.get_or_init(|| {
        std::env::var("WA_SLOWMO")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|factor| *factor > 0.)
            .unwrap_or(1.)
    });
    Duration::from_secs_f32(millis as f32 / 1000. * factor)
}

/// Where a surface enters from: the side its trigger is on.
#[derive(Clone, Copy)]
pub enum From {
    Above,
    Below,
    Right,
}

fn enter<E: IntoElement + Styled + 'static>(
    element: E,
    id: impl Into<ElementId>,
    millis: u64,
    distance: f32,
    from: From,
    easing: impl Fn(f32) -> f32 + 'static,
) -> AnimationElement<E> {
    element.with_animation(
        id,
        Animation::new(stretch(millis)).with_easing(easing),
        move |element, progress| {
            let away = (1. - progress) * distance;
            let element = element.relative().opacity(progress);
            match from {
                From::Above => element.top(px(-away)),
                From::Below => element.top(px(away)),
                From::Right => element.left(px(away)),
            }
        },
    )
}

/// A menu or small popover opening: 140 ms, a 4 pixel move and a fade.
pub fn pop<E: IntoElement + Styled + 'static>(element: E, id: impl Into<ElementId>, from: From) -> AnimationElement<E> {
    enter(element, id, 140, 4., from, ease_out())
}

/// A dialog or card arriving: 200 ms, an 8 pixel rise and a fade.
pub fn settle<E: IntoElement + Styled + 'static>(element: E, id: impl Into<ElementId>, from: From) -> AnimationElement<E> {
    enter(element, id, 200, 8., from, ease_out())
}

/// A side panel sliding in: 240 ms on the drawer curve.
pub fn slide_in<E: IntoElement + Styled + 'static>(element: E, id: impl Into<ElementId>) -> AnimationElement<E> {
    enter(element, id, 240, 24., From::Right, ease_drawer())
}

/// A new message arriving in the open conversation: 180 ms, a 6 pixel rise and a fade.
pub fn arrive<E: IntoElement + Styled + 'static>(element: E, id: impl Into<ElementId>) -> AnimationElement<E> {
    enter(element, id, 180, 6., From::Below, ease_out())
}

/// A full-window layer (viewer, editor, dialog backdrop) fading in: 160 ms.
pub fn fade<E: IntoElement + Styled + 'static>(element: E, id: impl Into<ElementId>) -> AnimationElement<E> {
    element.with_animation(
        id,
        Animation::new(stretch(160)).with_easing(ease_out()),
        |element, progress| element.opacity(progress),
    )
}

/// How long a row counts as newly arrived, a little longer than `arrive` runs.
pub fn arrival() -> Duration {
    stretch(260)
}
