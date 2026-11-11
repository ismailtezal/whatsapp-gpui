//! Frame-time measurement for development.
//!
//! With `WA_PERF=1` the app times the named sections of its interface code and prints a
//! line per section every two seconds: how often it ran, the average, and the worst
//! case. `frame` is the gap between two consecutive frames, so `1000 / avg` is the frame
//! rate while something is animating. Without the variable every call is a cheap check.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Default, Clone, Copy)]
struct Stat {
    count: u64,
    total: Duration,
    worst: Duration,
}

struct State {
    stats: BTreeMap<&'static str, Stat>,
    last_frame: Option<Instant>,
    note: Option<String>,
}

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("WA_PERF").is_ok_and(|value| !value.is_empty()))
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| {
        std::thread::spawn(report_loop);
        Mutex::new(State {
            stats: BTreeMap::new(),
            last_frame: None,
            note: None,
        })
    })
}

fn record(name: &'static str, elapsed: Duration) {
    if let Ok(mut state) = state().lock() {
        let stat = state.stats.entry(name).or_default();
        stat.count += 1;
        stat.total += elapsed;
        stat.worst = stat.worst.max(elapsed);
    }
}

fn report_loop() {
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let (stats, note) = match state().lock() {
            Ok(mut state) => (std::mem::take(&mut state.stats), state.note.take()),
            Err(_) => return,
        };
        if let Some(note) = note {
            eprintln!("note: {note}");
        }
        if stats.is_empty() {
            continue;
        }
        let mut line = String::from("perf:");
        for (name, stat) in stats {
            let average = stat.total.as_secs_f64() * 1000. / stat.count as f64;
            line.push_str(&format!(
                " {name} n={} avg={average:.2}ms max={:.2}ms |",
                stat.count,
                stat.worst.as_secs_f64() * 1000.
            ));
        }
        eprintln!("{line}");
    }
}

/// Times a section until the returned guard is dropped.
pub fn scope(name: &'static str) -> Scope {
    Scope {
        name,
        started: enabled().then(Instant::now),
    }
}

pub struct Scope {
    name: &'static str,
    started: Option<Instant>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            record(self.name, started.elapsed());
        }
    }
}

/// Remembers a line of state (for example the scroll position) to print with the next
/// report. Only the latest one is kept.
pub fn note(text: impl FnOnce() -> String) {
    if !enabled() {
        return;
    }
    if let Ok(mut state) = state().lock() {
        state.note = Some(text());
    }
}

/// Marks the start of a frame; the gap since the previous mark is recorded as `frame`.
/// Gaps over a quarter of a second are idle time, not slow frames, and are left out.
pub fn frame() {
    if !enabled() {
        return;
    }
    let now = Instant::now();
    let gap = match state().lock() {
        Ok(mut state) => state.last_frame.replace(now).map(|last| now - last),
        Err(_) => None,
    };
    if let Some(gap) = gap.filter(|gap| *gap < Duration::from_millis(250)) {
        record("frame", gap);
    }
}

/// The next step of the automatic scroll, in pixels: up for a while, then back down.
pub fn scroll_step() -> f32 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static TICK: AtomicU64 = AtomicU64::new(0);
    let tick = TICK.fetch_add(1, Ordering::Relaxed) % 800;
    if tick < 400 { -14. } else { 14. }
}

/// Whether the automatic scroll used for measuring is on (`WA_PERF_SCROLL=1`).
pub fn auto_scroll() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("WA_PERF_SCROLL").is_ok_and(|value| !value.is_empty()))
}
