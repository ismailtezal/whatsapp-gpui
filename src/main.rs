use gpui::prelude::*;
use gpui::{App, Bounds, Context, Window, WindowBounds, WindowOptions, div, px, size};
use gpui_component::{ActiveTheme, Root, TitleBar};

struct Shell;

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(TitleBar::new().child("WhatsApp"))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child("whatsapp-gpui: window is up"),
            )
    }
}

fn main() {
    gpui_platform::application()
        .with_assets(gpui_kit_assets::Assets)
        .run(|cx: &mut App| {
            gpui_component::init(cx);
            let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(720.), px(480.))),
                    ..TitleBar::window_options()
                },
                |window, cx| {
                    let view = cx.new(|_| Shell);
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("open main window");
            cx.activate(true);
        });
}
