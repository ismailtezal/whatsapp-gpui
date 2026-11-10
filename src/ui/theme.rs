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
pub fn install(dark: bool, cx: &mut App) {
    let t = if dark { Tokens::dark() } else { Tokens::light() };
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
