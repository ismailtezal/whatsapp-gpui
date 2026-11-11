//! Design tokens: the "modern" design of diskurt, neutral palette.
//!
//! Values come from `docs/design-spec.md`. Surfaces step up in tiny lightness increments,
//! lines are translucent, and hover / selected states are the foreground colour at a low
//! alpha rather than dedicated colours.

use gpui::{App, BoxShadow, Global, Hsla, Pixels, hsla, point, px, rgb};

/// Corner radii at the design's "lg" radius setting.
pub const R_SM: Pixels = px(6.);
pub const R_MD: Pixels = px(8.);
pub const R_LG: Pixels = px(10.);
pub const R_XL: Pixels = px(14.);
pub const R_2XL: Pixels = px(18.);

pub const TITLE_BAR_H: Pixels = px(32.);
pub const RAIL_W: Pixels = px(68.);
pub const COLUMN_W: Pixels = px(320.);
pub const HEADER_H: Pixels = px(48.);
/// Left gutter of a message row: 16px margin, 32px avatar, 12px gap.
pub const GUTTER: Pixels = px(60.);

pub const FONT_UI: &str = "Figtree";

#[derive(Clone, Debug)]
pub struct Tokens {
    pub dark: bool,
    pub style: Style,
    /// What is behind the window in the Liquid Glass style.
    pub backdrop: Backdrop,
    pub background: Hsla,
    pub foreground: Hsla,
    pub card: Hsla,
    pub popover: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub secondary: Hsla,
    pub muted: Hsla,
    pub muted_foreground: Hsla,
    pub accent: Hsla,
    pub destructive: Hsla,
    pub border: Hsla,
    pub input: Hsla,
    pub ring: Hsla,
    pub sidebar: Hsla,
    pub sidebar_foreground: Hsla,
    pub sidebar_accent: Hsla,
    pub sidebar_border: Hsla,
    pub rail: Hsla,
    pub tooltip: Hsla,
    pub tooltip_foreground: Hsla,
    pub online: Hsla,
    pub idle: Hsla,
    pub offline: Hsla,
    pub link: Hsla,
    pub mention: Hsla,
    pub code: Hsla,
}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

impl Tokens {
    pub fn dark() -> Self {
        Tokens {
            dark: true,
            style: Style::Modern,
            backdrop: Backdrop::Solid,
            background: hex(0x07080b),
            foreground: hex(0xedeef0),
            card: hex(0x0c0e12),
            popover: hex(0x0e1115),
            primary: hex(0x4092e7),
            primary_foreground: hex(0xfcfcfc),
            secondary: hex(0x15171c),
            muted: hex(0x14161b),
            muted_foreground: hex(0x8a8f98),
            accent: hex(0x171a1f),
            destructive: hex(0xed4a49),
            border: hsla(0., 0., 1., 0.075),
            input: hsla(0., 0., 1., 0.11),
            ring: hex(0x4092e7),
            sidebar: hex(0x040507),
            sidebar_foreground: hex(0xdcdee1),
            sidebar_accent: hex(0x111418),
            sidebar_border: hsla(0., 0., 1., 0.06),
            rail: hex(0x020304),
            tooltip: hex(0x212429),
            tooltip_foreground: hex(0xf4f5f7),
            online: hex(0x42c070),
            idle: hex(0xf5ae39),
            offline: hex(0x6e7279),
            link: hex(0x65b2f1),
            mention: hex(0xefb146),
            code: hex(0x040507),
        }
    }

    pub fn light() -> Self {
        Tokens {
            dark: false,
            style: Style::Modern,
            backdrop: Backdrop::Solid,
            background: hex(0xfbfcfd),
            foreground: hex(0x15181e),
            card: hex(0xffffff),
            popover: hex(0xffffff),
            primary: hex(0x2474cf),
            primary_foreground: hex(0xfcfcfc),
            secondary: hex(0xeff0f3),
            muted: hex(0xf0f2f4),
            muted_foreground: hex(0x686c74),
            accent: hex(0xebedf0),
            destructive: hex(0xdf2225),
            border: hex(0xe1e3e6),
            input: hex(0xdadce0),
            ring: hex(0x2474cf),
            sidebar: hex(0xf4f5f7),
            sidebar_foreground: hex(0x23272c),
            sidebar_accent: hex(0xe6e8ec),
            sidebar_border: hex(0xe1e3e6),
            rail: hex(0xebedef),
            tooltip: hex(0x181b20),
            tooltip_foreground: hex(0xf4f5f7),
            online: hex(0x28ad5e),
            idle: hex(0xe49921),
            offline: hex(0x9b9fa5),
            link: hex(0x006bbb),
            mention: hex(0xe49e22),
            code: hex(0xf2f3f6),
        }
    }

