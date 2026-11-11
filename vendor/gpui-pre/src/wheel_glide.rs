//! whatsapp-gpui patch: smooth mouse-wheel scrolling.
//!
//! A mouse wheel reports whole notches. Applied at once, each notch makes the content
//! jump; here the distance is queued and paid out over the next few frames, fast at
//! first and easing to a stop, the way browsers and native Windows apps scroll.
//! Touchpads report exact pixel distances and are not touched by this.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use crate::{Pixels, Point, point, px};

/// Pixels scrolled per wheel line. Windows sends three lines per notch by default, so a
/// notch moves 120 pixels.
pub(crate) const WHEEL_LINE: f32 = 40.;

/// How quickly the queued distance is paid out: after this many seconds about 63% of
/// it has been applied, after three times this about 95%.
const TIME_CONSTANT: f32 = 0.045;

#[derive(Default)]
pub(crate) struct WheelGlide {
    remaining: (f32, f32),
    last_tick: Option<Instant>,
    running: bool,
}

impl WheelGlide {
    /// Queues a distance. Returns true when the caller must start the frame loop that
    /// calls [`WheelGlide::step`].
    pub(crate) fn push(&mut self, delta: Point<Pixels>) -> bool {
        let (x, y) = (delta.x / px(1.), delta.y / px(1.));
        // Reversing direction cancels what was still queued the other way.
        if self.remaining.0 * x < 0. {
            self.remaining.0 = 0.;
        }
        if self.remaining.1 * y < 0. {
            self.remaining.1 = 0.;
        }
        self.remaining.0 += x;
        self.remaining.1 += y;
        let start = !self.running;
        self.running = true;
        start
    }

    /// The distance to apply this frame, and whether more is queued.
    pub(crate) fn step(&mut self, now: Instant) -> (Point<Pixels>, bool) {
        let elapsed = self
            .last_tick
            .map(|last| (now - last).as_secs_f32())
            .unwrap_or(1. / 120.)
            .clamp(0.001, 0.05);
        self.last_tick = Some(now);
        let share = 1. - (-elapsed / TIME_CONSTANT).exp();
        let take = |remaining: f32| {
            if remaining.abs() < 0.75 {
                remaining
            } else {
                remaining * share
            }
        };
        let step = (take(self.remaining.0), take(self.remaining.1));
        self.remaining.0 -= step.0;
        self.remaining.1 -= step.1;
        let more = self.remaining.0 != 0. || self.remaining.1 != 0.;
        if !more {
            self.running = false;
            self.last_tick = None;
        }
        (point(px(step.0), px(step.1)), more)
    }
}

thread_local! {
    /// Glides of plain scrolling elements, keyed by the address of their scroll offset.
    static GLIDES: RefCell<HashMap<usize, WheelGlide>> = RefCell::new(HashMap::new());
}

/// Queues a distance for the scrolling element identified by `key`. Returns true when
/// the caller must start the frame loop.
pub(crate) fn push_for(key: usize, delta: Point<Pixels>) -> bool {
    GLIDES.with(|glides| glides.borrow_mut().entry(key).or_default().push(delta))
}

/// The distance to apply this frame for `key`, and whether more is queued.
pub(crate) fn step_for(key: usize, now: Instant) -> (Point<Pixels>, bool) {
    GLIDES.with(|glides| {
        let mut glides = glides.borrow_mut();
        let Some(glide) = glides.get_mut(&key) else {
            return (Point::default(), false);
        };
        let (step, more) = glide.step(now);
        if !more {
            glides.remove(&key);
        }
        (step, more)
    })
}
