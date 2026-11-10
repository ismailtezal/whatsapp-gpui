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
        theme::install(true, cx);

        let backend = Backend::start(store.clone());
        crate::avatars::init(backend.clone());
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(940.), px(500.))),
                ..TitleBar::window_options()
            },
            |window, cx| {
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
        cx.activate(true);
    });
}