    /// The foreground colour at an alpha: the design's translucent fills.
    /// 3% message hover, 4.5% field, 6% chip, 9% selected row, 10% hairline.
    pub fn fg(&self, alpha: f32) -> Hsla {
        Hsla {
            a: alpha,
            ..self.foreground
        }
    }

    /// The accent at an alpha: 10% icon chips, 12% own reaction, 16% mention pill.
    pub fn tint(&self, alpha: f32) -> Hsla {
        Hsla {
            a: alpha,
            ..self.primary
        }
    }

    pub fn with_alpha(color: Hsla, alpha: f32) -> Hsla {
        Hsla { a: alpha, ..color }
    }

    fn shadow_colors(&self) -> (Hsla, Hsla) {
        if self.dark {
            (hsla(0., 0., 0., 0.5), hsla(0., 0., 0., 0.28))
        } else {
            (hsla(216. / 360., 0.24, 0.19, 0.16), hsla(216. / 360., 0.24, 0.19, 0.08))
        }
    }

    fn highlight(&self) -> Option<BoxShadow> {
        self.dark.then(|| BoxShadow {
            color: hsla(0., 0., 1., 0.045),
            offset: point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
            inset: true,
        })
    }

    fn layered(&self, near: (f32, f32, f32), far: (f32, f32, f32)) -> Vec<BoxShadow> {
        let (deep, soft) = self.shadow_colors();
        let mut shadows = vec![
            BoxShadow {
                color: soft,
                offset: point(px(0.), px(near.0)),
                blur_radius: px(near.1),
                spread_radius: px(near.2),
                inset: false,
            },
            BoxShadow {
                color: deep,
                offset: point(px(0.), px(far.0)),
                blur_radius: px(far.1),
                spread_radius: px(far.2),
                inset: false,
            },
        ];
        shadows.extend(self.highlight());
        shadows
    }

    /// Composer, hover toolbar, small floating controls.
    pub fn shadow_raised(&self) -> Vec<BoxShadow> {
        self.layered((1., 2., 0.), (6., 16., -8.))
    }

    /// Menus, popovers, tooltips, the jump-to-latest pill.
    pub fn shadow_popover(&self) -> Vec<BoxShadow> {
        self.layered((2., 6., -2.), (16., 40., -12.))
    }

    /// Dialogs and sheets.
    pub fn shadow_dialog(&self) -> Vec<BoxShadow> {
        self.layered((8., 20., -8.), (32., 72., -20.))
    }

    /// The floating main panel: barely there, the hairline does the work.
    pub fn shadow_panel(&self) -> Vec<BoxShadow> {
        let (_, soft) = self.shadow_colors();
        vec![BoxShadow {
            color: soft,
            offset: point(px(0.), px(1.)),
            blur_radius: px(3.),
            spread_radius: px(0.),
            inset: false,
        }]
    }
}

impl Global for Tokens {}

pub fn tokens(cx: &App) -> &Tokens {
    cx.global::<Tokens>()
}

