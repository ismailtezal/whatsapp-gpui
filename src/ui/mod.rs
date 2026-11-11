//! The graphical interface.

mod chat;
mod message;
mod shell;
pub mod theme;
mod widgets;

use std::borrow::Cow;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{App, AssetSource, Bounds, SharedString, WindowBounds, WindowOptions, px, size};
use gpui_component::{Root, TitleBar};

use crate::backend::Backend;
use crate::store::Store;

/// Icons and fonts shipped with the app, falling back to the component library's own.
#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
struct Embedded;

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if let Some(file) = Embedded::get(path) {
            return Ok(Some(file.data));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut names: Vec<SharedString> = Embedded::iter()
            .filter(|name| name.starts_with(path))
            .map(|name| SharedString::from(name.to_string()))
            .collect();
        names.extend(gpui_kit_assets::Assets.list(path)?);
        Ok(names)
    }
}

/// Opens the main window and runs until it is closed.
pub fn run() {
    let store = match Store::open_default() {
        Ok(store) => Arc::new(store),
        Err(error) => {
            eprintln!("could not open the chat database: {error}");
            return;
        }
    };

    gpui_platform::application().with_assets(Assets).run(move |cx: &mut App| {
        gpui_component::init(cx);
        if let Some(font) = Embedded::get("fonts/Figtree-Variable.ttf") {
            let _ = cx.text_system().add_fonts(vec![font.data]);
        }
        // A test run (any appearance override, or the offline test mode) must not save
        // its appearance over the owner's choice.
        if ["WA_THEME", "WA_STYLE", "WA_PALETTE", "WA_OFFLINE"]
            .iter()
            .any(|name| std::env::var(name).is_ok())
        {
            theme::set_volatile();
        }
        // Development aid: WA_THEME=light or dark overrides the remembered mode.
        let forced = std::env::var("WA_THEME").ok().map(|theme| theme != "light");
        theme::restore(store.clone(), forced, cx);
        theme::restore_style(cx);
        // Development aid: WA_STYLE picks a surface style for this run.
        if let Ok(name) = std::env::var("WA_STYLE") {
            theme::set_style(theme::Style::from_name(&name), cx);
        }
        // Development aid: WA_PALETTE picks a palette for this run.
        if let Ok(name) = std::env::var("WA_PALETTE") {
            theme::set_palette(theme::Palette::from_name(&name), cx);
        }

        // Call logs arrive inside history blobs; older builds did not store them.
        if store.kv("calls_imported").ok().flatten().is_none()
            && crate::import::import_saved_history(&store).is_ok()
        {
            let _ = store.set_kv("calls_imported", "1");
        }

        let backend = Backend::start(store.clone());
        crate::avatars::init(backend.clone());
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        // Interface test mode: the window is kept off the screen, out of the owner's way.
        let offscreen = std::env::var("WA_OFFSCREEN").is_ok_and(|value| !value.is_empty());
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(940.), px(500.))),
                // Glass needs a see-through window from the start.
                window_background: theme::tokens(cx).window_backdrop(),
                // Never drop to 30 frames a second to save power or heat: the owner
                // wants the interface at the full refresh rate of the screen.
                low_power_frame_interval: None,
                thermal_frame_interval: None,
                // A window in the background is limited to 30 frames a second, except
                // while measuring (WA_PERF), where the window is rarely in front.
                inactive_frame_interval: if std::env::var("WA_PERF").is_ok() {
                    None
                } else {
                    Some(std::time::Duration::from_micros(33_333))
                },
                ..TitleBar::window_options()
            },
            |window, cx| {
                if offscreen {
                    move_off_screen(window);
                }
                let shell = cx.new(|cx| shell::Shell::new(backend, window, cx));
                cx.new(|cx| Root::new(shell, window, cx))
            },
        )
        .expect("open main window");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        if !offscreen {
            cx.activate(true);
        }
    });
}

/// Moves the window far outside every screen without changing its size. GPUI itself
/// keeps new windows on a screen, so this goes to Windows directly.
fn move_off_screen(window: &gpui::Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    };
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    if let RawWindowHandle::Win32(win32) = handle.as_raw() {
        let hwnd = HWND(win32.hwnd.get() as *mut std::ffi::c_void);
        // SAFETY: the handle belongs to this live window; the call only moves it.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                -12000,
                -12000,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}