/// Installs the tokens and makes the component library's own widgets (text fields,
/// menus, tooltips, scrollbars) use the same colours, font and radii.
/// Set for test runs: appearance changes then last for the run only and are never
/// saved, so a test cannot overwrite what the owner chose.
static VOLATILE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Stops this run from saving appearance choices.
pub fn set_volatile() {
    VOLATILE.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn remember(store: &std::sync::Arc<crate::store::Store>, key: &str, value: &str) {
    if !VOLATILE.load(std::sync::atomic::Ordering::Relaxed) {
        let _ = store.set_kv(key, value);
    }
}

pub fn install(dark: bool, cx: &mut App) {
    let mut t = Tokens::of(palette(cx), dark);
    t.backdrop = backdrop(cx);
    let t = t.styled(style(cx));
    // Remember the mode for the next start.
    if let Some(store) = cx.try_global::<Appearance>().and_then(|a| a.store.clone()) {
        remember(&store, "theme", if dark { "dark" } else { "light" });
    }
    gpui_component::Theme::change(
        if dark {
            gpui_component::ThemeMode::Dark
        } else {
            gpui_component::ThemeMode::Light
        },
        None,
        cx,
    );
    {
        let theme = gpui_component::Theme::global_mut(cx);
        theme.font_family = FONT_UI.into();
        theme.font_size = px(14.);
        theme.radius = R_LG;
        theme.radius_lg = R_XL;
        theme.background = t.background;
        // The component library's root view paints this one behind everything.
        theme.tokens.background = t.background.into();
        theme.foreground = t.foreground;
        theme.border = t.border;
        theme.input = t.input;
        theme.ring = t.ring;
        theme.caret = t.foreground;
        theme.primary = t.primary;
        theme.primary_hover = Tokens::with_alpha(t.primary, 0.9);
        theme.primary_active = Tokens::with_alpha(t.primary, 0.85);
        theme.primary_foreground = t.primary_foreground;
        theme.secondary = t.secondary;
        theme.secondary_foreground = t.foreground;
        theme.muted = t.muted;
        theme.muted_foreground = t.muted_foreground;
        theme.accent = t.accent;
        theme.accent_foreground = t.foreground;
        theme.popover = t.popover;
        theme.popover_foreground = t.foreground;
        theme.danger = t.destructive;
        theme.link = t.link;
        theme.selection = t.tint(0.35);
        theme.sidebar = t.sidebar;
        theme.sidebar_foreground = t.sidebar_foreground;
        theme.sidebar_accent = t.sidebar_accent;
        theme.sidebar_border = t.sidebar_border;
        theme.title_bar = t.rail;
        theme.title_bar_border = t.rail;
        theme.list_hover = t.accent;
        theme.list_active = t.accent;
        theme.scrollbar = Tokens::with_alpha(t.background, 0.);
        theme.scrollbar_thumb = t.fg(0.16);
        theme.scrollbar_thumb_hover = t.fg(0.28);
    }
    cx.set_global(t);
}

/// The colour palettes of the design. Neutral is the default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Palette {
    Neutral,
    Turquoise,
    Midnight,
    Mono,
    Amber,
}

impl Palette {
    pub const ALL: [Palette; 5] = [
        Palette::Neutral,
        Palette::Turquoise,
        Palette::Midnight,
        Palette::Mono,
        Palette::Amber,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Palette::Neutral => "Neutral",
            Palette::Turquoise => "Turquoise",
            Palette::Midnight => "Midnight",
            Palette::Mono => "Mono",
            Palette::Amber => "Amber",
        }
    }

    pub fn from_name(name: &str) -> Palette {
        Palette::ALL
            .into_iter()
            .find(|palette| palette.name().eq_ignore_ascii_case(name))
            .unwrap_or(Palette::Neutral)
    }

    /// The palette's colours, in the order: background, foreground, card, popover, primary,
    /// primary-foreground, secondary, muted, muted-foreground, accent, destructive, ring,
    /// sidebar, sidebar-foreground, sidebar-accent, rail, tooltip, link, mention, code.
    fn values(self, dark: bool) -> Option<[u32; 20]> {
        Some(match (self, dark) {
            (Palette::Neutral, _) => return None,
            (Palette::Turquoise, true) => [
                0x14171b, 0xeef0f3, 0x1b1e23, 0x1b1e23, 0x42d8c8, 0x001e1e, 0x23272c, 0x1d2125, 0x999fa7, 0x25292f,
                0xf04c55, 0x42d8c8, 0x0d1013, 0xd5d8db, 0x1d2125, 0x0d1013, 0x2c3136, 0x58d1e5, 0xefb146, 0x0f1215,
            ],
            (Palette::Turquoise, false) => [
                0xffffff, 0x12161c, 0xffffff, 0xffffff, 0x007f80, 0xf9fdfc, 0xeef0f3, 0xf1f3f5, 0x646971, 0xebeef2,
                0xdb2b33, 0x009a9b, 0xeff1f4, 0x292e34, 0xe4e7eb, 0xeff1f4, 0x171b21, 0x00769c, 0xe19b1b, 0xf2f4f6,
            ],
            (Palette::Midnight, true) => [
                0x010101, 0xeeeef0, 0x020203, 0x030305, 0x8f9eef, 0x0d1023, 0x06070a, 0x060609, 0x8d8f94, 0x08090c,
                0xed4a49, 0x8f9eef, 0x000000, 0xdddee0, 0x040507, 0x000000, 0x111316, 0x92b9f8, 0xefb146, 0x000000,
            ],
            (Palette::Midnight, false) => [
                0xfbfcfd, 0x17181c, 0xffffff, 0xffffff, 0x4b53c2, 0xfcfcfc, 0xeff0f2, 0xf1f2f3, 0x6a6c70, 0xecedef,
                0xdf2225, 0x4b53c2, 0xf4f5f6, 0x25262a, 0xe7e8ea, 0xecedee, 0x191a1e, 0x355bbd, 0xe49e22, 0xf3f3f5,
            ],
            (Palette::Mono, true) => [
                0x080808, 0xeeeeee, 0x0e0e0e, 0x101010, 0xebebeb, 0x0d0d0d, 0x171717, 0x161616, 0x8f8f8f, 0x191919,
                0xed4a49, 0xebebeb, 0x050505, 0xdedede, 0x141414, 0x030303, 0x242424, 0xa4c9e9, 0xefb146, 0x050505,
            ],
            (Palette::Mono, false) => [
                0xfcfcfc, 0x181818, 0xffffff, 0xffffff, 0x1b1b1b, 0xfcfcfc, 0xf0f0f0, 0xf2f2f2, 0x6c6c6c, 0xededed,
                0xdf2225, 0x1b1b1b, 0xf5f5f5, 0x262626, 0xe8e8e8, 0xededed, 0x1b1b1b, 0x23588a, 0xe49e22, 0xf3f3f3,
            ],
            (Palette::Amber, true) => [
                0x0b0805, 0xf0eeec, 0x120d09, 0x14100b, 0xf9b13e, 0x301904, 0x1b1611, 0x1a1510, 0x978d83, 0x1e1812,
                0xed4a49, 0xf9b13e, 0x070503, 0xe1ddd9, 0x18130e, 0x040301, 0x29231d, 0xf4c26a, 0xefb146, 0x070503,
            ],
            (Palette::Amber, false) => [
                0xfdfbfa, 0x1d1711, 0xffffff, 0xffffff, 0xf09c17, 0x311805, 0xf2f0ec, 0xf4f1ee, 0x726a61, 0xf0ece8,
                0xdf2225, 0xf09c17, 0xf7f5f2, 0x2b251e, 0xebe7e2, 0xefece9, 0x1f1913, 0x9d5300, 0xe49e22, 0xf6f3f0,
            ],
        })
    }
}

impl Tokens {
    /// The tokens of a palette in dark or light mode.
    pub fn of(palette: Palette, dark: bool) -> Tokens {
        let mut t = if dark { Tokens::dark() } else { Tokens::light() };
        let Some(v) = palette.values(dark) else {
            return t;
        };
        t.background = hex(v[0]);
        t.foreground = hex(v[1]);
        t.card = hex(v[2]);
        t.popover = hex(v[3]);
        t.primary = hex(v[4]);
        t.primary_foreground = hex(v[5]);
        t.secondary = hex(v[6]);
        t.muted = hex(v[7]);
        t.muted_foreground = hex(v[8]);
        t.accent = hex(v[9]);
        t.destructive = hex(v[10]);
        t.ring = hex(v[11]);
        t.sidebar = hex(v[12]);
        t.sidebar_foreground = hex(v[13]);
        t.sidebar_accent = hex(v[14]);
        t.rail = hex(v[15]);
        t.tooltip = hex(v[16]);
        t.link = hex(v[17]);
        t.mention = hex(v[18]);
        t.code = hex(v[19]);
        t
    }
}

/// The palette in use, and where the appearance choice is remembered.
struct Appearance {
    palette: Palette,
    store: Option<std::sync::Arc<crate::store::Store>>,
}

impl Global for Appearance {}

/// Loads the remembered appearance and applies it. `force_dark` overrides the stored mode.
pub fn restore(store: std::sync::Arc<crate::store::Store>, force_dark: Option<bool>, cx: &mut App) {
    let palette = store
        .kv("palette")
        .ok()
        .flatten()
        .map(|name| Palette::from_name(&name))
        .unwrap_or(Palette::Neutral);
    let dark = force_dark.unwrap_or_else(|| store.kv("theme").ok().flatten().as_deref() != Some("light"));
    cx.set_global(Appearance {
        palette,
        store: Some(store),
    });
    install(dark, cx);
}

pub fn palette(cx: &App) -> Palette {
    cx.try_global::<Appearance>()
        .map(|appearance| appearance.palette)
        .unwrap_or(Palette::Neutral)
}

/// Switches palette, keeping the current dark or light mode.
pub fn set_palette(palette: Palette, cx: &mut App) {
    let dark = cx.try_global::<Tokens>().map(|t| t.dark).unwrap_or(true);
    let store = cx.try_global::<Appearance>().and_then(|a| a.store.clone());
    if let Some(store) = &store {
        remember(store, "palette", palette.name());
    }
    cx.set_global(Appearance { palette, store });
    install(dark, cx);
}

impl Tokens {
    /// Text colour of a mention pill: the accent mixed 70 / 30 with the foreground.
    pub fn mention_text(&self) -> Hsla {
        let accent = gpui::Rgba::from(self.primary);
        let text = gpui::Rgba::from(self.foreground);
        let mix = |a: f32, b: f32| a * 0.7 + b * 0.3;
        gpui::Rgba {
            r: mix(accent.r, text.r),
            g: mix(accent.g, text.g),
            b: mix(accent.b, text.b),
            a: 1.,
        }
        .into()
    }
}

/// How surfaces are treated, on top of the palette and the dark / light mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Style {
    /// Solid, layered surfaces: the design as specified.
    #[default]
    Modern,
    /// Translucent surfaces over a blurred view of what is behind the window, with light
    /// edges that read as the rim of a pane of glass.
    Glass,
    /// Real glass, drawn by the renderer: floating surfaces show a blurred copy of what
    /// passes beneath them, bent at the rim like a thick pane, with a lit edge.
    Liquid,
    /// Deeper blacks, accent-coloured hairlines and a soft glow on what is active.
    Futuristic,
}

impl Style {
    pub const ALL: [Style; 4] = [Style::Modern, Style::Glass, Style::Liquid, Style::Futuristic];

    pub fn name(self) -> &'static str {
        match self {
            Style::Modern => "Modern",
            Style::Glass => "Glass",
            Style::Liquid => "Liquid Glass",
            Style::Futuristic => "Futuristic",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Style::Modern => "Solid, layered surfaces",
            Style::Glass => "Translucent panes over a blurred backdrop",
            Style::Liquid => "Real glass that blurs and bends what passes beneath",
            Style::Futuristic => "Deep blacks, accent lines and glow",
        }
    }

    pub fn from_name(name: &str) -> Style {
        Style::ALL
            .into_iter()
            .find(|style| {
                let full = style.name();
                let first = full.split(' ').next().unwrap_or(full);
                full.eq_ignore_ascii_case(name) || first.eq_ignore_ascii_case(name)
            })
            .unwrap_or_default()
    }
}

impl Tokens {
    /// Applies a style's surface treatment to the palette's colours.
    pub fn styled(mut self, style: Style) -> Tokens {
        self.style = style;
        let alpha = |color: Hsla, a: f32| Hsla { a, ..color };
        match style {
            Style::Modern => {}
            Style::Glass => {
                // Chrome is the most see-through, the reading surface the least.
                self.rail = alpha(self.rail, 0.34);
                self.sidebar = alpha(self.sidebar, 0.26);
                self.background = alpha(self.background, if self.dark { 0.62 } else { 0.70 });
                self.card = alpha(self.card, if self.dark { 0.66 } else { 0.74 });
                // Menus stay nearly solid: text over text must remain readable.
                self.popover = alpha(self.popover, 0.93);
                self.tooltip = alpha(self.tooltip, 0.95);
                self.sidebar_accent = self.fg(0.09);
                self.secondary = self.fg(0.08);
                self.muted = self.fg(0.08);
                self.accent = self.fg(0.11);
                self.border = self.fg(0.14);
                self.sidebar_border = self.fg(0.12);
                self.input = self.fg(0.18);
            }
            Style::Liquid => {
                // The window keeps one opaque base colour (the rail token); every
                // other surface is glass or a faint fill, so the ambient colours and
                // the content beneath show through.
                let base = if self.dark { hex(0x05060b) } else { hex(0xe6ebf3) };
                self.rail = match self.backdrop {
                    Backdrop::Solid => base,
                    // The window is see-through. A thin veil is left so text that sits
                    // on no glass (the title bar) stays readable over a bright desktop.
                    Backdrop::Frosted => alpha(base, if self.dark { 0.30 } else { 0.26 }),
                    Backdrop::Clear => alpha(base, if self.dark { 0.16 } else { 0.12 }),
                };
                self.sidebar = alpha(self.sidebar, 0.);
                self.background = alpha(self.background, 0.);
                self.card = self.fg(if self.dark { 0.07 } else { 0.05 });
                self.popover = alpha(self.popover, 0.90);
                self.tooltip = alpha(self.tooltip, 0.95);
                self.sidebar_accent = self.fg(0.10);
                self.secondary = self.fg(0.08);
                self.muted = self.fg(0.08);
                self.accent = self.fg(0.12);
                self.border = self.fg(0.12);
                self.sidebar_border = self.fg(0.10);
                self.input = self.fg(0.18);
            }
            Style::Futuristic => {
                if self.dark {
                    self.background = hex(0x04050a);
                    self.sidebar = hex(0x020308);
                    self.rail = hex(0x010205);
                    self.card = hex(0x080a12);
                    self.popover = hex(0x0a0d16);
                    self.sidebar_accent = hex(0x0b0e18);
                    self.accent = hex(0x10131e);
                }
                self.border = self.tint(0.20);
                self.sidebar_border = self.tint(0.16);
                self.input = self.tint(0.30);
            }
        }
        self
    }

    /// The fill of a message bubble: ours or someone else's.
    pub fn bubble(&self, mine: bool) -> Hsla {
        match (self.style, mine) {
            (Style::Modern, true) => self.tint(0.16),
            (Style::Modern, false) => self.fg(0.06),
            (Style::Glass, true) => self.tint(0.36),
            (Style::Glass, false) => self.fg(0.12),
            (Style::Liquid, true) => self.tint(0.42),
            (Style::Liquid, false) => self.fg(0.13),
            (Style::Futuristic, true) => self.tint(0.18),
            (Style::Futuristic, false) => self.fg(0.045),
        }
    }

    /// The edge of a message bubble.
    pub fn bubble_border(&self, mine: bool) -> Hsla {
        match (self.style, mine) {
            (Style::Modern, _) => gpui::transparent_black(),
            // A light rim, like the edge of glass catching light.
            (Style::Glass, _) => self.fg(0.16),
            (Style::Liquid, _) => self.fg(0.14),
            (Style::Futuristic, true) => self.tint(0.60),
            (Style::Futuristic, false) => self.tint(0.18),
        }
    }

    /// The hairline round the floating main panel.
    pub fn panel_border(&self) -> Hsla {
        match self.style {
            Style::Modern => self.fg(0.10),
            Style::Glass => self.fg(0.20),
            // The glass draws its own lit rim.
            Style::Liquid => gpui::transparent_black(),
            Style::Futuristic => self.tint(0.38),
        }
    }

    /// The shadow of the floating main panel.
    pub fn panel_shadow(&self) -> Vec<BoxShadow> {
        let mut shadows = self.shadow_panel();
        match self.style {
            Style::Modern => {}
            Style::Glass => {
                // Depth below, and a bright inner top edge.
                shadows.push(BoxShadow {
                    color: hsla(0., 0., 0., if self.dark { 0.35 } else { 0.12 }),
                    offset: point(px(0.), px(12.)),
                    blur_radius: px(32.),
                    spread_radius: px(-8.),
                    inset: false,
                });
                shadows.push(BoxShadow {
                    color: hsla(0., 0., 1., if self.dark { 0.10 } else { 0.55 }),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(0.),
                    spread_radius: px(0.),
                    inset: true,
                });
            }
            Style::Liquid => {
                shadows.clear();
            }
            Style::Futuristic => shadows.push(BoxShadow {
                color: self.tint(0.22),
                offset: point(px(0.), px(0.)),
                blur_radius: px(28.),
                spread_radius: px(-6.),
                inset: false,
            }),
        }
        shadows
    }

    /// A glow for what is active (selected rail tile, send button). Empty outside the
    /// futuristic style.
    pub fn glow(&self) -> Vec<BoxShadow> {
        match self.style {
            Style::Futuristic => vec![BoxShadow {
                color: self.tint(0.55),
                offset: point(px(0.), px(0.)),
                blur_radius: px(14.),
                spread_radius: px(0.),
                inset: false,
            }],
            _ => Vec::new(),
        }
    }

    /// What the window shows behind its own surfaces.
    pub fn window_backdrop(&self) -> gpui::WindowBackgroundAppearance {
        match (self.style, self.backdrop) {
            (Style::Glass, _) => gpui::WindowBackgroundAppearance::Blurred,
            (Style::Liquid, Backdrop::Clear) => gpui::WindowBackgroundAppearance::Transparent,
            (Style::Liquid, Backdrop::Frosted) => gpui::WindowBackgroundAppearance::Blurred,
            _ => gpui::WindowBackgroundAppearance::Opaque,
        }
    }
}

struct StyleChoice(Style);

impl Global for StyleChoice {}

pub fn style(cx: &App) -> Style {
    cx.try_global::<StyleChoice>()
        .map(|choice| choice.0)
        .unwrap_or_default()
}

/// Applies the style remembered from the last run. Call after [`restore`].
pub fn restore_style(cx: &mut App) {
    let remembered = cx
        .try_global::<Appearance>()
        .and_then(|appearance| appearance.store.clone())
        .and_then(|store| store.kv("style").ok().flatten())
        .map(|name| Style::from_name(&name))
        .unwrap_or_default();
    cx.set_global(StyleChoice(remembered));
    let backdrop = cx
        .try_global::<Appearance>()
        .and_then(|appearance| appearance.store.clone())
        .and_then(|store| store.kv("backdrop").ok().flatten())
        .map(|name| Backdrop::from_name(&name))
        .unwrap_or_default();
    cx.set_global(BackdropChoice(backdrop));
    let dark = cx.try_global::<Tokens>().map(|t| t.dark).unwrap_or(true);
    install(dark, cx);
}

/// Switches style, keeping the palette and the dark or light mode.
pub fn set_style(style: Style, cx: &mut App) {
    if let Some(store) = cx.try_global::<Appearance>().and_then(|a| a.store.clone()) {
        remember(&store, "style", style.name());
    }
    cx.set_global(StyleChoice(style));
    let dark = cx.try_global::<Tokens>().map(|t| t.dark).unwrap_or(true);
    install(dark, cx);
}

/// Which kind of surface a glass fill is for; each bends and blurs differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// A large pane holding content: the chat list, the conversation panel.
    Panel,
    /// A bar floating over scrolling content: the chat header, the message box, the rail.
    Bar,
    /// A menu, picker or dialog: text over text, so it is the most frosted.
    Menu,
    /// A small floating control: a pill or a round button.
    Chip,
}

impl Tokens {
    /// Whether surfaces are drawn as real glass (blurred and refracted by the renderer).
    pub fn liquid(&self) -> bool {
        self.style == Style::Liquid
    }

    /// The fill of a surface. In the Liquid Glass style this is real glass: the surface
    /// shows a blurred copy of what lies beneath, bent at its rim. In the other styles
    /// it is the plain colour given as `solid`.
    pub fn surface(&self, kind: Surface, solid: Hsla) -> gpui::Background {
        if !self.liquid() {
            return solid.into();
        }
        // Dark glass is smoked, light glass is milky.
        let base = if self.dark {
            hsla(225. / 360., 0.30, 0.07, 1.)
        } else {
            hsla(220. / 360., 0.35, 0.98, 1.)
        };
        // Over a see-through window the glass carries more of its own colour, or text
        // would be lost against whatever is on the desktop.
        // A clear window shows whatever is on the desktop, sharp, so surfaces that hold
        // text must be dense enough to read over anything; the bars stay lighter.
        let extra = match (self.backdrop, kind) {
            (Backdrop::Solid, _) => 0.,
            (Backdrop::Frosted, _) => 0.09,
            (Backdrop::Clear, Surface::Panel) => 0.30,
            (Backdrop::Clear, Surface::Menu) => 0.24,
            (Backdrop::Clear, Surface::Bar | Surface::Chip) => 0.28,
        };
        let tint = |a: f32| Hsla {
            a: (a + extra).min(0.92),
            ..base
        };
        let rim = hsla(0., 0., 1., if self.dark { 0.55 } else { 0.95 });
        let spec = match kind {
            Surface::Panel => gpui::Glass {
                tint: tint(if self.dark { 0.52 } else { 0.50 }),
                blur: 36.,
                refraction: 26.,
                bevel: 20.,
                highlight: Hsla { a: rim.a * 0.75, ..rim },
                dispersion: 0.10,
                saturation: 1.5,
            },
            Surface::Bar => gpui::Glass {
                tint: tint(if self.dark { 0.34 } else { 0.38 }),
                blur: 4.5,
                refraction: 38.,
                bevel: 17.,
                highlight: rim,
                dispersion: 0.22,
                saturation: 1.6,
            },
            Surface::Menu => gpui::Glass {
                tint: tint(if self.dark { 0.66 } else { 0.70 }),
                blur: 26.,
                refraction: 18.,
                bevel: 12.,
                highlight: Hsla { a: rim.a * 0.8, ..rim },
                dispersion: 0.10,
                saturation: 1.5,
            },
            Surface::Chip => gpui::Glass {
                tint: tint(if self.dark { 0.30 } else { 0.36 }),
                blur: 4.,
                refraction: 16.,
                bevel: 10.,
                highlight: rim,
                dispersion: 0.14,
                saturation: 1.6,
            },
        };
        gpui::glass(spec)
    }

    /// Fill of menus, pickers and dialogs.
    pub fn popover_fill(&self) -> gpui::Background {
        self.surface(Surface::Menu, self.popover)
    }

    /// The soft colours behind everything in the Liquid Glass style, as (x, y, size,
    /// colour) with positions as fractions of the window. Glass needs something to bend.
    pub fn ambient(&self) -> Vec<(f32, f32, f32, Hsla)> {
        // The soft colours stand in for a backdrop; a see-through window has the desktop.
        if !self.liquid() || self.backdrop != Backdrop::Solid {
            return Vec::new();
        }
        let hue = self.primary.h;
        let turn = |by: f32| (hue + by).rem_euclid(1.);
        let (light, alpha) = if self.dark { (0.45, 0.60) } else { (0.72, 0.62) };
        // Neighbouring hues of the accent, so the backdrop reads as one mood.
        vec![
            (0.08, 0.10, 520., hsla(turn(0.), 0.80, light, alpha)),
            (0.92, 0.16, 460., hsla(turn(0.11), 0.75, light, alpha * 0.85)),
            (0.62, 0.98, 560., hsla(turn(-0.09), 0.80, light, alpha * 0.8)),
            (0.30, 0.74, 360., hsla(turn(0.20), 0.70, light, alpha * 0.5)),
        ]
    }
}

/// What shows behind the window in the Liquid Glass style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Backdrop {
    /// The desktop and other windows show through, sharp.
    #[default]
    Clear,
    /// The desktop shows through, blurred by Windows.
    Frosted,
    /// Nothing shows through: soft colours of the app's own are painted instead.
    Solid,
}

impl Backdrop {
    pub const ALL: [Backdrop; 3] = [Backdrop::Clear, Backdrop::Frosted, Backdrop::Solid];

    pub fn name(self) -> &'static str {
        match self {
            Backdrop::Clear => "Clear",
            Backdrop::Frosted => "Frosted",
            Backdrop::Solid => "Solid",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Backdrop::Clear => "See the desktop through the window",
            Backdrop::Frosted => "See the desktop, blurred",
            Backdrop::Solid => "Not see-through",
        }
    }

    pub fn from_name(name: &str) -> Backdrop {
        Backdrop::ALL
            .into_iter()
            .find(|backdrop| backdrop.name().eq_ignore_ascii_case(name))
            .unwrap_or_default()
    }
}

struct BackdropChoice(Backdrop);

impl Global for BackdropChoice {}

pub fn backdrop(cx: &App) -> Backdrop {
    cx.try_global::<BackdropChoice>()
        .map(|choice| choice.0)
        .unwrap_or_default()
}

/// Switches what is behind the window, keeping everything else.
pub fn set_backdrop(backdrop: Backdrop, cx: &mut App) {
    if let Some(store) = cx.try_global::<Appearance>().and_then(|a| a.store.clone()) {
        remember(&store, "backdrop", backdrop.name());
    }
    cx.set_global(BackdropChoice(backdrop));
    let dark = cx.try_global::<Tokens>().map(|t| t.dark).unwrap_or(true);
    install(dark, cx);
}
