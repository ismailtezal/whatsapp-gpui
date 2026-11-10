# Design spec: diskurt "modern" design, for a native (GPUI) re-implementation

Extracted from the diskurt source at `C:\Projects\diskurt` (read-only reference; commit `60cb73c`, branch as checked out on 2026-11-11). Diskurt is a Next.js 16 + Tailwind CSS v4 + shadcn/Radix app. This document turns its CSS into fixed numbers so the look can be rebuilt in a toolkit without CSS.

Conventions used here:

- **px** values assume the 16px root font size and the `default` density (Tailwind spacing unit = 4px). `h-8` = 32px, `px-3` = 12px, and so on.
- **Hex** values are sRGB, converted from the source `oklch()` values with the same OKLab matrix diskurt's own generator uses (`scripts/build-themes.mjs`); they match the hex swatches the repo generates (`lib/appearance-swatches.ts`). Out-of-gamut colours are clamped per channel (a browser's gamut mapping can differ by a few units on the most saturated accents).
- "X @ N% over Y" means colour X at N% alpha, alpha-composited in sRGB over the opaque colour Y. Use the alpha form where your toolkit can draw translucent fills, and the composited hex where it cannot.
- **(inferred)** marks a value that was derived by reasoning (CSS specificity, Tailwind defaults, arithmetic) and not read literally from a source line.
- Source references are given as `path:line` relative to `C:\Projects\diskurt`.

Contents:

1. [How appearance is structured](#1-how-appearance-is-structured)
2. [Colour tokens](#2-colour-tokens)
3. [What `data-style="modern"` changes](#3-what-data-stylemodern-changes)
4. [`data-layout="inset"`: the floating main panel](#4-data-layoutinset-the-floating-main-panel)
5. [Radius scale (`data-radius="lg"`)](#5-radius-scale-data-radiuslg)
6. [Density (`default`)](#6-density-default)
7. [Typography](#7-typography)
8. [Icons](#8-icons)
9. [Motion](#9-motion)
10. [Chat-surface anatomy](#10-chat-surface-anatomy)
11. [Window chrome (desktop app)](#11-window-chrome-desktop-app)
12. [Which look the owner actually uses](#12-which-look-the-owner-actually-uses)
13. [Essence: what makes it recognisable](#13-essence-what-makes-it-recognisable)
14. [Not determined / caveats](#14-not-determined--caveats)

---

## 1. How appearance is structured

Source: `lib/appearance.ts`, `app/globals.css:1-137`, `app/layout.tsx`, `lib/prefs.ts`.

Appearance is a set of data attributes on `<html>`:

| Attribute | Values | Default | Modern preset |
| --- | --- | --- | --- |
| `data-theme` (mode) | `dark`, `light`, `system` | `dark` | (independent of the preset) |
| `data-palette` (colours) | `neutral`, `turquoise`, `midnight`, `ocean`, `violet`, `rose`, `pomegranate`, `amber`, `forest`, `mono` | `neutral` | (independent of the preset) |
| `data-style` | `classic`, `modern` | `classic` | `modern` |
| `data-font` | `geist`, `figtree`, `inter`, `jakarta`, `nunito`, `plex`, `grotesk`, `lexend`, `system` | `geist` | `figtree` |
| `data-radius` | `none`, `sm`, `md`, `lg`, `xl`, `2xl` | `md` | `lg` |
| `data-density` | `compact`, `default`, `comfortable` | `default` | `default` |
| `data-layout` | `flat`, `inset` | `flat` | `inset` |
| `data-icons` | `thin`, `medium`, `regular` | `regular` | `medium` |

The modern preset is exactly `{ uiStyle: 'modern', uiFont: 'figtree', uiRadius: 'lg', uiDensity: 'default', uiLayout: 'inset', uiIcons: 'medium' }` (`lib/appearance.ts:56-59`). Its in-app description is "Filled fields, softer depth, floating panel"; the style hint reads "Modern uses filled fields, pill reactions and softer shadows."

In the Tailwind source three custom variants express the design: `modern:` (applies under `[data-style='modern']`), `inset:` and `flat:` (`app/globals.css:25-27`). They use `:is()`, so a `modern:` class always beats the plain class it refines. Everything in section 3 comes from reading every `modern:` class in the codebase.

There is a third, optional layer, gradient themes (`data-gradient`, `lib/appearance-gradients.ts`), which paints a gradient behind translucent panels. It is not part of the modern preset and is not specified here.

Other per-user settings that affect the chat surface: `fontSize` (chat text, 12 to 20px, default 14), `messageDisplay` (`cozy` default, or `compact`), `use24HourClock` (default true), `showSendButton`.

---

## 2. Colour tokens

Source: `app/themes.css` (generated), `scripts/build-themes.mjs` (the definitions), `app/globals.css:33-53` (mode tokens).

### 2.1 Token roles

| Token | Role |
| --- | --- |
| `background` / `foreground` | The main panel (chat) surface and its text. Dialogs also use `background`. |
| `card` / `card-foreground` | Raised in-flow surfaces: the composer, embed and file cards, pinned-message cards. |
| `popover` / `popover-foreground` | Floating layers: menus, popovers, the message hover toolbar, toasts. |
| `primary` / `primary-foreground` | The accent: primary buttons, selection, selected rail tile ring, own reactions, focus. |
| `secondary` / `secondary-foreground` | Quiet filled buttons (light mode; dark modern uses foreground alpha instead). |
| `muted` / `muted-foreground` | Quiet fills; `muted-foreground` is the secondary text colour everywhere (timestamps, placeholders, icons at rest). |
| `accent` / `accent-foreground` | Hover/focus fill inside menus and for ghost buttons in light mode. Not a brand colour. |
| `destructive` | Danger text and buttons, unread-count badges, the "new messages" line. |
| `border` | 1px lines and separators. In dark mode it is white at 7.5% alpha, so it adapts to any surface. |
| `input` | Field borders (classic) and outline-button borders. White at 11% in dark mode. |
| `ring` | Focus colour (equals `primary` except in turquoise light). |
| `sidebar`, `sidebar-foreground`, `sidebar-primary`, `sidebar-primary-foreground`, `sidebar-accent`, `sidebar-accent-foreground`, `sidebar-border`, `sidebar-ring` | The channel/conversation column, and, in the inset layout, the colour behind the floating panel. |
| `rail` | The narrow server rail; also the desktop title bar colour. The darkest surface. |
| `tooltip` / `tooltip-foreground` | Tooltip surface (modern style only; classic inverts foreground/background). |
| `online`, `idle`, `dnd`, `offline` | Presence dots. `online` doubles as "success" (join arrows, copied check), `idle` as "warning" (owner crown, offline banner icon). There are no separate success/warning tokens. |
| `link` | Link text in messages and inline text actions. |
| `mention` | Amber used for "this message mentions you" (row tint at 10% plus a 2px left bar). |
| `code` | Inline code and code block background. |

Chat-specific colours that are *not* tokens but recipes (see 2.4): message hover, selected sidebar row, mention pills, own-reaction pill, replying-to row, jump flash. **There is no "own message" colour**: diskurt is Discord-style, every message is a flat row with avatar and name; your own messages look the same as anyone's.

Elevation order in dark mode, darkest to lightest: `rail` < `sidebar` < `background` < `card` < `popover` < `tooltip`. Each step is a small lightness increase (OKLCH L 0.095, 0.115, 0.135, 0.165, 0.175, 0.26 in neutral). Depth comes from these steps plus soft shadows, not from visible borders.

### 2.2 Mode tokens (same for every palette)

`app/globals.css:33-53`.

| Token | Light | Dark |
| --- | --- | --- |
| `--shadow-color` | `oklch(0.3 0.03 260 / 0.16)` = `#252e3d` @ 16% = `rgba(37,46,61,0.16)` | `oklch(0 0 0 / 0.5)` = `rgba(0,0,0,0.5)` |
| `--shadow-color-soft` | `oklch(0.3 0.03 260 / 0.08)` = `rgba(37,46,61,0.08)` | `oklch(0 0 0 / 0.28)` = `rgba(0,0,0,0.28)` |
| `--highlight` (1px inner top edge on floating layers) | `oklch(1 0 0 / 0)` = none | `oklch(1 0 0 / 0.045)` = `rgba(255,255,255,0.045)` |
| `--chat-font-size` | 14px | 14px |
| `color-scheme` | light | dark |

### 2.3 Neutral palette (the default: graphite with a blue accent)

#### Neutral — dark

| Token | Source (oklch) | Hex sRGB | Notes |
| --- | --- | --- | --- |
| `--background` | `oklch(0.135 0.008 262)` | `#07080b` |  |
| `--foreground` | `oklch(0.95 0.003 262)` | `#edeef0` |  |
| `--card` | `oklch(0.165 0.009 262)` | `#0c0e12` |  |
| `--card-foreground` | `oklch(0.95 0.003 262)` | `#edeef0` |  |
| `--popover` | `oklch(0.175 0.009 262)` | `#0e1115` |  |
| `--popover-foreground` | `oklch(0.95 0.003 262)` | `#edeef0` |  |
| `--primary` | `oklch(0.65 0.15 252)` | `#4092e7` |  |
| `--primary-foreground` | `oklch(0.99 0 0)` | `#fcfcfc` |  |
| `--secondary` | `oklch(0.205 0.01 262)` | `#15171c` |  |
| `--secondary-foreground` | `oklch(0.92 0.004 262)` | `#e3e5e7` |  |
| `--muted` | `oklch(0.2 0.01 262)` | `#14161b` |  |
| `--muted-foreground` | `oklch(0.65 0.014 262)` | `#8a8f98` |  |
| `--accent` | `oklch(0.215 0.011 262)` | `#171a1f` |  |
| `--accent-foreground` | `oklch(0.96 0.003 262)` | `#f1f2f4` |  |
| `--destructive` | `oklch(0.64 0.2 25)` | `#ed4a49` |  |
| `--border` | `oklch(1 0 0 / 0.075)` | `#ffffff @ 7.5%` | on background = `#191b1e`; on card = `#1e2024`; on popover = `#202226`; on sidebar = `#17181a` |
| `--input` | `oklch(1 0 0 / 0.11)` | `#ffffff @ 11%` | on background = `#222326`; on card = `#27292c`; on popover = `#292b2e`; on sidebar = `#202123` |
| `--ring` | `oklch(0.65 0.15 252)` | `#4092e7` |  |
| `--sidebar` | `oklch(0.115 0.008 262)` | `#040507` |  |
| `--sidebar-foreground` | `oklch(0.9 0.005 262)` | `#dcdee1` |  |
| `--sidebar-primary` | `oklch(0.65 0.15 252)` | `#4092e7` |  |
| `--sidebar-primary-foreground` | `oklch(0.99 0 0)` | `#fcfcfc` |  |
| `--sidebar-accent` | `oklch(0.19 0.01 262)` | `#111418` |  |
| `--sidebar-accent-foreground` | `oklch(0.96 0.003 262)` | `#f1f2f4` |  |
| `--sidebar-border` | `oklch(1 0 0 / 0.06)` | `#ffffff @ 6%` | on sidebar = `#131416` |
| `--sidebar-ring` | `oklch(0.65 0.15 252)` | `#4092e7` |  |
| `--rail` | `oklch(0.095 0.007 262)` | `#020304` |  |
| `--tooltip` | `oklch(0.26 0.011 262)` | `#212429` |  |
| `--tooltip-foreground` | `oklch(0.97 0.003 262)` | `#f4f5f7` |  |
| `--online` | `oklch(0.72 0.16 152)` | `#42c070` |  |
| `--idle` | `oklch(0.8 0.15 75)` | `#f5ae39` |  |
| `--dnd` | `oklch(0.65 0.2 25)` | `#f14d4c` |  |
| `--offline` | `oklch(0.55 0.012 262)` | `#6e7279` |  |
| `--link` | `oklch(0.74 0.12 245)` | `#65b2f1` |  |
| `--mention` | `oklch(0.8 0.14 78)` | `#efb146` |  |
| `--code` | `oklch(0.115 0.008 262)` | `#040507` |  |

#### Neutral — light

| Token | Source (oklch) | Hex sRGB | Notes |
| --- | --- | --- | --- |
| `--background` | `oklch(0.99 0.002 262)` | `#fbfcfd` |  |
| `--foreground` | `oklch(0.21 0.012 262)` | `#15181e` |  |
| `--card` | `oklch(1 0 0)` | `#ffffff` |  |
| `--card-foreground` | `oklch(0.21 0.012 262)` | `#15181e` |  |
| `--popover` | `oklch(1 0 0)` | `#ffffff` |  |
| `--popover-foreground` | `oklch(0.21 0.012 262)` | `#15181e` |  |
| `--primary` | `oklch(0.56 0.16 255)` | `#2474cf` |  |
| `--primary-foreground` | `oklch(0.99 0 0)` | `#fcfcfc` |  |
| `--secondary` | `oklch(0.955 0.004 262)` | `#eff0f3` |  |
| `--secondary-foreground` | `oklch(0.27 0.012 262)` | `#23272c` |  |
| `--muted` | `oklch(0.96 0.004 262)` | `#f0f2f4` |  |
| `--muted-foreground` | `oklch(0.53 0.013 262)` | `#686c74` |  |
| `--accent` | `oklch(0.945 0.005 262)` | `#ebedf0` |  |
| `--accent-foreground` | `oklch(0.21 0.012 262)` | `#15181e` |  |
| `--destructive` | `oklch(0.58 0.22 27)` | `#df2225` |  |
| `--border` | `oklch(0.915 0.005 262)` | `#e1e3e6` |  |
| `--input` | `oklch(0.895 0.006 262)` | `#dadce0` |  |
| `--ring` | `oklch(0.56 0.16 255)` | `#2474cf` |  |
| `--sidebar` | `oklch(0.97 0.003 262)` | `#f4f5f7` |  |
| `--sidebar-foreground` | `oklch(0.27 0.012 262)` | `#23272c` |  |
| `--sidebar-primary` | `oklch(0.56 0.16 255)` | `#2474cf` |  |
| `--sidebar-primary-foreground` | `oklch(0.99 0 0)` | `#fcfcfc` |  |
| `--sidebar-accent` | `oklch(0.93 0.006 262)` | `#e6e8ec` |  |
| `--sidebar-accent-foreground` | `oklch(0.21 0.012 262)` | `#15181e` |  |
| `--sidebar-border` | `oklch(0.915 0.005 262)` | `#e1e3e6` |  |
| `--sidebar-ring` | `oklch(0.56 0.16 255)` | `#2474cf` |  |
| `--rail` | `oklch(0.945 0.004 262)` | `#ebedef` |  |
| `--tooltip` | `oklch(0.22 0.012 262)` | `#181b20` |  |
| `--tooltip-foreground` | `oklch(0.97 0.003 262)` | `#f4f5f7` |  |
| `--online` | `oklch(0.66 0.16 152)` | `#28ad5e` |  |
| `--idle` | `oklch(0.74 0.15 72)` | `#e49921` |  |
| `--dnd` | `oklch(0.6 0.21 25)` | `#e23439` |  |
| `--offline` | `oklch(0.7 0.01 262)` | `#9b9fa5` |  |
| `--link` | `oklch(0.52 0.15 250)` | `#006bbb` |  |
| `--mention` | `oklch(0.75 0.15 75)` | `#e49e22` |  |
| `--code` | `oklch(0.965 0.004 262)` | `#f2f3f6` |  |

### 2.4 Derived colours (modern style recipes resolved against neutral)

These are the translucent fills the modern style uses instead of dedicated tokens. The recipe is the truth; the hex is the recipe composited over the surface it normally sits on.

#### Derived colours — neutral dark

| Name | Recipe | Hex | Used for |
| --- | --- | --- | --- |
| Field fill (rest) | foreground @ 4.5% over background | `#111316` | Input / Textarea / Select / InputGroup at rest |
| Field fill (hover) | foreground @ 6.5% over background | `#16171a` | field hover |
| Field fill (rest) on card | foreground @ 4.5% over card | `#16181c` | fields inside cards |
| Field fill (rest) on popover | foreground @ 4.5% over popover | `#181b1e` | fields inside popovers |
| Message row hover | foreground @ 3% over background | `#0e0f12` | modern:bg-foreground/[0.03] |
| Composer disabled bar | foreground @ 4% over background | `#101115` | no-permission composer |
| Tabs list track | foreground @ 5% over background | `#121417` | TabsList |
| Chip fill 6% | foreground @ 6% over background | `#141619` | reaction pill at rest, skeleton, kbd, member-row hover |
| Chip fill 7% | foreground @ 7% over background | `#17181b` | ghost Button hover, secondary Badge, role chips |
| Fill 8% | foreground @ 8% over background | `#191b1e` | secondary Button rest |
| Fill 10% | foreground @ 10% over background | `#1e1f22` | active tab |
| Fill 12% | foreground @ 12% over background | `#222427` | secondary Button hover, slider track |
| Fill 15% | foreground @ 15% over background | `#292b2e` | Switch off track, hover hairlines (foreground/15) |
| Inset panel border (on panel) | foreground @ 10% over background | `#1e1f22` | border-foreground/10 round the floating main panel |
| Inset panel border (on sidebar) | foreground @ 10% over sidebar | `#1b1c1f` | same hairline seen against the sidebar colour |
| Composer border | foreground @ 8% over card | `#1e2024` | border-foreground/[0.08] on composer card |
| Sidebar selected row | foreground @ 9% over sidebar | `#191a1c` | modern:bg-foreground/[0.09]; JumpButton hover |
| Sidebar hover row | sidebar-accent @ 45% over sidebar | `#0a0c0f` | hover:bg-sidebar-accent/45 |
| Sidebar resting row text | sidebar-foreground @ 70% over sidebar | `#9b9da0` | text-sidebar-foreground/70 |
| Sidebar section label text | sidebar-foreground @ 60% over sidebar | `#86878a` | text-sidebar-foreground/60 |
| User panel card | foreground @ 4.5% over sidebar | `#0e1012` | floating footer card bg |
| User panel card ring | foreground @ 5% over sidebar | `#101113` | 1px inset ring |
| User panel button hover | foreground @ 6% over sidebar | `#121315` | profile button hover inside the card (approx.) |
| Rail hover ring | foreground @ 15% over rail | `#252628` | 2px ring on hovered rail tile |
| Message body text | foreground @ 90% over background | `#d6d7da` | text-foreground/90 |
| Primary hover | primary @ 90% over background | `#3b84d1` | hover:bg-primary/90 |
| Primary tint 5% | primary @ 5% over background | `#0a0f16` | row being replied to |
| Primary tint 10% | primary @ 10% over background | `#0c1621` | header channel chip, active header button, welcome tile |
| Primary tint 12% | primary @ 12% over background | `#0e1926` | own reaction pill fill |
| Primary tint 15% | primary @ 15% over background | `#0f1d2c` | active header button hover; ring-primary/15 |
| Primary border 45% | primary @ 45% over background | `#21466e` | own reaction border |
| Mention pill bg | primary @ 16% over background | `#101e2f` | .mention background |
| Mention pill bg hover | primary @ 28% over background | `#172f49` | .mention:hover |
| Selection | primary @ 35% over background | `#1b3858` | ::selection |
| Jump flash | primary @ 14% over background | `#0f1c2a` | flash keyframe start |
| Mentioned-me row | mention @ 10% over background | `#1e1911` | bg-mention/10 + 2px left bar in --mention |
| Focus ring (field) | ring @ 20% over background | `#122437` | 3px ring, ring-ring/20 |
| Focus border (field) | ring @ 70% over background | `#2f69a5` | 1px border, border-ring/70 |
| Focus ring (composer) | ring @ 10% over background | `#0c1621` | 3px ring, ring-ring/10 |
| Focus border (composer) | ring @ 45% over card | `#244a72` | 1px border, border-ring/45 |
| Focus ring (buttons) | ring @ 50% over background | `#244d79` | 3px ring, ring-ring/50 |
| New-messages line | destructive @ 60% over background | `#913031` | bg-destructive/60 1px line |
| Destructive tint 12% | destructive @ 12% over sidebar | `#200d0f` | muted/deafened button in user panel |
| Destructive menu item focus | destructive @ 20% over popover | `#3b1c1f` | destructive menu item highlight |
| Dialog scrim | black @ 55% over background | `#030405` | bg-black/55 + 2px backdrop blur |
| Scrollbar thumb | foreground @ 16% over background | `#2c2d30` | scrollbar-thin |
| Reply spine | muted-foreground @ 40% over background | `#3b3e44` | reply connector line |
| Button top highlight | white @ 22% over primary | `#6aaaec` | inset 0 1px 0 on primary Button |
| Header / panel divider | border x 70% over background | `#141518` | modern:border-border/70 under the 48px header and beside the member list |
| Border on background | border over background | `#191b1e` | default 1px border |
| Border on popover | border over popover | `#202226` | menu / popover edge and separators |
| Mention pill text | color-mix(in oklch, primary 70%, foreground) | `#7daeed` | .mention colour |

#### Derived colours — neutral light

| Name | Recipe | Hex | Used for |
| --- | --- | --- | --- |
| Field fill (rest) | foreground @ 4.5% over background | `#f1f1f3` | Input / Textarea / Select / InputGroup at rest |
| Field fill (hover) | foreground @ 6.5% over background | `#ecedef` | field hover |
| Field fill (rest) on card | foreground @ 4.5% over card | `#f4f5f5` | fields inside cards |
| Field fill (rest) on popover | foreground @ 4.5% over popover | `#f4f5f5` | fields inside popovers |
| Message row hover | foreground @ 3% over background | `#f4f5f6` | modern:bg-foreground/[0.03] |
| Composer disabled bar | foreground @ 4% over background | `#f2f3f4` | no-permission composer |
| Tabs list track | foreground @ 5% over background | `#eff0f2` | TabsList |
| Chip fill 6% | foreground @ 6% over background | `#edeef0` | reaction pill at rest, skeleton, kbd, member-row hover |
| Chip fill 7% | foreground @ 7% over background | `#ebeced` | secondary Badge, role chips (ghost Button hover uses --accent in light) |
| Fill 8% | foreground @ 8% over background | `#e9eaeb` | (dark-only recipe; light secondary Button uses --secondary) |
| Fill 10% | foreground @ 10% over background | `#e4e5e7` | (dark-only recipe; light active tab uses --background + soft shadow) |
| Fill 12% | foreground @ 12% over background | `#dfe0e2` | slider track (light secondary hover = --secondary @ 80%) |
| Fill 15% | foreground @ 15% over background | `#d8dadc` | Switch off track, hover hairlines (foreground/15) |
| Inset panel border (on panel) | foreground @ 10% over background | `#e4e5e7` | border-foreground/10 round the floating main panel |
| Inset panel border (on sidebar) | foreground @ 10% over sidebar | `#dedfe1` | same hairline seen against the sidebar colour |
| Composer border | foreground @ 8% over card | `#eceded` | border-foreground/[0.08] on composer card |
| Sidebar selected row | foreground @ 9% over sidebar | `#e0e1e4` | modern:bg-foreground/[0.09]; JumpButton hover |
| Sidebar hover row | sidebar-accent @ 45% over sidebar | `#edeff2` | hover:bg-sidebar-accent/45 |
| Sidebar resting row text | sidebar-foreground @ 70% over sidebar | `#626569` | text-sidebar-foreground/70 |
| Sidebar section label text | sidebar-foreground @ 60% over sidebar | `#77797e` | text-sidebar-foreground/60 |
| User panel card | foreground @ 4.5% over sidebar | `#eaebed` | floating footer card bg |
| User panel card ring | foreground @ 5% over sidebar | `#e9eaec` | 1px inset ring |
| User panel button hover | foreground @ 6% over sidebar | `#e7e8ea` | profile button hover inside the card (approx.) |
| Rail hover ring | foreground @ 15% over rail | `#cbcdd0` | 2px ring on hovered rail tile |
| Message body text | foreground @ 90% over background | `#2c2f34` | text-foreground/90 |
| Primary hover | primary @ 90% over background | `#3a81d4` | hover:bg-primary/90 |
| Primary tint 5% | primary @ 5% over background | `#f0f5fb` | row being replied to |
| Primary tint 10% | primary @ 10% over background | `#e5eef8` | header channel chip, active header button, welcome tile |
| Primary tint 12% | primary @ 12% over background | `#e1ebf8` | own reaction pill fill |
| Primary tint 15% | primary @ 15% over background | `#dbe7f6` | active header button hover; ring-primary/15 |
| Primary border 45% | primary @ 45% over background | `#9abfe8` | own reaction border |
| Mention pill bg | primary @ 16% over background | `#d9e6f6` | .mention background |
| Mention pill bg hover | primary @ 28% over background | `#bfd6f0` | .mention:hover |
| Selection | primary @ 35% over background | `#b0cced` | ::selection |
| Jump flash | primary @ 14% over background | `#dde9f7` | flash keyframe start |
| Mentioned-me row | mention @ 10% over background | `#f9f2e7` | bg-mention/10 + 2px left bar in --mention |
| Focus ring (field) | ring @ 20% over background | `#d0e1f4` | 3px ring, ring-ring/20 |
| Focus border (field) | ring @ 70% over background | `#659ddd` | 1px border, border-ring/70 |
| Focus ring (composer) | ring @ 10% over background | `#e5eef8` | 3px ring, ring-ring/10 |
| Focus border (composer) | ring @ 45% over card | `#9dc0ea` | 1px border, border-ring/45 |
| Focus ring (buttons) | ring @ 50% over background | `#90b8e6` | 3px ring, ring-ring/50 |
| New-messages line | destructive @ 60% over background | `#ea797c` | bg-destructive/60 1px line |
| Destructive tint 12% | destructive @ 12% over sidebar | `#f1dcde` | muted/deafened button in user panel |
| Destructive menu item focus | destructive @ 10% over popover | `#fce9e9` | destructive menu item highlight |
| Dialog scrim | black @ 55% over background | `#717172` | bg-black/55 + 2px backdrop blur |
| Scrollbar thumb | foreground @ 16% over background | `#d6d7d9` | scrollbar-thin |
| Reply spine | muted-foreground @ 40% over background | `#c0c2c6` | reply connector line |
| Button top highlight | white @ 22% over primary | `#5492da` | inset 0 1px 0 on primary Button |
| Header / panel divider | border x 70% over background | `#e9eaed` | modern:border-border/70 under the 48px header and beside the member list |
| Border on background | border over background | `#e1e3e6` | default 1px border |
| Border on popover | border over popover | `#e1e3e6` | menu / popover edge and separators |
| Mention pill text | color-mix(in oklch, primary 70%, foreground) | `#285696` | .mention colour |

### 2.5 How palettes are structured

Every palette defines the same 36 tokens for light and dark (`scripts/build-themes.mjs`). Two are hand-tuned (`neutral`, `turquoise`); the other eight come from one function, `derive({ hue, tint, depth, accent })`, which keeps neutral's lightness ladder and changes only:

- **the neutral hue and chroma**: every grey is `oklch(L, c × tint, hue)` with the same L as neutral. `tint` 0 is pure grey (mono), 1.7 is the most coloured (ocean).
- **`depth`**: added to the L of the dark surfaces only (midnight uses −0.075, which pushes rail and sidebar to true black).
- **the accent**: `primary`, `primary-foreground`, `ring`, `sidebar-primary`, `sidebar-primary-foreground`, `sidebar-ring`, `link`, for light and dark.

Fixed across all derived palettes: `card` and `popover` in light mode are pure white; `destructive`, `online`, `idle`, `dnd`, `mention` keep neutral's values; dark `border`, `input`, `sidebar-border` are always white at 7.5% / 11% / 6%.

So to add a palette later you need: a hue, a tint multiplier, an optional dark depth offset, and six accent colours. Parameters per palette:

| Palette | hue | tint | depth | Accent dark / on-accent / link | Accent light / on-accent / link |
| --- | --- | --- | --- | --- | --- |
| midnight | 270 | 0.6 | −0.075 | `0.72 0.12 275` / `0.18 0.04 275` / `0.78 0.1 260` | `0.5 0.17 275` / `0.99 0 0` / `0.5 0.16 265` |
| ocean | 235 | 1.7 | 0 | `0.74 0.13 220` / `0.18 0.05 235` / `0.8 0.1 215` | `0.52 0.13 235` / `0.99 0 0` / `0.5 0.14 240` |
| violet | 292 | 1.5 | 0 | `0.72 0.15 300` / `0.2 0.06 300` / `0.78 0.12 290` | `0.53 0.19 300` / `0.99 0 0` / `0.5 0.17 290` |
| rose | 350 | 1.2 | 0 | `0.74 0.14 355` / `0.2 0.06 355` / `0.8 0.1 340` | `0.56 0.19 5` / `0.99 0 0` / `0.52 0.16 355` |
| pomegranate | 25 | 1 | 0 | `0.59 0.2 25` / `0.99 0 0` / `0.78 0.12 35` | `0.54 0.21 25` / `0.99 0 0` / `0.5 0.18 30` |
| amber | 70 | 1.3 | 0 | `0.81 0.15 75` / `0.24 0.05 60` / `0.84 0.12 80` | `0.76 0.16 70` / `0.24 0.05 55` / `0.52 0.13 60` |
| forest | 150 | 1.3 | 0 | `0.75 0.15 155` / `0.2 0.05 155` / `0.8 0.12 160` | `0.52 0.13 155` / `0.99 0 0` / `0.48 0.12 160` |
| mono | 0 | 0 | 0 | `0.94 0 0` / `0.16 0 0` / `0.82 0.06 245` | `0.22 0 0` / `0.99 0 0` / `0.45 0.1 250` |

(values are OKLCH `L C H`.) The neutral ladder that `derive` reuses, as OKLCH lightness/chroma before the tint multiplier:

| Token | Light L / C | Dark L / C (L + depth where marked d) |
| --- | --- | --- |
| background | 0.99 / 0.002 | 0.135 d / 0.008 |
| foreground, card-foreground, popover-foreground, accent-foreground (light), sidebar-accent-foreground (light) | 0.21 / 0.012 | 0.95 / 0.003 (accent-foreground and sidebar-accent-foreground: 0.96 / 0.003) |
| card | white | 0.165 d / 0.009 |
| popover | white | 0.175 d / 0.009 |
| secondary | 0.955 / 0.004 | 0.205 d / 0.01 |
| secondary-foreground, sidebar-foreground | 0.27 / 0.012 | 0.92 / 0.004 (sidebar-foreground 0.9 / 0.005) |
| muted | 0.96 / 0.004 | 0.2 d / 0.01 |
| muted-foreground | 0.53 / 0.013 | 0.65 / 0.014 |
| accent | 0.945 / 0.005 | 0.215 d / 0.011 |
| border, sidebar-border | 0.915 / 0.005 | white @ 7.5% (sidebar-border white @ 6%) |
| input | 0.895 / 0.006 | white @ 11% |
| sidebar | 0.97 / 0.003 | 0.115 d / 0.008 |
| sidebar-accent | 0.93 / 0.006 | 0.19 d / 0.01 |
| rail | 0.945 / 0.004 | 0.095 d / 0.007 |
| tooltip | 0.22 / 0.012 | 0.26 d / 0.011 |
| tooltip-foreground | 0.97 / 0.003 | 0.97 / 0.003 |
| offline | 0.7 / 0.01 | 0.55 / 0.012 |
| code | 0.965 / 0.004 | 0.115 d / 0.008 |

Turquoise differs structurally from the derived palettes: its dark surfaces are lighter slate (background L 0.205 instead of 0.135), `rail` equals `sidebar`, `card` equals `popover`, `sidebar-accent` is a 6% overlay instead of a solid, light `background` is pure white, and in light mode `ring` is a lighter teal than `primary`. On dark turquoise, text on the accent is dark (`#001e1e`), not white.

### 2.6 All ten palettes at a glance

| Palette | Dark rail / sidebar / background / card / popover | Dark primary (text on it) | Dark foreground / muted-fg | Dark link | Light sidebar / background | Light primary (text on it) | Light link |
| --- | --- | --- | --- | --- | --- | --- | --- |
| neutral | `#020304` / `#040507` / `#07080b` / `#0c0e12` / `#0e1115` | `#4092e7` (`#fcfcfc`) | `#edeef0` / `#8a8f98` | `#65b2f1` | `#f4f5f7` / `#fbfcfd` | `#2474cf` (`#fcfcfc`) | `#006bbb` |
| turquoise | `#0d1013` / `#0d1013` / `#14171b` / `#1b1e23` / `#1b1e23` | `#42d8c8` (`#001e1e`) | `#eef0f3` / `#999fa7` | `#58d1e5` | `#eff1f4` / `#ffffff` | `#007f80` (`#f9fdfc`) | `#00769c` |
| midnight | `#000000` / `#000000` / `#010101` / `#020203` / `#030305` | `#8f9eef` (`#0d1023`) | `#eeeef0` / `#8d8f94` | `#92b9f8` | `#f4f5f6` / `#fbfcfd` | `#4b53c2` (`#fcfcfc`) | `#355bbd` |
| ocean | `#010305` / `#020609` / `#04090d` / `#080f14` / `#0a1216` | `#1ebde3` (`#001424`) | `#ebeff1` / `#82929c` | `#69cee6` | `#f2f6f8` / `#fafcfe` | `#0072a8` (`#fcfcfc`) | `#006aaa` |
| violet | `#030305` / `#050409` / `#08070d` / `#0e0d14` / `#100f16` | `#b48df4` (`#1b0e2d`) | `#eeeef1` / `#8f8d9b` | `#b6aaff` | `#f5f5f8` / `#fcfbfe` | `#7f47c7` (`#fcfcfc`) | `#654abb` |
| rose | `#040203` / `#070405` / `#0b0709` / `#120c0f` / `#140f11` | `#ee83af` (`#2a0817`) | `#f0edef` / `#988b90` | `#eaa4d2` | `#f7f4f5` / `#fdfbfc` | `#c83065` (`#fcfcfc`) | `#ab366e` |
| pomegranate | `#040202` / `#070404` / `#0b0707` / `#120d0c` / `#140f0e` | `#db373a` (`#fcfcfc`) | `#f0eeed` / `#988c8b` | `#fa9b82` | `#f7f4f4` / `#fdfbfb` | `#cd1526` (`#fcfcfc`) | `#b32517` |
| amber | `#040301` / `#070503` / `#0b0805` / `#120d09` / `#14100b` | `#f9b13e` (`#301904`) | `#f0eeec` / `#978d83` | `#f4c26a` | `#f7f5f2` / `#fdfbfa` | `#f09c17` (`#311805`) | `#9d5300` |
| forest | `#020302` / `#030604` / `#060906` / `#0b100c` / `#0d120e` | `#4ec983` (`#001c0b`) | `#edefed` / `#889289` | `#71d6a3` | `#f3f6f4` / `#fbfcfb` | `#007e46` (`#fcfcfc`) | `#007145` |
| mono | `#030303` / `#050505` / `#080808` / `#0e0e0e` / `#101010` | `#ebebeb` (`#0d0d0d`) | `#eeeeee` / `#8f8f8f` | `#a4c9e9` | `#f5f5f5` / `#fcfcfc` | `#1b1b1b` (`#fcfcfc`) | `#23588a` |

### 2.7 Other palettes in full (compact)

Alpha tokens show the colour, its alpha, and the result over the surface they belong to. The remaining five (ocean, violet, rose, pomegranate, forest) follow the `derive` rule in 2.5 and their key colours are in 2.6.

#### turquoise

| Token | Dark | Light |
| --- | --- | --- |
| `--background` | `#14171b` | `#ffffff` |
| `--foreground` | `#eef0f3` | `#12161c` |
| `--card` | `#1b1e23` | `#ffffff` |
| `--card-foreground` | `#eef0f3` | `#12161c` |
| `--popover` | `#1b1e23` | `#ffffff` |
| `--popover-foreground` | `#eef0f3` | `#12161c` |
| `--primary` | `#42d8c8` | `#007f80` |
| `--primary-foreground` | `#001e1e` | `#f9fdfc` |
| `--secondary` | `#23272c` | `#eef0f3` |
| `--secondary-foreground` | `#e6e8eb` | `#1d2228` |
| `--muted` | `#1d2125` | `#f1f3f5` |
| `--muted-foreground` | `#999fa7` | `#646971` |
| `--accent` | `#25292f` | `#ebeef2` |
| `--accent-foreground` | `#f4f5f7` | `#12161c` |
| `--destructive` | `#f04c55` | `#db2b33` |
| `--border` | `#ffffff @ 7.5% (= #26292c on background)` | `#e0e3e7` |
| `--input` | `#ffffff @ 11% (= #2e3134 on background)` | `#d6d9de` |
| `--ring` | `#42d8c8` | `#009a9b` |
| `--sidebar` | `#0d1013` | `#eff1f4` |
| `--sidebar-foreground` | `#d5d8db` | `#292e34` |
| `--sidebar-primary` | `#42d8c8` | `#007f80` |
| `--sidebar-primary-foreground` | `#001e1e` | `#f9fdfc` |
| `--sidebar-accent` | `#ffffff @ 6% (= #1c1e21 on sidebar)` | `#12161c @ 6% (= #e2e4e7 on sidebar)` |
| `--sidebar-accent-foreground` | `#f4f5f7` | `#12161c` |
| `--sidebar-border` | `#ffffff @ 6% (= #1c1e21 on sidebar)` | `#dde0e4` |
| `--sidebar-ring` | `#42d8c8` | `#009a9b` |
| `--rail` | `#0d1013` | `#eff1f4` |
| `--tooltip` | `#2c3136` | `#171b21` |
| `--tooltip-foreground` | `#f4f5f7` | `#f4f5f7` |
| `--online` | `#4ac777` | `#1da758` |
| `--idle` | `#f5ae39` | `#e49921` |
| `--dnd` | `#f45055` | `#e23439` |
| `--offline` | `#70757c` | `#94999e` |
| `--link` | `#58d1e5` | `#00769c` |
| `--mention` | `#efb146` | `#e19b1b` |
| `--code` | `#0f1215` | `#f2f4f6` |

#### midnight

| Token | Dark | Light |
| --- | --- | --- |
| `--background` | `#010101` | `#fbfcfd` |
| `--foreground` | `#eeeef0` | `#17181c` |
| `--card` | `#020203` | `#ffffff` |
| `--card-foreground` | `#eeeef0` | `#17181c` |
| `--popover` | `#030305` | `#ffffff` |
| `--popover-foreground` | `#eeeef0` | `#17181c` |
| `--primary` | `#8f9eef` | `#4b53c2` |
| `--primary-foreground` | `#0d1023` | `#fcfcfc` |
| `--secondary` | `#06070a` | `#eff0f2` |
| `--secondary-foreground` | `#e4e4e6` | `#25262a` |
| `--muted` | `#060609` | `#f1f2f3` |
| `--muted-foreground` | `#8d8f94` | `#6a6c70` |
| `--accent` | `#08090c` | `#ecedef` |
| `--accent-foreground` | `#f1f2f3` | `#17181c` |
| `--destructive` | `#ed4a49` | `#df2225` |
| `--border` | `#ffffff @ 7.5% (= #141414 on background)` | `#e2e3e5` |
| `--input` | `#ffffff @ 11% (= #1d1d1d on background)` | `#dbdcdf` |
| `--ring` | `#8f9eef` | `#4b53c2` |
| `--sidebar` | `#000000` | `#f4f5f6` |
| `--sidebar-foreground` | `#dddee0` | `#25262a` |
| `--sidebar-primary` | `#8f9eef` | `#4b53c2` |
| `--sidebar-primary-foreground` | `#0d1023` | `#fcfcfc` |
| `--sidebar-accent` | `#040507` | `#e7e8ea` |
| `--sidebar-accent-foreground` | `#f1f2f3` | `#17181c` |
| `--sidebar-border` | `#ffffff @ 6% (= #0f0f10 on sidebar)` | `#e2e3e5` |
| `--sidebar-ring` | `#8f9eef` | `#4b53c2` |
| `--rail` | `#000000` | `#ecedee` |
| `--tooltip` | `#111316` | `#191a1e` |
| `--tooltip-foreground` | `#f4f5f6` | `#f4f5f6` |
| `--online` | `#42c070` | `#28ad5e` |
| `--idle` | `#f5ae39` | `#e49921` |
| `--dnd` | `#f14d4c` | `#e23439` |
| `--offline` | `#707176` | `#9d9ea2` |
| `--link` | `#92b9f8` | `#355bbd` |
| `--mention` | `#efb146` | `#e49e22` |
| `--code` | `#000000` | `#f3f3f5` |

#### mono

| Token | Dark | Light |
| --- | --- | --- |
| `--background` | `#080808` | `#fcfcfc` |
| `--foreground` | `#eeeeee` | `#181818` |
| `--card` | `#0e0e0e` | `#ffffff` |
| `--card-foreground` | `#eeeeee` | `#181818` |
| `--popover` | `#101010` | `#ffffff` |
| `--popover-foreground` | `#eeeeee` | `#181818` |
| `--primary` | `#ebebeb` | `#1b1b1b` |
| `--primary-foreground` | `#0d0d0d` | `#fcfcfc` |
| `--secondary` | `#171717` | `#f0f0f0` |
| `--secondary-foreground` | `#e4e4e4` | `#262626` |
| `--muted` | `#161616` | `#f2f2f2` |
| `--muted-foreground` | `#8f8f8f` | `#6c6c6c` |
| `--accent` | `#191919` | `#ededed` |
| `--accent-foreground` | `#f2f2f2` | `#181818` |
| `--destructive` | `#ed4a49` | `#df2225` |
| `--border` | `#ffffff @ 7.5% (= #1b1b1b on background)` | `#e3e3e3` |
| `--input` | `#ffffff @ 11% (= #232323 on background)` | `#dcdcdc` |
| `--ring` | `#ebebeb` | `#1b1b1b` |
| `--sidebar` | `#050505` | `#f5f5f5` |
| `--sidebar-foreground` | `#dedede` | `#262626` |
| `--sidebar-primary` | `#ebebeb` | `#1b1b1b` |
| `--sidebar-primary-foreground` | `#0d0d0d` | `#fcfcfc` |
| `--sidebar-accent` | `#141414` | `#e8e8e8` |
| `--sidebar-accent-foreground` | `#f2f2f2` | `#181818` |
| `--sidebar-border` | `#ffffff @ 6% (= #141414 on sidebar)` | `#e3e3e3` |
| `--sidebar-ring` | `#ebebeb` | `#1b1b1b` |
| `--rail` | `#030303` | `#ededed` |
| `--tooltip` | `#242424` | `#1b1b1b` |
| `--tooltip-foreground` | `#f5f5f5` | `#f5f5f5` |
| `--online` | `#42c070` | `#28ad5e` |
| `--idle` | `#f5ae39` | `#e49921` |
| `--dnd` | `#f14d4c` | `#e23439` |
| `--offline` | `#717171` | `#9e9e9e` |
| `--link` | `#a4c9e9` | `#23588a` |
| `--mention` | `#efb146` | `#e49e22` |
| `--code` | `#050505` | `#f3f3f3` |

#### amber

| Token | Dark | Light |
| --- | --- | --- |
| `--background` | `#0b0805` | `#fdfbfa` |
| `--foreground` | `#f0eeec` | `#1d1711` |
| `--card` | `#120d09` | `#ffffff` |
| `--card-foreground` | `#f0eeec` | `#1d1711` |
| `--popover` | `#14100b` | `#ffffff` |
| `--popover-foreground` | `#f0eeec` | `#1d1711` |
| `--primary` | `#f9b13e` | `#f09c17` |
| `--primary-foreground` | `#301904` | `#311805` |
| `--secondary` | `#1b1611` | `#f2f0ec` |
| `--secondary-foreground` | `#e7e4e1` | `#2b251e` |
| `--muted` | `#1a1510` | `#f4f1ee` |
| `--muted-foreground` | `#978d83` | `#726a61` |
| `--accent` | `#1e1812` | `#f0ece8` |
| `--accent-foreground` | `#f3f1ef` | `#1d1711` |
| `--destructive` | `#ed4a49` | `#df2225` |
| `--border` | `#ffffff @ 7.5% (= #1d1a17 on background)` | `#e6e2de` |
| `--input` | `#ffffff @ 11% (= #262320 on background)` | `#e0dbd7` |
| `--ring` | `#f9b13e` | `#f09c17` |
| `--sidebar` | `#070503` | `#f7f5f2` |
| `--sidebar-foreground` | `#e1ddd9` | `#2b251e` |
| `--sidebar-primary` | `#f9b13e` | `#f09c17` |
| `--sidebar-primary-foreground` | `#301904` | `#311805` |
| `--sidebar-accent` | `#18130e` | `#ebe7e2` |
| `--sidebar-accent-foreground` | `#f3f1ef` | `#1d1711` |
| `--sidebar-border` | `#ffffff @ 6% (= #161412 on sidebar)` | `#e6e2de` |
| `--sidebar-ring` | `#f9b13e` | `#f09c17` |
| `--rail` | `#040301` | `#efece9` |
| `--tooltip` | `#29231d` | `#1f1913` |
| `--tooltip-foreground` | `#f7f5f2` | `#f7f5f2` |
| `--online` | `#42c070` | `#28ad5e` |
| `--idle` | `#f5ae39` | `#e49921` |
| `--dnd` | `#f14d4c` | `#e23439` |
| `--offline` | `#787068` | `#a49d96` |
| `--link` | `#f4c26a` | `#9d5300` |
| `--mention` | `#efb146` | `#e49e22` |
| `--code` | `#070503` | `#f6f3f0` |

### 2.8 Avatar and server-icon fallback tints

`components/app/avatar.tsx:190-200`, `components/app/server-icon.tsx`, `lib/hue.ts`. People and servers without a picture get initials on a tinted tile: "tinted, never saturated blocks". The hue is picked from eight values by hashing the id or name: `hash = (hash × 31 + charCode) | 0` over the string, then `HUES[abs(hash) % 8]` with `HUES = [252, 160, 75, 25, 300, 210, 130, 185]`.

- Avatar fallback: background `oklch(0.65 0.13 h / 0.18)`, initials `oklch(0.78 0.11 h)` in dark mode and `oklch(0.5 0.14 h)` in light, weight 500, font size `max(9, round(size × 0.36))` px. Initials: first two letters of a single word, or first letters of the first two words, upper-cased.
- Server icon fallback (modern): background `oklch(0.65 0.12 h / 0.2)`, initials `oklch(0.82 0.1 h)` dark / `oklch(0.45 0.12 h)` light, weight 600, font size `max(10, round(size × 0.36))` px (`× 0.28` when the acronym has three letters). Acronym: first letter of up to three words.

| Hue | Avatar fallback tint (solid, drawn @ 18%) | = over neutral-dark background | Initials, dark mode | Initials, light mode | Server-icon tint @ 20% over neutral-dark rail | Server initials, dark | Server initials, light |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 252 | `#4f93dc` | `#142131` | `#82bcfc` | `#1364b0` | `#13202e` | `#94c9ff` | `#165695` |
| 160 | `#31a773` | `#0e251e` | `#73cd9f` | `#007a46` | `#0e231b` | `#88d8ae` | `#00683d` |
| 75 | `#bd821a` | `#271e0e` | `#e1ad63` | `#905300` | `#271d0c` | `#eabb79` | `#7b4800` |
| 25 | `#d36c66` | `#2b1a1c` | `#f69b94` | `#a43b38` | `#2b1919` | `#ffaba3` | `#8c3432` |
| 300 | `#9c7bd2` | `#211d2f` | `#c2a7f4` | `#714ca6` | `#211b2c` | `#ceb6fc` | `#60438d` |
| 210 | `#00a3bb` | `#05242b` | `#51cade` | `#00768f` | `#022328` | `#6dd6e8` | `#00657a` |
| 130 | `#749f43` | `#1a2315` | `#a0c679` | `#477200` | `#192212` | `#afd28c` | `#3e6102` |
| 185 | `#00a89a` | `#052525` | `#55cec0` | `#007b6e` | `#022422` | `#70d9cc` | `#00695e` |

Other fixed colours: "streaming" status dot `oklch(0.62 0.18 300)` = `#9867e1`; text on destructive fills is pure white `#ffffff`.

---

## 3. What `data-style="modern"` changes

Every rule below was read from a `modern:` class or a `[data-style='modern']` CSS rule. "Classic" is what the same element looks like without it. Radii are resolved for `data-radius="lg"` (see section 5).

### 3.1 Elevation (shadows)

`app/globals.css:59-75`. Four shadow tokens; modern replaces Tailwind's stock shadows with softer, deeper ones plus a 1px inner top highlight in dark mode.

| Token | Used by | Classic | Modern |
| --- | --- | --- | --- |
| `shadow-raised` | Composer, message hover toolbar, small floating controls | `0 1px 3px 0 rgb(0 0 0 / 0.1), 0 1px 2px -1px rgb(0 0 0 / 0.1)` | `0 1px 2px <soft>, 0 6px 16px -8px <shadow>, inset 0 1px 0 <highlight>` |
| `shadow-popover` | Menus, popovers, tooltips, toasts, jump-to-present pill, connection pill | `0 4px 6px -1px rgb(0 0 0 / 0.1), 0 2px 4px -2px rgb(0 0 0 / 0.1)` | `0 2px 6px -2px <soft>, 0 16px 40px -12px <shadow>, inset 0 1px 0 <highlight>` |
| `shadow-dialog` | Dialogs, sheets, quick switcher, login card | `0 10px 15px -3px rgb(0 0 0 / 0.1), 0 4px 6px -4px rgb(0 0 0 / 0.1)` | `0 8px 20px -8px <soft>, 0 32px 72px -20px <shadow>, inset 0 1px 0 <highlight>` |
| `shadow-panel` | The floating main panel (inset layout) | none | `0 1px 3px <soft>` |

Format is CSS `x y blur spread colour`. Resolved colours: dark `<shadow>` = `rgba(0,0,0,0.5)`, `<soft>` = `rgba(0,0,0,0.28)`, `<highlight>` = `rgba(255,255,255,0.045)`; light `<shadow>` = `rgba(37,46,61,0.16)`, `<soft>` = `rgba(37,46,61,0.08)`, `<highlight>` = none.

The principle, quoted from the source comment: "floating layers get soft depth; in-flow surfaces step their background instead." Cards lose their shadow in modern (`modern:shadow-none`); fields lose theirs too.

### 3.2 Fields: Input, Textarea, Select trigger, InputGroup

`components/ui/input.tsx`, `textarea.tsx`, `select.tsx`, `input-group.tsx`. "Filled and borderless at rest; the accent edge and glow show while focused."

| Property | Classic | Modern |
| --- | --- | --- |
| Height | 36px (`h-9`); textarea min 64px | same |
| Padding | 12px horizontal, 4px vertical (textarea 8px vertical) | same |
| Radius | 8px | 10px |
| Fill at rest | transparent (dark: `input` @ 30%) | `foreground` @ 4.5%, both modes |
| Border at rest | 1px `input` | 1px transparent (keep the 1px box so focus does not shift layout) |
| Shadow | `0 1px 2px rgb(0 0 0 / 0.05)` | none |
| Hover | none | fill `foreground` @ 6.5% |
| Focus | border `ring`, 3px ring `ring` @ 50% | fill becomes transparent, border `ring` @ 70%, 3px ring `ring` @ 20% |
| Placeholder | `muted-foreground` | `muted-foreground` @ 80% |
| Text | 14px, 400 | same |
| Invalid | border `destructive`, ring `destructive` @ 20% (dark 40%) | same |
| Transition | colour, background, border, shadow over 150ms | same |
| Disabled | 50% opacity | same |

An InputGroup (field with an icon or button inside, used for the header search box) carries the fill, radius and focus ring on the group; the inner input is transparent with no ring.

### 3.3 Buttons

`components/ui/button.tsx`. Base: inline-flex, centred, gap 8px, 14px text, weight 500, transition 150ms ease-out on colour/background/border/shadow/transform, **press = scale 0.97**, disabled = 50% opacity, focus-visible = border `ring` + 3px ring `ring` @ 50%.

Sizes (height × horizontal padding; icons inside are 16px unless sized):

| Size | Box | Radius classic | Radius modern | Notes |
| --- | --- | --- | --- | --- |
| `default` | 36px high, 16px padding (12px when it contains an icon) | 8px | 10px | |
| `xs` | 28px high, 10px padding (8px with icon), gap 4px, 12px text | 8px | 10px (inferred: the `modern:rounded-lg` base class outranks the size's `rounded-md`) | |
| `sm` | 32px high, 12px padding (10px with icon), gap 6px | 8px | 10px (inferred, same reason) | |
| `lg` | 40px high, 24px padding (16px with icon) | 8px | 10px | |
| `icon` | 36 × 36 | 8px | 10px | |
| `icon-xs` | 28 × 28 | 8px | 10px (inferred) | message hover toolbar buttons |
| `icon-sm` | 32 × 32 | 8px | **8px** (explicit `modern:rounded-md`) | header, composer and user-panel buttons |
| `icon-lg` | 40 × 40 | 8px | 10px | |

Variants:

| Variant | Classic | Modern differences |
| --- | --- | --- |
| `default` (primary) | fill `primary`, text `primary-foreground`, hover fill `primary` @ 90% | adds shadow `inset 0 1px 0 rgba(255,255,255,0.22), 0 1px 2px <soft>`: "a faint top highlight so solid buttons read as pressable" |
| `destructive` | fill `destructive` (dark: @ 60%), text white, hover @ 90% | dark uses full `destructive`; shadow `inset 0 1px 0 rgba(255,255,255,0.16), 0 1px 2px <soft>` |
| `outline` | 1px border, fill `background` (dark: `input` @ 30%, border `input`), small shadow, hover fill `accent` | border `input`, fill transparent, no shadow; dark hover fill `foreground` @ 6% |
| `secondary` | fill `secondary`, text `secondary-foreground`, hover `secondary` @ 80% | dark: fill `foreground` @ 8%, hover `foreground` @ 12% |
| `ghost` | no fill; hover fill `accent` (dark `accent` @ 50%), text `accent-foreground` | text `foreground` @ 90%; dark hover fill `foreground` @ 7% |
| `link` | text `primary`, underline on hover (offset 4px) | same |

Most toolbar-type ghost buttons in the chat UI additionally set text `muted-foreground` at rest and `foreground` on hover.

### 3.4 Menus (dropdown and context) and Select lists

`components/ui/dropdown-menu.tsx`, `context-menu.tsx`, `select.tsx`. "Roomier menus".

| Property | Classic | Modern |
| --- | --- | --- |
| Surface | `popover` fill, `popover-foreground` text, 1px `border`, `shadow-popover` | same tokens (the shadow token itself is deeper, see 3.1) |
| Container radius | 8px | **14px** |
| Container padding | 4px | **6px** |
| Min width | 128px; chat menus set 224px (`w-56`), submenus 208px | same |
| Item | 14px text, padding 8px × 6px (so 32px tall), gap 8px, icon 16px in `muted-foreground` | same |
| Item radius | 6px | **8px** |
| Item hover / keyboard focus | fill `accent`, text `accent-foreground`; no transition | same |
| Destructive item | text and icon `destructive`; focus fill `destructive` @ 10% (dark 20%) | same |
| Disabled item | 50% opacity | same |
| Checkbox / radio item | 32px left padding, indicator (16px check or 8px filled dot) at left 8px | same |
| Submenu trigger | like an item, 16px chevron-right at the far right, stays `accent` while open | same |
| Label | 14px, 500, padding 8px × 6px | **12px, `muted-foreground`**, padding 8px horizontal, 6px top, 4px bottom |
| Separator | 1px `border`, margin 4px vertical, bleeds 4px into the padding each side | margin **6px** vertical, bleeds 6px (always edge to edge) |
| Shortcut text | 12px, `muted-foreground`, letter-spacing 0.1em, right aligned | same |
| Offset from trigger | 4px; kept 8px from window edges | same |
| Open animation | fade in + scale from 95%, 150ms, ease-out, origin at the trigger | same |
| Close animation | fade out + scale to 95%, 100ms | same |
| Max height | available height, scrolls | same |

Select lists use the same surface, item and animation rules (14px container radius in modern), but their inner padding stays 4px and the selected item shows a 16px check at the right.

### 3.5 Popovers

`components/ui/popover.tsx`: `popover` fill, 1px `border`, `shadow-popover`, default width 288px, padding 16px, offset 4px. Radius 8px classic, **14px modern**. Same open/close animation as menus. Pickers (emoji) use `padding: 0` and size themselves.

### 3.6 Tooltips

`components/ui/tooltip.tsx`.

| Property | Classic | Modern |
| --- | --- | --- |
| Colours | fill `foreground`, text `background` (inverted) | fill `tooltip`, text `tooltip-foreground` (a dark chip in both modes) |
| Shadow | none | `shadow-popover` |
| Radius | 8px | 10px |
| Padding | 8px × 4px | 10px × 6px |
| Text | 12px, 500, balanced wrapping | same |
| Arrow | none | none |
| Offset | 6px | same |
| Delay | 400ms before the first tooltip; moving to a neighbour within 300ms opens instantly | same |
| Animation | only the delayed open animates: fade + scale from 97%, 150ms ease-out; close: fade 100ms | same |

A tooltip with a shortcut shows a `Kbd` chip after the label (gap 8px, right padding 4px); inside a modern tooltip the chip is white @ 10% fill, white @ 10% border, text `tooltip-foreground` @ 80%.

### 3.7 Dialogs, alert dialogs, sheets

`components/ui/dialog.tsx`, `alert-dialog.tsx`, `sheet.tsx`.

| Property | Classic | Modern |
| --- | --- | --- |
| Scrim | black @ 50% | black @ 55% **plus a 2px backdrop blur** (sheets: 55%, no blur) |
| Surface | `background` fill, 1px `border`, `shadow-dialog` | same tokens |
| Radius | 10px | **18px** |
| Padding | 24px | same |
| Gap between header / body / footer | 16px | **20px** (dialogs that lay out their own panes keep their own gap) |
| Max width | 512px default, never closer than 16px to the window edge | same |
| Header | column, gap 8px | gap 6px, 24px right padding (clears the close button) |
| Title | 18px, 600, line-height 1 | line-height 1.25, letter-spacing −0.025em |
| Description | 14px, `muted-foreground` | same |
| Footer | buttons right-aligned, gap 8px | same |
| Close button | 16px X at top/right 16px, 70% opacity, 100% on hover | "a real close button": 28 × 28 box at top/right **14px**, radius 8px, icon `muted-foreground`; hover fill `accent`, icon `foreground` |
| Open animation | fade + scale from 96%, 250ms ease-out | same |
| Close animation | fade + scale to 98%, 150ms | same |
| Sheet (side drawer) | slides from its edge, 300ms in / 200ms out, curve `cubic-bezier(0.32, 0.72, 0, 1)` | same |

Alert dialog buttons: confirm = primary button, cancel = outline button.

### 3.8 Cards, badges, separators, small controls

| Component | Classic | Modern |
| --- | --- | --- |
| Card (`ui/card.tsx`) | `card` fill, 1px `border`, radius `xl`, 24px vertical padding, small shadow | **no shadow** |
| Badge (`ui/badge.tsx`) | 12px 500, padding 8px × 2px, radius 8px | **fully round (pill)**; `secondary` variant fill `foreground` @ 7% |
| Separator | 1px `border` | same; several chat dividers use `border` @ 70% (see 3.9) |
| Switch | 32 × 18.4px, 1px transparent border, thumb 16px | **36 × 20px**, 2px transparent border, off track `foreground` @ 15%, on track `primary`, thumb 16px **white** with shadow `0 1px 2px rgba(0,0,0,0.25)`, travels 16px, 200ms ease-out |
| Checkbox | 16px, radius 4px, 1px `input` border, small shadow; checked: `primary` fill, 14px check in `primary-foreground` | radius **5px**, no shadow |
| Radio | 16px circle, 1px `input` border, 8px `primary` dot | unchanged |
| Slider | track 6px `muted`, range `primary`, thumb 16px white with 1px `primary` border, hover/focus ring 4px `ring` @ 50% | track `foreground` @ 12%, thumb border **2px**, ring `ring` @ 30%, thumb shadow `0 1px 3px <shadow>` |
| Tabs | list 36px high, radius 10px, 3px padding, fill `muted`; trigger radius 8px, 14px 500; active: `background` fill + small shadow (dark: `input` @ 30% fill, `input` border) | list fill `foreground` @ 5%; triggers `muted-foreground`, hover `foreground`; active: text `foreground`, shadow `0 1px 2px <soft>`, dark fill `foreground` @ 10% with no border |
| Toggle | hover `muted` / text `muted-foreground`; on: `accent` | hover fill `accent`, text `foreground`; on: text `foreground` |
| Kbd | 20px high, min-width 20px, radius 6px, fill `muted`, 12px 500 `muted-foreground`, padding 4px | fill `foreground` @ 6%, radius **5px**, 1px border with a **2px bottom border** (a keycap), padding 6px, 11px |
| Skeleton | fill `accent`, pulsing, radius 8px | fill `foreground` @ 6% |
| Progress | 8px bar, track `primary` @ 20%, fill `primary`, fully round | unchanged |
| Empty-state icon tile | 40px, radius 10px, fill `muted`, icon 24px | 44px, radius 14px, fill `primary` @ 10%, icon 20px in `primary`, 1px inset ring `primary` @ 15% |
| Toast (sonner) | bottom-right, `popover` fill, 1px `border`, radius `radius × 1.25` (12.5px at lg), `shadow-popover` | same (shadow token is deeper) |
| List item (`ui/item.tsx`) | radius 8px; muted variant `muted` @ 50%; icon tile 32px bordered | radius 10px; muted variant `foreground` @ 3.5%; icon tile borderless, fill `foreground` @ 6%, radius 10px |

### 3.9 Chat-specific modern rules

These are restated with full detail in section 10; listed here so the classic/modern difference is in one place.

| Element | Classic | Modern |
| --- | --- | --- |
| App sidebar total width | 296px (18.5rem) | **316px** (19.75rem) |
| Server rail width | 56px, gap 8px, 12px top padding | **68px**, gap 10px, 16px top padding |
| Rail tile | 36px, radius 10px, unselected tiles 75% opacity | **40px**, radius 14px, always 100% opacity; hover shows a 2px ring `foreground` @ 15% |
| Rail unread dot | 8px, `foreground`, 2px ring | **10px, `primary`**, 3px ring in `rail` colour |
| Rail count badge | 16px, 2px ring | 18px, 3px ring |
| Rail "add" tile | dashed border | borderless, fill `sidebar-accent`, icon turns `primary` on hover |
| Selected sidebar row | fill `sidebar-accent` | fill **`foreground` @ 9%**; its leading icon turns **`primary`** |
| Server header button | 32px, radius 8px | 40px, radius 10px, icon 24px |
| Search/jump button | bordered, `background` @ 40% | borderless, fill `sidebar-accent`, radius 10px, hover `foreground` @ 9% |
| User panel | flat strip with a top border | **floating card**: 8px margin (4px top), radius 14px, fill `foreground` @ 4.5%, 1px inset ring `foreground` @ 5%, no top border |
| Channel header border | `border` | `border` @ 70%; horizontal padding 16px |
| Channel header icon | bare `#` in `muted-foreground` | **28px chip**, radius 10px, fill `primary` @ 10%, icon `primary` |
| Channel title | 14px 600 | **15px 600, letter-spacing −0.025em** |
| DM header avatar | 22px | 26px |
| Active header button | fill `accent`, text `foreground` | fill `primary` @ 10%, icon `primary`; hover `primary` @ 15% |
| Message row hover | fill `muted` @ 40% | fill `foreground` @ 3% |
| Author name | 14px 500 | 14px **600** |
| Message hover toolbar | radius 8px, 1px gap, small shadow, right 12px, 14px above the row | radius 10px, 2px gap, `shadow-raised`, right 16px, 16px above |
| Reaction chip | 24px high, radius 8px, 12px 500, fill `muted` | **28px high, pill**, 12px 600, gap 6px, padding 8px, fill `foreground` @ 6%; own: border `primary` @ 45%, fill `primary` @ 12%, text `primary` |
| Composer | radius 10px, 1px `input` border, small shadow; focus border `ring` @ 60% | radius **14px**, border `foreground` @ 8%, fill `card`, `shadow-raised`; focus border `ring` @ 45% + 3px ring `ring` @ 10% |
| Read-only composer bar | 40px, dashed border, radius 10px | 44px, borderless, fill `foreground` @ 4%, radius 14px |
| Welcome tile (top of a channel) | 48px, radius 10px, bordered `card` | 56px, radius 18px, fill `primary` @ 10%, icon 24px `primary`, inset ring `primary` @ 15% |
| Member list | 240px wide, border `border` | 256px, border `border` @ 70% |
| Member row | min 48px, radius 8px, avatar 36px, hover `accent` @ 70% | min **52px**, radius 10px, avatar **38px**, hover `foreground` @ 6% |
| Side panel header icon | bare icon | 28px chip, radius 10px, `primary` @ 10% / `primary` |
| Speaking ring, "in call" text, connected-voice icon | `online` (green) | `primary` |
| Settings nav | 8px padding, bordered groups, right border | 10px padding, groups separated by 12px space, no right border |
| Login / invite / onboarding | bare form on `background` | card on `sidebar` background: radius 18px, 1px border, `background` fill, padding 24px (32px on wide screens), max width 400px, `shadow-dialog`, with a soft radial glow of `primary` @ 16% from the top centre (520px tall) |

### 3.10 Scrollbars, selection, focus outline (same in classic and modern)

- Scrollbars (`scrollbar-thin`, `app/globals.css:337-340`): CSS `scrollbar-width: thin`, thumb `foreground` @ 16%, track transparent. In Chromium on Windows a thin scrollbar is about 8px wide with square ends (inferred; the app sets no width, radius or hover colour). The server rail hides its scrollbar entirely.
- Text selection: `primary` @ 35%.
- Every element's default border colour is `border`; default outline colour is `ring` @ 50%.
- Pointer cursor on every enabled button.

---

## 4. `data-layout="inset"`: the floating main panel

Source: `components/shell/app-shell.tsx:105-143`, `components/ui/sidebar.tsx:311-325`, commit `0d3418d` ("even inset gaps").

```
window (desktop app: 32px title bar in --rail on top, see section 11)
+------+---------------------+--+--------------------------------------------------+--+
| rail | conversation column |8 |  8px gap (shows --sidebar)                        |8 |
| 68px | 248px               |px| +----------------------------------------------+ |px|
|      |                     |  | | main panel                                   | |  |
| bg:  | bg: --sidebar       |  | | bg: --background                             | |  |
| rail |                     |  | | 1px border: foreground @ 10%                 | |  |
|      |                     |  | | radius 14px, contents clipped                | |  |
|      | [user card, m=8]    |  | | shadow: 0 1px 3px <soft>                     | |  |
|      |                     |  | +----------------------------------------------+ |  |
|      |                     |  |  8px gap                                         |  |
+------+---------------------+--+--------------------------------------------------+--+
```

| Property | Value |
| --- | --- |
| What sits behind the panel | The whole app shell is painted `sidebar` (`inset:bg-sidebar`). The gap around the panel is therefore the sidebar colour, so the conversation column and the gap read as one continuous surface: "rail and channels sit on the window chrome, the main view is a raised rounded panel". |
| Gap around the panel | **8px on all four sides** (`m-2`): top, right, bottom, and between the conversation column and the panel. The sidebar has no padding of its own. |
| Panel fill | `background` |
| Panel border | 1px `foreground` @ 10% ("a hairline edge that shows on solid themes and gradients alike"). Neutral dark: `#1e1f22` against the panel, `#1b1c1f` against the sidebar. |
| Panel radius | `rounded-xl` = radius + 4px = **14px** at `data-radius="lg"` |
| Panel shadow | `shadow-panel` = `0 1px 3px rgba(0,0,0,0.28)` dark, `0 1px 3px rgba(37,46,61,0.08)` light. Very subtle; the border does most of the work. |
| Panel overflow | hidden: header, message list, composer and right-hand panels are all clipped by the rounded corners. |
| Sidebar (rail + column) width | fixed **316px** in modern (296px classic). Rail 68px + column 248px. Not resizable, not collapsible on desktop. |
| Sidebar borders | None in inset. The `flat:` borders (right edge of the sidebar, right edge of the rail, bottom of the sidebar header) are removed, so nothing separates rail, column and gap except the tiny lightness step between `rail` and `sidebar`. |
| Sidebar header offset | The column's 48px header gets an 8px top margin (`inset:mt-2`) so it lines up with the panel's 48px header, whose top edge is 8px down. |
| Rail top padding | 16px (modern), 12px bottom. |
| Banners | Update / version / connection / push banners render inside the panel, above the view. |
| Below 768px width | The sidebar becomes a slide-in sheet and the panel fills the window with no margin, radius or border. The desktop window's minimum width is 940px, so this never happens in the desktop app. |

Right-hand panels (member list, pins, search, inbox, DM profile) are **inside** the floating panel, separated from the chat by a 1px left border in `border` @ 70%; they do not float separately. The channel header spans the full panel width above both.

In neutral dark the three surfaces are `rail #020304`, `sidebar #040507`, panel `#07080b`: almost black, distinguished mainly by the hairline. In neutral light: `rail #ebedef`, `sidebar #f4f5f7`, panel `#fbfcfd` with a `#e4e5e7` edge.

---

## 5. Radius scale (`data-radius="lg"`)

`app/globals.css:77-94, 288-292`. `--radius` = 0.625rem = **10px**. The named steps are offsets from it:

| Tailwind class | Formula | px at `lg` | For comparison, classic `md` (8px) |
| --- | --- | --- | --- |
| `rounded-sm` | radius − 4 | **6** | 4 |
| `rounded-md` | radius − 2 | **8** | 6 |
| `rounded-lg` | radius | **10** | 8 |
| `rounded-xl` | radius + 4 | **14** | 12 |
| `rounded-2xl` | radius + 8 | **18** | 16 |
| `rounded-xs` | fixed | 2 | 2 |
| `rounded` (bare) | fixed | 4 | 4 |
| `rounded-full` | fixed | pill / circle | same |

Other radius settings for reference: `none` 0, `sm` 6, `md` 8, `lg` 10, `xl` 14, `2xl` 18 (the base `--radius`).

What uses which, in the modern style:

| Radius | Components |
| --- | --- |
| 18px (`2xl`) | Dialogs, alert dialogs, login/onboarding card, welcome tile at the top of a channel, "what's new" dialog |
| 14px (`xl`) | Floating main panel, menus, popovers, composer, user-panel card, rail tiles, pinned/search result cards, quick switcher, drag-and-drop overlay, incoming-call card, read-only composer bar, empty-state icon tile |
| 12.5px | Toasts (`radius × 1.25`) |
| 10px (`lg`) | Buttons (all sizes except `icon-sm`), inputs, selects, textareas, tooltips, tabs list, message hover toolbar, member rows, server header button, search/jump button, header icon chip, user-panel profile button, attachment images, file cards, embeds, composer attachment tiles, code blocks (`--radius`), server icon inside a rail tile (inline: `round(36 × 0.28)`) |
| 8px (`md`) | Menu items, `icon-sm` buttons (header/composer/user panel), channel rows, DM rows, settings nav rows, dialog close button, tab triggers, autocomplete rows, emoji-picker category buttons, quick-reaction buttons in the toolbar |
| 6px (`sm`) | Small thumbnails inside attachment tiles |
| 5px | Checkbox, Kbd |
| 4px | Mention pill, inline code, spoiler, bot tag, tiny 20px icon buttons (`rounded`) |
| pill | Reaction chips, badges, count badges, status dots, switch, jump-to-present button, connection pill, slider |

Avatars do **not** follow the radius setting. The default avatar shape is a rounded square with radius `max(4, round(size × 0.24))` (`components/profile/flair/shape.ts:24-27`): 14px avatar → 4, 20 → 5, 26 → 6, 28 → 7, 30 → 7, 32 → 8, 38 → 9, 56 → 13. Users can pick other shapes (circle, square, squircle, hexagon, blob) per profile. Server icons use `max(6, round(size × 0.28))`.

---

## 6. Density (`default`)

`app/globals.css:124-130`. Density only changes Tailwind's spacing unit `--spacing`: `compact` 0.225rem (3.6px), **`default` 0.25rem (4px, Tailwind's own default; no override is written)**, `comfortable` 0.275rem (4.4px). Every spacing utility (`h-*`, `w-*`, `size-*`, `p-*`, `m-*`, `gap-*`, `space-*`, `inset-*`) is a multiple of it.

Density does **not** change: font sizes and line heights, radii, border widths, anything written as an explicit px value (rail width 68px, avatar sizes, status dots, the 60px message gutter, 11px text), or rem-based widths (sidebar 19.75rem).

Key spacing-driven metrics at the default 4px unit:

| Element | Value |
| --- | --- |
| Channel header, sidebar header, side-panel header, settings header | 48px high |
| Channel row, friends row, settings nav row, category header band | 32px high |
| DM row | 40px high |
| User panel inner row | 52px high |
| Member row | min 52px |
| Menu item | 32px high (20px line + 6px + 6px) |
| Autocomplete row | 32px high |
| Default button / input | 36px high |
| Icon buttons in header, composer, user panel | 32 × 32 |
| Toolbar buttons on message hover | 28 × 28 |
| Reaction chip | 28px high |
| Composer text area | min 32px, inner padding 6px, so the composer is 46px tall when empty (inferred: 32 + 12 + 2px border) |
| Standard horizontal page padding in the chat column | 16px |
| Sidebar list horizontal padding | 8px (rows then pad 8px inside) |
| Row internal gap (icon to label) | 8px (DM rows 10px, member rows 12px) |
| Gap between sibling rows | 0 in the channel list; 2px in the DM list and member list |
| Message group spacing | 12px above each new group; 1px vertical padding per continued message |
| Message list bottom padding | 20px |
| Dialog padding / gap | 24px / 20px |
| Menu padding | 6px |

---

## 7. Typography

### 7.1 Fonts

Source: `app/layout.tsx:14-24`, `app/globals.css:96-122, 247-249`.

- **UI font: Figtree** (variable font, weight axis 300 to 900, normal style only), loaded through `next/font/google` with subsets `latin` and `latin-ext`, `display: swap`. CSS variable `--font-figtree`; full stack: `Figtree, "Figtree Fallback", ui-sans-serif, system-ui, -apple-system, 'Segoe UI', sans-serif`.
- **The font file is not bundled in the repo as a source asset.** `next/font` downloads it from Google Fonts at build time. The only copies on disk are build cache artefacts with hashed names: `C:\Projects\diskurt\.next\dev\static\media\400bf8aa837fcb7e-s.0end3qt1tw3rz.woff2` (latin-ext range) and `C:\Projects\diskurt\.next\dev\static\media\f7aa21714c1c53f8-s.0bhxxck2.9j91.woff2` (latin range). They are subset WOFF2 files and may be regenerated or renamed by any build; for a native app, obtain the full Figtree variable TTF (SIL Open Font License, published on Google Fonts) and bundle it. Turkish needs the latin-ext glyphs.
- Fallback while loading: Arial with `size-adjust: 100.72%`, `ascent-override: 94.32%`, `descent-override: 24.82%`, `line-gap-override: 0%`. These are Figtree's metrics relative to Arial, useful if you need to match line boxes.
- **Monospace: Geist Mono** (`--font-geist-mono`), stack `ui-monospace, 'Cascadia Code', 'SFMono-Regular', Menlo, monospace`. Used for inline code and code blocks only. Also not bundled.
- Emoji: `'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji'` (so Segoe UI Emoji on Windows).
- Rendering: `-webkit-font-smoothing: antialiased` on `body`.
- Weights actually used in the shell, chat, UI kit (class counts): 500 medium (76 uses), 600 semibold (30), 400 normal (the default body weight), 700 bold (1: the bot tag). No light weights, no italics except "original message deleted" and code comments.
- Numbers that change in place (counts, timers, percentages) use tabular figures.

### 7.2 Size scale

Root is 16px. Tailwind's named sizes carry a default line height; arbitrary sizes (`text-[11px]`) inherit the parent's line height, 1.5 from the page unless the container sets `leading-*` (inferred from Tailwind's base styles).

| Class | Size / line height | Class uses in shell + chat + UI kit |
| --- | --- | --- |
| `text-xs` | 12px / 16px | 82 |
| `text-sm` | 14px / 20px | 87 |
| `text-[11px]` | 11px | 12 |
| `text-[10px]` | 10px | 9 |
| `text-[13px]` | 13px | 9 |
| `text-[15px]` | 15px | 3 |
| `text-base` | 16px / 24px | 5 |
| `text-lg` | 18px / 28px | 6 |
| `text-xl` | 20px / 28px | 4 |

Line-height helpers: `leading-none` 1, `leading-tight` 1.25, `leading-snug` 1.375, `leading-relaxed` 1.625, `leading-5` 20px. Letter-spacing: `tracking-tight` −0.025em, `tracking-wide` 0.025em, `tracking-widest` 0.1em. The interface is 14px-based; nothing in the chat shell is larger than 20px.

### 7.3 Text styles by element (modern)

| Element | Size | Weight | Line height | Colour | Notes |
| --- | --- | --- | --- | --- | --- |
| Sidebar item (channel row) | 14px | 400; **500 when selected or unread** | 20px | `sidebar-foreground` @ 70%; selected `sidebar-accent-foreground`; unread `sidebar-foreground` | truncates with ellipsis |
| DM / conversation row name | 14px | 400; 500 when selected or unread | 1.25 | as above | |
| DM row subtitle (status, member count) | 11px | 400 | 1.25 | `muted-foreground` (`primary` when "in call") | |
| Sidebar section header (category, "Direct messages") | 12px | 500 | 16px | `sidebar-foreground` @ 60%, hover `sidebar-foreground` | sentence case, **not** upper-cased, no letter-spacing |
| Server name in sidebar header | 14px | 600 | 20px | `sidebar-foreground` | |
| Search/jump button label | 12px | 400 | 16px | `muted-foreground` | Kbd inside: 10px |
| Channel / chat title (header) | **15px** | **600** | about 21px (inferred; single line in a 48px bar) | `foreground` | letter-spacing −0.025em |
| Header topic / presence text | 12px | 400 | 16px | `muted-foreground` | |
| Message author | 14px | **600** | 20px | `foreground`, or the user's role/name colour | underline on hover |
| Message timestamp (after the name) | 11px | 400 | 20px row | `muted-foreground` | "Today at 19:53" form; short clock form `10px` in the gutter on hover |
| Message body | `--chat-font-size`, **14px** default (12 to 20) | 400 | **1.45** (20.3px) | `foreground` @ 90% | bold in text is 600; wraps anywhere |
| "(edited)" tag | 10px | 400 | | `muted-foreground` | |
| Reply preview line | 12px | 400, author name 500 | 16px | `muted-foreground`, hover `foreground` | |
| System line (joined, pinned, call) | 14px | 400, names 500 | 20px | `muted-foreground`, names `foreground` | |
| Date separator label | 11px | 500 | | `muted-foreground` | |
| "New" separator label | 10px | 600 | | `destructive` | |
| Typing indicator | 11px | 400, names 500 | | `muted-foreground`, names `foreground` | |
| Composer text and placeholder | 14px (follows chat font size) | 400 | 1.45 | text `foreground`; placeholder `muted-foreground` | |
| Member list section header | 12px | 600 | 16px | `muted-foreground` | count after an em dash at 70% opacity, weight 500 |
| Member name | 15px | 600 | 1.25 | `foreground` or name colour | |
| Member status line | 12px | 400 | 16px | `muted-foreground` | |
| User panel name / status | 14px / 11px | 500 / 400 | 1.25 | `sidebar-foreground` / `muted-foreground` | |
| Button | 14px (12px for `xs`) | 500 | 20px | per variant | |
| Input | 14px | 400 | | `foreground`; placeholder `muted-foreground` @ 80% | |
| Badge | 12px | 500 | 16px | per variant | |
| Count badge (unread/mentions) | 10px | 600 | 1 | white on `destructive` | tabular figures; "99+" above 99 |
| Bot tag | 9px | 700 | | `primary-foreground` on `primary` | letter-spacing 0.025em |
| Kbd | 11px | 500 | | `muted-foreground` | |
| Tooltip | 12px | 500 | 16px | `tooltip-foreground` | |
| Menu item | 14px | 400 | 20px | `popover-foreground` | |
| Menu label | 12px | 500 | 16px | `muted-foreground` | |
| Dialog title | 18px | 600 | 1.25 | `foreground` | letter-spacing −0.025em |
| Dialog description | 14px | 400 | 20px | `muted-foreground` | |
| Settings page heading | 18px | 600 | 28px | `foreground` | letter-spacing −0.025em |
| Welcome heading (top of a chat) | 20px | 600 | 28px | `foreground` | letter-spacing −0.025em |
| Reaction count | 12px | 600 | | `muted-foreground`; own `primary` | tabular figures |
| Inline code | 0.86em mono | 400 | | inherits | |
| Code block | 0.84em mono | 400 | 1.5 | inherits | |
| Title bar wordmark (desktop) | 12px | 600 | | title bar muted colour | Segoe UI Variable Text, letter-spacing 0.01em |

---

## 8. Icons

- Library: **lucide** (`lucide-react ^0.564.0`). 24 × 24 viewBox outline icons, round line caps and joins, no fill.
- Stroke width: lucide's default is 2. A global rule rewrites it: `svg.lucide[stroke-width='2'] { stroke-width: var(--icon-stroke) }` (`app/globals.css:327-329`). `thin` = 1.5, **`medium` = 1.75**, `regular` = 2. The stroke is in viewBox units, so a 16px icon at `medium` draws a line about 1.17px wide, a 20px icon about 1.46px (inferred). Icons given an explicit stroke keep it (the tiny channel lock uses 3, the file placeholder 1.5).
- Default size: **16px** inside buttons, menu items, inputs and rows.
- Other sizes in use: 14px (small row actions, search glyph, system-message icons, chevrons in the sidebar header), 12px (category chevron, voice-state glyphs, crown), 20px (rail tile glyphs in modern, empty-state tile), 24px (welcome tile, upload overlay), 18px (admin shield in the rail).
- Colour: `currentColor`. Icons at rest are usually `muted-foreground` and switch to `foreground` on hover; menu icons are always `muted-foreground` unless the item is destructive.

Icons used on the chat surface (lucide names): `MessagesSquare` (home/DM tile), `Plus`, `Download`, `ShieldCheck`, `Search`, `ChevronsUpDown`, `ChevronDown`, `ChevronRight`, `Hash`, `Volume2`, `Megaphone`, `Lock`, `Users`, `X`, `Settings`, `UserPlus`, `Mic`, `MicOff`, `Headphones`, `HeadphoneOff`, `Phone`, `Video`, `Bell`, `BellOff`, `Pin`, `PinOff`, `CircleUser`, `Inbox`, `Smile`, `SmilePlus`, `SendHorizontal`, `CornerUpLeft` (reply), `Pencil`, `MoreHorizontal`, `Copy`, `Link2`, `MailOpen`, `Trash2`, `EyeOff`, `ArrowDown`, `ArrowRight`, `AtSign`, `FileText`, `FileAudio`, `Upload`, `Check`, `Crown`, `WifiOff`, `Menu`.

---

## 9. Motion

Source: `app/globals.css:300-305` and component classes. The source comment: "strong custom curves; the built-in ones are too weak."

| Name | Curve |
| --- | --- |
| `ease-out` (used almost everywhere) | `cubic-bezier(0.23, 1, 0.32, 1)` |
| `ease-in-out` | `cubic-bezier(0.77, 0, 0.175, 1)` |
| `ease-drawer` (sheets) | `cubic-bezier(0.32, 0.72, 0, 1)` |

| Interaction | Animation |
| --- | --- |
| Hover colour changes on rows and buttons | 150ms (rows: `transition-colors`, Tailwind default 150ms ease) |
| **Message row hover** | **instant, no transition**, on purpose ("it fires constantly while scrolling") |
| Button / pressable press | scale to 0.97, 150ms ease-out; quick-reaction emoji buttons scale to 0.9 |
| Menu, popover, select open | fade from 0 + scale from 0.95, 150ms ease-out, origin at the trigger |
| Menu, popover close | fade to 0 + scale to 0.95, 100ms |
| Tooltip | first open after 400ms delay: fade + scale from 0.97, 150ms; close: fade 100ms; instant when moving between neighbours |
| Dialog open / close | fade + scale from 0.96, 250ms ease-out / fade + scale to 0.98, 150ms; scrim fades with the same timings |
| Sheet open / close | slide from edge, 300ms / 200ms, `ease-drawer` |
| Category chevron | rotate to −90° when collapsed, 150ms ease-out |
| Switch thumb | 200ms ease-out |
| Jump-to-present pill | fade in + rise 4px, 150ms ease-out |
| Drop-files overlay | fade in 150ms |
| Unsaved-changes bar (settings) | slide up 16px + fade, 200ms in / 150ms out |
| First-time surfaces (`rise-in`) | fade + rise 8px, 300ms ease-out, once |
| Jumped-to message (`flash`) | row background `primary` @ 14% held for 35% of 1.6s, then fades to transparent |
| Typing dots | each 4px dot pulses opacity 0.35 → 1 → 0.35 over 1.2s (peak at 30%), infinite, the three dots offset by 0.16s |
| Pending message | 60% opacity until confirmed; opacity transition 150ms |
| Speaking ring | appears instantly, fades out over 150ms |
| Autocomplete above the composer | no animation ("suggestions follow the keyboard") |
| Skeleton | Tailwind `animate-pulse` (2s opacity pulse) |
| Spinner | 16px, rotating (1s linear) |

Reduced motion (setting or OS): scale and translate parts are removed, fades stay ("fewer and gentler, not zero").

---

## 10. Chat-surface anatomy

All values are for the modern preset at default density. Class quotes are from the source; px and colours are resolved. "fg" = `foreground`.

### 10.1 App shell

`components/shell/app-shell.tsx`. Full-height row: sidebar (316px fixed: rail 68px + conversation column 248px, the column a flex column with the list on top and the user card at the bottom) and the floating main panel (section 4). The main panel is a column: optional banners, then the current view (chat, friends, voice, empty state).

### 10.2 Server rail

`components/shell/server-rail.tsx`. (For a messenger without servers this is the place for top-level destinations: chats, calls, status, settings.)

| Property | Value |
| --- | --- |
| Container | 68px wide, full height, fill `rail`, column, items centred, gap 10px, padding 16px top / 12px bottom, scrolls vertically with the scrollbar hidden |
| Tile | 40 × 40 button, radius 14px. Content fills it and inherits the radius (server icons keep their own 10px inline radius). |
| Tile, selected | 2px ring in `primary`, separated from the tile by a 2px gap in the `rail` colour (`ring-2 ring-primary ring-offset-2 ring-offset-rail`) |
| Tile, hover (unselected) | 2px ring `fg` @ 15%, same 2px gap |
| Tile, press | scale 0.97; transitions (ring, opacity, transform) 150ms ease-out |
| Home / direct messages tile | fill `primary`, 20px `MessagesSquare` glyph in `primary-foreground` |
| Server tile | image, or initials on a hue tint (2.8) |
| Unread dot | 10px circle in `primary` at the tile's top-right, 2px outside the corner, with a 3px ring in `rail` (a cut-out look). Shown when unread, not selected, no mentions. |
| Mention count | bottom-right, 4px outside the corner: 18px high, min 18px wide, pill, fill `destructive`, white 10px 600 text, 4px horizontal padding, 3px ring in `rail` |
| Divider | 28 × 1px, `sidebar-border`, rounded ends, between the home tile group and servers |
| Add tile | fill `sidebar-accent`, 20px `Plus` in `muted-foreground`, turns `primary` on hover |
| Tooltip | every tile, on the right |

### 10.3 Conversation column: header and search

`components/shell/server-sidebar.tsx`, `dm-sidebar.tsx`.

- Header band: 48px high, 8px top margin (inset), 8px horizontal padding, no bottom border in inset.
- Server header button (server view): 40px high, full width, radius 10px, 8px horizontal padding, gap 10px: 24px server icon, name 14px 600 (truncates), 14px `ChevronsUpDown` in `muted-foreground`. Hover and open: fill `sidebar-accent`. Opens a dropdown as wide as the button (min 224px), 6px below.
- Search / jump button (below the header in a server with 8px top padding; *inside* the header band in the DM view): 32px high, full width, radius 10px, fill `sidebar-accent`, 10px horizontal padding, gap 8px: 14px `Search`, label 12px `muted-foreground` ("Search" / "Find or start a conversation"), `Ctrl K` keycap (16px high, 10px text). Hover: fill `fg` @ 9%, text `sidebar-accent-foreground`. It opens the quick switcher, it is not an inline text field.

### 10.4 Conversation column: rows

Channel row (`components/shell/channel-row.tsx:241-247`):

`group/row flex h-8 w-full min-w-0 items-center gap-2 rounded-md px-2 text-left text-sm text-sidebar-foreground/70 ... transition-colors`

| State | Appearance |
| --- | --- |
| Rest | 32px high, radius 8px, 8px horizontal padding, gap 8px; 16px leading icon in `muted-foreground`; label 14px in `sidebar-foreground` @ 70% |
| Hover | fill `sidebar-accent` @ 45%, label `sidebar-accent-foreground`; trailing action icons appear (20px hit boxes, 14px glyphs, `sidebar-foreground` @ 60% → 100% on hover) |
| Selected | fill **`fg` @ 9%**, label 500 in `sidebar-accent-foreground`, **leading icon `primary`**; action icons always visible |
| Unread | label 500 in full `sidebar-foreground`; a 6px dot in `sidebar-foreground` at the right (hidden on hover) |
| Mentions | count pill at the right: 16px high, min 16px, pill, `destructive`, white 10px 600 |
| Muted | whole row 50% opacity |
| Keyboard focus | 2px ring `sidebar-ring` |

"Hover is a lighter step than the selected row, so the two never look alike."

Section header (category): 32px band with 12px top padding; inside it a 24px-high button: 12px chevron (rotates when collapsed) + 12px 500 label in `sidebar-foreground` @ 60%; a 20px `+` button appears at the right on hover.

Direct-message row (`components/shell/dm-sidebar.tsx:46-84`), the closest thing to a messenger's chat row:

| Property | Value |
| --- | --- |
| Box | 40px high, radius 8px, 8px horizontal padding, gap 10px, 2px between rows |
| Avatar | 28px with a status dot (10px dot, cut out of the avatar) |
| Text | name 14px (500 if unread or selected), optional subtitle 11px `muted-foreground`; line height 1.25 |
| Right side | mention count pill (hidden on hover), replaced on hover by a 20px close button with a 14px `X` |
| States | same fills as channel rows: hover `sidebar-accent` @ 45%, selected `fg` @ 9% |

The list scrolls with a thin scrollbar, 8px horizontal padding, 8px vertical padding. Above the rows: a "Friends" nav row (32px, same states, 16px `Users` icon that turns `primary` when selected) and a 32px section label "Direct messages" (12px 500, `sidebar-foreground` @ 60%, 12px top margin) with a 20px `+` button.

### 10.5 User panel (bottom of the column)

`components/shell/app-shell.tsx:124-127`, `user-panel.tsx`.

`modern:m-2 modern:mt-1 modern:overflow-hidden modern:rounded-xl modern:border-t-0 modern:bg-foreground/[0.045] modern:ring-1 modern:ring-inset modern:ring-foreground/[0.05]`

- A card floating at the bottom of the column: margin 8px (4px on top), radius 14px, fill `fg` @ 4.5%, 1px inset ring `fg` @ 5%, clipped. A voice-connection strip, when present, sits in the same card above the user row, separated by a 1px line in `fg` @ 6%.
- User row: 52px high, 6px horizontal padding, gap 4px.
- Profile button (fills the remaining width): 4px padding, radius 10px, gap 8px, 30px avatar with status dot, name 14px 500, status line 11px `muted-foreground`. Hover and open: fill `fg` @ 6%. Opens the profile card as a popover above (320px wide, 10px offset).
- Buttons at the right: mute (28 × 32 with a 16 × 32 chevron button attached for the device menu), deafen (same), settings (32 × 32). Ghost style, 16px icons in `muted-foreground`, hover text `fg` and fill `fg` @ 7%. Active (muted/deafened): icon `destructive`, fill `destructive` @ 12%, hover @ 18%.

### 10.6 Chat header

`components/chat/channel-header.tsx:185`.

`flex h-12 shrink-0 items-center gap-2 border-b px-3 modern:border-border/70 modern:md:px-4`

| Property | Value |
| --- | --- |
| Box | 48px high (including the 1px bottom border in `border` @ 70%), 16px horizontal padding, gap 8px |
| Leading (channel) | 28 × 28 chip, radius 10px, fill `primary` @ 10%, 16px icon in `primary` |
| Leading (DM / group) | 26px avatar; a DM shows the status dot |
| Title | 15px 600, letter-spacing −0.025em, `foreground`, does not shrink |
| After the title | DM: presence word in 12px `muted-foreground`. Channel with a topic: a 16px-high vertical separator with 4px side margins, then the topic in 12px `muted-foreground` (truncates, hover `foreground`, opens a dialog) |
| Right group | gap 2px. Ghost `icon-sm` buttons (32 × 32, radius 8px), 16px icons, `muted-foreground` → `foreground` on hover, hover fill `fg` @ 7% dark / `accent` light. In a DM: call, video call, notifications, pins, profile toggle, search field, inbox. |
| Active button | fill `primary` @ 10%, icon `primary`; hover `primary` @ 15% |
| Notification dot on a button | 6px `destructive` circle, 6px from the top and right of the button |
| Search field | InputGroup 32px high, 208px wide, filled style (3.2), 16px `Search` icon, 12px text, clear button (24px) when not empty; Enter opens the search panel |
| Tooltips | below each button |

### 10.7 Message list

`components/chat/message-list.tsx`.

- Scroll container fills the space between header and composer, thin scrollbar, content bottom-aligned (short conversations sit at the bottom). Bottom padding 20px.
- Messages from the same author within 7 minutes group together; a reply, a system message, a new day or the "new" line starts a new group.
- **Date separator**: 16px horizontal margin, 12px vertical margin; a 1px `border` line each side of a centred label (gap 8px), label 11px 500 `muted-foreground` ("Today", "Yesterday", or a long date).
- **Unread ("new") separator**: 16px horizontal margin, 4px vertical margin; a 1px line in `destructive` @ 60% filling the width, then the word "New" at the right end (10px 600 `destructive`, gap 8px). Placed where you had read up to when you opened the chat; it stays put while you read.
- **Scroll-to-bottom ("Jump to present")**: appears when more than 1600px from the bottom. A secondary `xs` button (28px high, 12px text + 16px `ArrowDown`), pill-shaped, 1px `border`, `shadow-popover`, centred horizontally, 12px above the bottom of the list. Enters with fade + 4px rise, 150ms.
- **Loading skeleton**: five rows, 16px padding, 20px apart: a 32px rounded square (radius 8px) and two 12px bars (first 112px wide, second 40 to 90% of the width), gap 12px.
- **Welcome block** (top of history): padding 40px top, 16px sides and bottom. DM: 56px avatar, name 20px 600 tight, `@username` 14px `muted-foreground`, a sentence in 14px `muted-foreground`, an optional outline `xs` button. Channel: the 56px tinted tile (3.9), heading 20px 600, sentence 14px `muted-foreground`.
- **Drop-files overlay**: 8px inside the chat column, radius 14px, 2px dashed border `primary` @ 60%, fill `background` @ 85% with 2px blur; centred 24px `Upload` icon in `primary`, 14px 500 title, 12px `muted-foreground` hint.

### 10.8 Message row

`components/chat/message-item.tsx:540-617`. The layout is a flat, full-width row (no bubbles):

```
|<- 16 ->[avatar 32]<- 12 ->| Author name  Today at 19:53            |<- 48 right padding ->|
|<------- 60px gutter ------>| Message text, 14px / 1.45, fg @ 90%     |
|          19:54 (on hover)  | A continued message from the same author|
```

| Property | Value |
| --- | --- |
| Row | full width, right padding 48px, content starts at a **60px left gutter** (`pl-[60px]`) |
| Group start | 12px top margin, 2px top padding. Continued messages: 1px top and bottom padding. |
| Avatar | 32px, rounded square (radius 8px), absolutely placed 16px from the left, 2px from the top of the row; a button (press scale 0.97) that opens the profile popover to the right |
| Header line | baseline-aligned, gap 8px, line height 20px: author name (14px 600, underline on hover, opens the popover, has its own context menu) then timestamp (11px `muted-foreground`, tooltip with the full date). A 12px crown in `idle` marks the server owner; a bot tag may follow. |
| Body | 14px, line height 1.45, `fg` @ 90%; "(edited)" in 10px `muted-foreground` after the text, 4px gap |
| Continued-message time | on hover, the short time (10px `muted-foreground`) appears centred in the 60px gutter, 3px from the top |
| Hover | row fill `fg` @ 3%, **no transition**; the toolbar appears |
| Mentions you | row fill `mention` @ 10% plus a 2px bar in `mention` down the left edge |
| Being replied to (composer open on it) | row fill `primary` @ 5% plus a 2px bar in `primary` down the left edge |
| Jumped to | `flash` animation (section 9) |
| Sending | body at 60% opacity; failed: text `destructive`, then a 12px line with the error, "Retry" (`link` colour, 12px icon) and "Delete" |
| Blocked author | a centred 12px `muted-foreground` line between two rules with a "Show" link |
| Right-click | context menu, 224px wide (10.12) |

**Own messages are not styled differently.** The only difference is that the hover toolbar and the context menu include "Edit".

**Reply preview** (above the header line of a reply; `message-item.tsx:136-182`): one 12px `muted-foreground` line starting at the 60px gutter, 2px bottom margin: a 14px avatar, the original author's name (500, in their role colour), then the original text truncated to one line; hover turns it `foreground`; click jumps to the original. A connector "spine" joins it to the avatar below: a 24 × 8px shape at left 32px, starting at the line's vertical middle, with 1px left and top borders in `muted-foreground` @ 40% and an 8px top-left radius. If the original is gone: italic "Original message was deleted".

**System line** (joined, pinned, added to group, call started/ended): 14px `muted-foreground`, 2px vertical padding, starts at the gutter; a 14px icon centred in the 60px gutter (`ArrowRight` in `online` for joins, `Pin`, `UserPlus` in `online`, `UserMinus` in `destructive`, `Phone` in `online`, `PhoneMissed` in `destructive`); names are 500 `foreground` and clickable; the timestamp follows inline.

**Hover toolbar** (`message-item.tsx:385-446`):

`absolute right-3 z-10 flex items-center gap-px rounded-md border bg-popover p-0.5 shadow-sm modern:right-4 modern:gap-0.5 modern:rounded-lg modern:shadow-raised` + `-top-3.5 modern:-top-4`

- Floats over the top-right of the hovered row: 16px from the right, its top 16px above the row's top edge (so it overlaps the previous message). Near the top of the list it moves inside the row instead (2px from the top).
- Box: `popover` fill, 1px `border`, radius 10px, padding 2px, gap 2px, `shadow-raised`. Total height 34px.
- Contents left to right: up to three quick-reaction emoji (28px buttons, 16px emoji, radius 8px, hover fill `accent`, press scale 0.9), add reaction (`SmilePlus`), edit (`Pencil`, own messages only), reply (`CornerUpLeft`), more (`MoreHorizontal`). Icon buttons are ghost `icon-xs`: 28 × 28, 16px icons, `muted-foreground` → `foreground`.
- Stays visible while its menu or picker is open.

**Inline editor** (editing your message): replaces the body; a box with radius 8px, 1px `input` border, `background` fill, padding 12px × 8px, focus = border `ring` + 3px ring `ring` @ 50%; below it an 11px `muted-foreground` hint ("escape to cancel, enter to save") with the two words as `link`-coloured buttons.

**Compact display mode** (optional setting): no avatars; the time (10px) sits right-aligned in a 48px column at the left, the name is inline before the text, groups are 4px apart.

### 10.9 Message content

`app/globals.css:580-714`, `components/chat/markup.tsx`, `attachments.tsx`, `embeds.tsx`, `reactions.tsx`.

| Element | Spec |
| --- | --- |
| Link | `link` colour, underline on hover |
| Bold | weight 600 |
| Mention (`@user`, `@role`, `#channel`) | pill: radius 4px, 2px horizontal padding, weight 500, fill `primary` @ 16%, text = `primary` mixed 70/30 with `foreground` in OKLCH (neutral dark `#7daeed`, light `#285696`); hover fill `primary` @ 28%; colours animate over 150ms |
| Inline code | mono 0.86em, padding 0.1em × 0.35em, radius 4px, fill `code`, 1px `border` |
| Code block | margin 4px top / 2px bottom, radius 10px, 1px `border`, fill `code`, padding 0.55em × 0.75em, mono 0.84em, line height 1.5, horizontal scroll; a 28px copy button (radius 8px, `popover` fill, `shadow-raised`) appears top-right on hover |
| Block quote | a 2px rounded bar in `border` at the left, 10px gap to the text, 2px vertical margin |
| Headings | 600, line height 1.3, sizes 1.4em / 1.2em / 1.06em, margin 6px top / 2px bottom |
| Small text | 0.8em, `muted-foreground` |
| Lists | 1.25em left margin, discs / decimals |
| Spoiler | hidden: radius 4px, fill `muted-foreground`, text transparent; revealed: fill `accent` |
| Custom emoji in text | 1.375em square, aligned −0.3em |
| Emoji-only message ("jumbo") | emoji 44px (native emoji font size 38.4px, line height 44px) |
| Inline timestamp | radius 4px, fill `muted`, padding 4px × 1px |
| **Reactions row** | 4px below the content, wraps, gap 4px |
| Reaction chip | 28px high, pill, 1px border, 8px horizontal padding, gap 6px: 14px emoji + count (12px 600, tabular). Others': border transparent, fill `fg` @ 6%, text `muted-foreground`; hover border `fg` @ 15%, text `foreground`. **Yours: border `primary` @ 45%, fill `primary` @ 12%, text `primary`.** Press scale 0.97; 150ms ease-out. Tooltip lists who reacted. |
| Add-reaction chip | 32 × 28 pill, fill `fg` @ 6%, 14px `SmilePlus` in `muted-foreground`; invisible until the row is hovered |
| **Attachments** | 4px below the text, stacked with 6px gaps; several images wrap in a 480px-wide grid |
| Image | scaled to fit within 480 × 340 (never upscaled, min 40px), radius 10px, 1px `border`, `muted` fill behind while loading; click opens a viewer. Spoiler: blurred with a centred "SPOILER" chip (black @ 70%, white 11px 600, radius 8px). |
| File card | max width 384px, radius 10px, 1px `border`, `card` fill, 10px padding, gap 12px: a 36px tile (radius 8px, `muted` fill, 16px file icon), name 14px 500 (underline on hover) over size 12px `muted-foreground` @ 70%, and a 32px download button (radius 8px, hover `accent`) |
| Audio card | max width 384px, radius 10px, bordered `card`, 12px padding: header (16px icon, name in `link`, size) over a 36px-high player |
| Upload in progress | card like a file card: "Uploading N files", percentage, and a 4px progress bar |
| Link embed | max width 420px, radius 10px, 1px `border` with a 2px left edge, `card` fill, 12px padding: site name 12px `muted-foreground`, title 14px 500 in `link`, description 14px `fg` @ 80% (max 4 lines), optional image (radius 8px; thumbnail 64px) |

### 10.10 Composer

`components/chat/composer.tsx:627-912`.

```
| 16px |  +------------------------------------------------------------+  | 16px |
|      |  | [reply bar 32px]                                            |  |      |
|      |  | [attachment tiles]                                          |  |      |
|      |  | [+]  Message #channel                    [GIF] [smile] [>]  |  |      |
|      |  +------------------------------------------------------------+  |      |
|      |   typing indicator (11px)                    counters             |  20px bottom band
```

| Property | Value |
| --- | --- |
| Wrapper | 16px horizontal padding, 20px bottom padding, no top padding (the list's own 20px bottom padding makes the gap) |
| Card | `overflow-hidden rounded-lg border ... modern:rounded-xl modern:border-foreground/[0.08] modern:dark:bg-card modern:shadow-raised`: radius 14px, 1px border `fg` @ 8%, fill `card`, `shadow-raised` |
| Focus (any child focused) | border `ring` @ 45% and a 3px ring `ring` @ 10%; border and shadow animate over 150ms |
| Input row | padding 6px, gap 4px, children bottom-aligned (buttons stay at the bottom as the text grows) |
| Attach button | ghost `icon-sm` 32 × 32, 16px `Plus`, `muted-foreground` → `foreground` |
| Text area | grows with content up to 14 lines, then scrolls; min height 32px; padding 4px horizontal, 7px vertical; chat font (14px / 1.45); no border or fill of its own; placeholder "Message #name" / "Message @name" in `muted-foreground` |
| Right buttons | gap 2px: GIF, emoji (`Smile`), each ghost `icon-sm` |
| Send button | `icon-sm` 32 × 32 with 16px `SendHorizontal`. With content: primary variant (fill `primary`, top highlight). On desktop widths it is hidden unless the "show send button" setting is on; Enter sends, Shift+Enter makes a new line. Disabled (50% opacity) when empty or too long. |
| Reply bar | first child of the card, 32px high, bottom border, 12px horizontal padding, gap 8px, 12px `muted-foreground`: "Replying to **Name**" (name 500 `foreground`), an "@ On/Off" toggle (12px `AtSign`; `primary` when on), and a 20px close button with a 14px `X` |
| Attachment tray | below the reply bar, bottom border, 8px padding, horizontal scroll, gap 8px. Tile: 144px wide, radius 10px, 1px `border`, `background` fill, 6px padding; preview 96px high (radius 8px, `muted` fill, image covers, or a 28px file icon); name 12px; size 10px `muted-foreground`; a small two-button control (spoiler, remove; 24px each, 14px icons) at the tile's top-right, `popover` fill, bordered, `shadow-raised`. |
| Status band | absolutely placed in the wrapper's 20px bottom padding, 20px from each side, 11px `muted-foreground`: typing indicator at the left, slow-mode and remaining-characters counters at the right (the counter turns `destructive` 500 when over the limit) |
| Read-only state | instead of the card: a 44px bar, radius 14px, fill `fg` @ 4%, 14px horizontal padding, 16px `EyeOff` + 14px `muted-foreground` text |

**Typing indicator** (`components/chat/typing-indicator.tsx`): three 4px dots in `muted-foreground`, 3px apart, pulsing (section 9); then, 6px to the right, "**Name** is typing…" with names in 500 `foreground`; truncates. Up to three names, then "Several people are typing".

**Autocomplete** (mentions, channels, emoji, slash commands): a panel directly above the composer card, same left and right edges as the card, 6px gap; radius 14px, 1px `border`, `popover` fill, `shadow-popover`. Header: 12px 500 `muted-foreground`, padding 12px sides, 8px top, 4px bottom. Rows: 32px high, radius 8px, 8px horizontal padding, gap 8px, 14px; selected row fill `accent`, others text `fg` @ 80%. User row: 20px avatar with status, name 500, `@username` right-aligned 12px `muted-foreground`. List max height 288px, 4px padding. Appears and disappears instantly.

### 10.11 Right-hand panels

Member list (`components/chat/member-list.tsx`), inside the main panel at the right:

| Property | Value |
| --- | --- |
| Box | 256px wide, 1px left border `border` @ 70%, 8px horizontal padding, 16px bottom padding, thin scrollbar; shown at window widths of 1024px and up, otherwise a 288px sheet from the right |
| Section header | 12px 600 `muted-foreground`, padding 8px sides, 16px top, 6px bottom; optional 8px role-colour dot; count after an em dash |
| Row | min 52px, radius 10px, padding 8px × 6px, gap 12px, 2px between rows; hover fill `fg` @ 6% (150ms); offline rows at 50% opacity, 100% on hover |
| Row content | 38px avatar with a 10px status dot (online members only), name 15px 600, optional status line 12px `muted-foreground`, optional 20px activity icon at the right |

DM profile panel (`components/profile/dm-profile-panel.tsx`): 288px wide, left border `border` @ 60%, shown from 1280px; contains the user's profile card. Toggled by the header's profile button.

Pins / search / inbox panels (`components/chat/side-panels.tsx`): 384px wide, left border `border` @ 70%. Header 48px, 16px padding, gap 8px: 28px icon chip (radius 10px, `primary` @ 10%, 16px icon `primary`), title 14px 600, a 28px close button. Body 12px padding. Result card: radius 14px, 1px `border`, `card` fill, 12px padding, 8px below; hover border `fg` @ 15%; inside: 36px avatar, name 14px 500, time 11px, text at `fg` @ 80%; a "Jump" outline `xs` button appears top-right on hover. Below 1024px these panels overlay the chat with `shadow-dialog`.

### 10.12 Context menus

Style per 3.4. Width 224px. The message menu (`components/chat/message-menu.tsx`), in order, each with a 16px leading icon:

1. Add reaction ▸ (submenu, 208px: six recent emoji at 16px, a separator, "More…")
2. Edit (own messages)
3. Reply
4. Pin / Unpin
5. separator
6. Copy text
7. Mark unread
8. Copy message link
9. Remove embeds (when applicable)
10. separator, then **Delete** (destructive)
11. separator, then Copy ID (developer mode)

The same item list renders as a dropdown from the toolbar's "more" button. Channel, DM, server and user rows have their own context menus in the same style; the notifications menu uses radio items and a "Mute ▸" submenu with durations.

### 10.13 Emoji picker

`components/chat/emoji-picker.tsx`. Opens in a popover (radius 14px, no padding).

| Property | Value |
| --- | --- |
| Size | 352 × 380px (shrinks to fit the window, keeps 12px from the edges) |
| Top bar | 8px padding, bottom border, gap 6px: a 32px search field (bordered classic style here: 1px `input`, radius 8px, 14px `Search` at the right) and a 32px skin-tone button |
| Category strip | 40px wide column at the left, right border, fill `muted` @ 30%, 6px vertical padding, 2px gap; 32px buttons (radius 8px, 16px icons, `muted-foreground`, hover `accent`) |
| Grid | 6px horizontal padding; cells 36 × 36; native emoji drawn at 22px, custom emoji 28px |
| Section header | sticky, 12px 500 `muted-foreground`, `popover` @ 95% with a slight blur |
| Footer | 40px, top border, 12px padding: preview of the hovered emoji and its `:name:` (12px 500) |

### 10.14 Avatars and presence

`components/app/avatar.tsx`.

- Sizes on the chat surface: 14 (reply preview), 20 (autocomplete, voice users), 26 (DM header), 28 (DM row), 30 (user panel), 32 (message), 36 (pinned/search cards), 38 (member row), 56 (welcome block).
- Default shape: rounded square, radius `max(4, round(size × 0.24))` (see section 5). Fallback: initials on a hue tint (2.8).
- **Status dot**: drawn at the bottom-right, *in a hole cut out of the avatar* (the avatar is masked, so the surface behind shows through the gap; no ring in a guessed colour). Geometry by avatar size:

| Avatar size | Dot diameter | Gap around the dot | Dot overhang past the corner |
| --- | --- | --- | --- |
| up to 24px | 8px | 2px | 2px |
| 25 to 40px | 10px | 2px | 2px |
| 41 to 64px | 12px | 2.5px | 2px |
| 65 to 95px | 16px | 3px | −2px (inset) |
| 96px and up | 24px | 5px | −4px (inset) |

  The dot's right and bottom edges sit `overhang` px outside the avatar box; the cut-out is a circle of radius `diameter / 2 + gap` centred on the dot.
- Dot colours: `online`, `idle`, `dnd`, `offline`, streaming `#9867e1`. Plain dots in lists are 8px.
- Speaking ring (voice): an outline following the avatar's shape, 2px stroke with a 2px transparent gap (3px / 3px for avatars of 64px and up), in `primary` (modern), cut around the status dot.

### 10.15 Badges and small indicators

| Indicator | Spec |
| --- | --- |
| Mention / unread count | 16px high, min 16px wide, pill, `destructive` fill, white 10px 600, 4px horizontal padding; "99+" cap. On the rail: 18px with a 3px `rail`-coloured ring. |
| Unread dot in a row | 6px circle, `sidebar-foreground` |
| Notification dot on a header button or tab | 6px circle, `destructive` |
| Bot tag | 16px high, radius 4px, `primary` fill, `primary-foreground` 9px 700, 4px padding |
| "LIVE" tag | radius 6px, `destructive` fill, white 9px 600, 4px padding, 16px line height |
| Connection pill | centred 8px from the top of the main panel: pill, 1px `border`, `popover` fill, padding 12px × 4px, 12px 500, `shadow-popover`, a 14px spinner or `WifiOff` in `idle`. Appears only after 2.5s of trouble. |

### 10.16 Modals, settings, quick switcher

- Dialogs: section 3.7.
- **Settings window** (`components/settings/settings-layout.tsx`): a dialog 960px wide (max) × 720px high (max), radius 18px, no padding, no gap, two panes. Left nav: 224px, fill `sidebar`, 10px padding; group title 12px 500 `muted-foreground` (padding 8px sides, 6px top, 4px bottom), groups 12px apart; nav row 32px, radius 8px, 8px padding, 14px, `sidebar-foreground` @ 75%, same hover/selected fills as sidebar rows, danger rows in `destructive`. Right pane: fill `background`; header 48px with a bottom border `border` @ 70%, 16px padding, title 14px 500, 32px close button; content column max 672px, padding 20px sides, 24px top, 112px bottom. Section heading 18px 600 tight with a 14px `muted-foreground` description (24px below); sections separated by a rule with 32px margins. A switch row: label 14px 500, description 14px `muted-foreground`, switch at the right, 12px vertical padding, 24px gap.
- Unsaved-changes bar: floats 16px above the bottom of the content, max 576px wide, radius 14px, 1px `border`, `popover` fill, `shadow-popover`, padding 8px (16px left): text 14px, a ghost "Reset" and a primary "Save changes" (`sm`).
- **Quick switcher** (Ctrl K, `components/app/quick-switcher.tsx`): scrim black @ 40%; panel 16% of the window height from the top, centred, max 512px wide, radius 14px, 1px `border`, `popover` fill, `shadow-dialog`. Search row 44px with a bottom border and a 16px `Search` icon at 50% opacity, 12px padding. Result row 36px, radius 8px, gap 10px, 14px; selected fill `accent`; right-aligned 12px `muted-foreground` secondary text. Footer: 11px `muted-foreground` key hints, top border, padding 12px × 8px. List max height 50% of the window.
- **Friends / home view** (`components/shell/friends-view.tsx`): header 48px, bottom border `border` @ 60%, 12px padding: 16px `Users` icon + title 14px 600, a 16px vertical separator, a 32px-high tabs list with 12px triggers, and an `xs` primary "Add friend" button. List rows: 8px padding, hover `accent` @ 50%.

### 10.17 Empty states

Centred column, 24px gap, max text width 384px: optional icon tile (44px, radius 14px, `primary` @ 10%, 20px icon `primary`, inset ring `primary` @ 15%), title, description in `muted-foreground`, optional outline `sm` button.

---

## 11. Window chrome (desktop app)

Source: `desktop/src/main-window.cjs`, `desktop/src/config.cjs`, `desktop/ui/titlebar.html|css|js`, `desktop/ui/base.css`, `desktop/src/preload-app.cjs:77-125`. (`C:\Projects\diskurt\electron\` exists but is empty; the Electron app lives in `desktop\`.) Electron 44, Windows build via electron-builder (NSIS).

| Property | Value |
| --- | --- |
| Frame | **Frameless on Windows** (`frame: false`). No native title bar and **no `titleBarOverlay`**: the app draws its own caption buttons in HTML. (macOS uses `titleBarStyle: 'hidden'` with traffic lights at x 12, y 10.) |
| Structure | The window's own page is only the title bar; the web app runs in a separate view placed below it, so app dialogs and scrims never cover the title bar. |
| Title bar height | **32px**; hidden in fullscreen |
| Title bar colour | The web app's **`rail`** colour ("the title bar wears the server rail's colour, like Discord's"), re-reported whenever mode, palette or style changes. Text and glyphs use the app's `muted-foreground`; hover uses `foreground`. Neutral dark: bar `#020304`, muted `#8a8f98`, foreground `#edeef0`. |
| Before the app reports colours | `background #08090c`, `foreground #e9ebf0`, `muted #8a90a0`, dark |
| Left side | 12px left padding; 16px logo (radius 4px), 7px gap, "Diskurt" 12px 600, letter-spacing 0.01em, in the muted colour |
| Drag region | the whole bar except buttons; double-click toggles maximise |
| Caption buttons | three, right-aligned, each **46px wide × 32px high** (full bar height), no radius: minimise, maximise/restore, close. Glyphs are 10 × 10 SVGs with 1px strokes: a horizontal line; a 9px square with 1px corner radius (restore: two offset 7px squares); an X. |
| Caption button states | rest: glyph in muted colour, no fill. Hover: fill `foreground` @ 10%, glyph `foreground`. Pressed: fill `foreground` @ 16%. **Close** hover: fill `#ed4245`, glyph white; pressed `#c53236`. Transition 120ms ease on background and colour. |
| Update button | when an update is ready: a 28 × 24 button (radius 6px) before the caption buttons with a 16px download arrow in `#3fb950`; hover fill `rgba(63,185,80,0.14)`; press scale 0.94 |
| Title bar font | `'Segoe UI Variable Text', 'Segoe UI', system-ui`, 14px base (not Figtree: the bar is a separate local page) |
| Default window size | 1280 × 800; restores the last position, size and maximised state |
| **Minimum window size** | **940 × 500** |
| Window background colour | the reported theme background (the `rail` colour), so resizing never flashes white |
| Splash window | 320 × 380, frameless, not resizable |
| Taskbar | overlay badge icons for 1 to 9 and "9+" unread mentions |

The desktop shell's own pages (splash, screen picker) use a fixed neutral dark set: bg `#0f1115`, surface `#171a20`, raised `#1e222a`, fg `#e9ebf0`, muted `#8a90a0`, line white @ 8%, primary `#3a86e2`, primary hover `#4b92ea`.

Stacking in the desktop app with the modern/inset design, top to bottom: 32px title bar in `rail`; then the app: 68px rail in `rail` and, beside it, everything else on `sidebar` with the main panel floating 8px inside.

---

## 12. Which look the owner actually uses

**Conclusion.** Dark mode: high confidence. Palette: neutral (graphite with the blue accent) is the best-supported answer, medium confidence. The modern *design* is what the owner asked for; nothing in the repository records the owner's own account settings, so the palette the owner personally pairs with modern is **not proven**. If one alternative palette is offered, make it turquoise (see evidence 6).

Evidence:

1. **Defaults.** `DEFAULT_PREFS.theme = 'dark'` (`lib/prefs.ts:32`), `DEFAULT_USER_SETTINGS.theme = 'dark'`, `DEFAULT_PALETTE = 'neutral'`. The toaster also assumes dark until told otherwise. Note the default *design* is classic; modern is opt-in.
2. **Desktop app.** The built-in theme before the web app reports colours is dark (`#08090c`); the shell's own pages are `color-scheme: dark` with the neutral blue `#3a86e2`. Snapshots in `desktop\dist\snaps\` (`titlebar-2x.png`, `picker.png`, splash images) are all dark: a near-black title bar with grey "Diskurt" wordmark and caption buttons; a dark screen-share picker with rounded corners.
3. **Promotional images** in `data\announcement-work\` (four PNGs, 12 Oct 2026), made for announcing the Windows app. They embed a real screenshot of the chat shell: dark mode, near-black graphite surfaces, **blue** accent (home tile, links, mentions), i.e. the neutral palette. That screenshot shows the **classic** design with the flat layout (bordered search field, bare `#` in the header, bordered welcome tile, no floating panel); the message in it is dated 3 Oct 2026, one day before the modern design was written, so it says nothing about modern either way.
4. **`.shots\`** holds 748 PNGs; nearly all are of "Meydan" (the 3D social space, character creator, mini-games), not the chat. The ones viewed: `settings-dialog-1.png`, `settings-dialog-2.png`, `final-desktop-1.png`, `first-visit.png` (character creator: dark translucent side panel over a warm backdrop, white pill buttons), `settings-card-1.png` (a dev page: near-black background, a bordered dark card, a blue "Düzenle" button matching neutral dark `primary`, Geist-like type, i.e. dark + neutral + classic defaults), `final-menu-1.png` (a dark dialog over the 3D scene), `test.png` and `issue.png` (dev pages). All dark; where an accent is visible it is the neutral blue. None shows the chat shell, and none shows the modern design. (`.shots\profile\` is a browser profile directory, not screenshots; it was not opened.)
5. **QA harness** `data\qa\music-menu\build-music.cjs` renders test pages with `<html data-theme="dark" data-style="modern">`; its `submenu.png` shows dark menus with the modern 14px corners. So modern has been exercised in dark.
6. **History.** Commit `a407ee1` (4 Oct 2026) introduced what is now "modern" as a single look: "turquoise on slate, inset panel, refined components … 10px radius, Figtree for UI text, finer lucide strokes". Commit `d1f1f83` the same day made the original look the default again and split the redesign into independent palette and design layers. So the modern design was *designed* against the turquoise palette, while the product default and the brand assets ("Brand icons and the fatal-error page follow the neutral blue") are neutral.
7. **No stored settings.** User settings live in PostgreSQL, not in the repo. `data\diskurt.db` is a leftover SQLite file last written on 3 Oct 2026, before the appearance system existed; it was not opened. The README and `docs\` (Meydan design notes only) do not mention a preferred theme. Tests do not cover appearance.

Recommended default for the re-implementation: **dark mode + neutral palette + modern preset**, with light mode and the turquoise palette as the first additions.

---

## 13. Essence: what makes it recognisable

If only these are right, it will read as diskurt's modern design:

1. **The floating main panel.** The app background is the sidebar colour; the chat lives in a card with an even 8px gap on all four sides, a 14px radius, a 1px `foreground` @ 10% hairline and almost no shadow. The sidebar has no border and no background of its own beside it.
2. **Near-black, low-contrast layering.** Dark surfaces step up in tiny lightness increments (rail `#020304`, sidebar `#040507`, panel `#07080b`, card `#0c0e12`, popover `#0e1115`) with a faint cool tint (hue 262). Lines are white at 6 to 7.5% alpha. Nothing is mid-grey.
3. **One accent, used sparingly.** Blue `#4092e7` (dark) for the primary button, the selected rail ring, the selected row's icon, focus, own reactions, mentions and links. Everything else is neutral. Red (`destructive`) is only for counts, the "new" line and danger.
4. **Translucent foreground fills instead of borders.** Fields, chips, hover and selected states are `foreground` at 3 to 12% alpha: fields 4.5% (6.5% hover), message hover 3%, chips 6%, selected sidebar row 9%. Fields have no visible border at rest.
5. **Focus as a soft glow.** A focused field turns transparent, gains a 1px accent edge (`ring` @ 70%) and a 3px halo (`ring` @ 20%). The composer does the same more quietly (45% / 10%).
6. **Figtree, 14px, medium weights.** Body 14px, secondary text 12px and 11px `muted-foreground`, weight 500 for emphasis and 600 for names and titles, nothing heavier, nothing larger than 20px. Section labels are sentence case at 12px, never small caps.
7. **Lucide icons at 1.75 stroke, 16px**, in `muted-foreground` at rest and `foreground` on hover.
8. **Generous, consistent rounding.** 10px controls, 14px floating layers and the composer, 18px dialogs, 8px rows and menu items, pills for reactions and badges, rounded-square avatars (radius 24% of the size).
9. **Soft, deep shadows only on floating layers**, each with a 1px inner top highlight in dark mode (`rgba(255,255,255,0.045)`); in-flow cards have none. **Menus are roomy:** 14px corners, 6px padding, 32px items with 8px corners, edge-to-edge separators, small muted labels; they pop in at 95% scale over 150ms and leave in 100ms.
10. **Flat message rows, not bubbles.** 32px avatar in a 60px gutter, name 600 + 11px muted timestamp, body at 90% foreground with 1.45 line height, grouped messages 2px apart and groups 12px apart; an instant 3% hover tint and a small floating toolbar at the top-right of the hovered row.
11. **The composer as a raised card.** 14px radius, hairline border, `card` fill and the `raised` shadow, 16px from the sides, ghost icon buttons inside, the typing indicator tucked underneath in 11px text.
12. **Tinted accent chips.** The channel icon in the header, panel header icons, the welcome tile and empty-state icons all sit on a rounded square of `primary` @ 10% with the icon in `primary`.
13. **The user card.** At the bottom of the sidebar, a rounded 14px card (foreground @ 4.5% with a 5% inner ring) holds the avatar, name, status and three quiet icon buttons.
14. **Quick, decisive motion.** 150ms with a strong ease-out (`cubic-bezier(0.23, 1, 0.32, 1)`), a 0.97 press scale on every button, dialogs settling from 96% over 250ms, and no animation at all on message hover.
15. **A 32px custom title bar in the rail colour** with flat 46px caption buttons (red close on hover), so the chrome and the sidebar are one dark frame around the floating panel.

---

## 14. Not determined / caveats

- **The owner's saved appearance settings** (mode, palette, whether any fine-tuning differs from the modern preset) are not in the repository. Section 12 is inference from defaults and artefacts.
- **No screenshot of the chat shell in the modern design exists in the repo.** Everything about modern here comes from source code, not from a rendered reference. Before polishing, it would be worth capturing the real app with the Modern preset selected and comparing.
- **Hex values for the most saturated colours** (`primary`, `destructive`, `link`, `online`, some accent palettes) are clamped conversions; a browser may render them 1 to 3 units different per channel. Greys are exact.
- **Syntax-highlight colours**: `app/globals.css` references `--hl-keyword`, `--hl-string`, `--hl-number`, `--hl-function`, but no file in the repo defines them, so highlighted code currently inherits the text colour except comments (`muted-foreground`, italic) and deletions (`destructive`).
- **Scrollbar width and shape** are left to the browser (`scrollbar-width: thin`); the 8px figure is an estimate.
- **Line heights for arbitrary font sizes** (10, 11, 13, 15px) are inherited and marked (inferred) where stated.
- **Button radius for `xs`, `sm`, `icon-xs` in modern** (10px) is inferred from CSS specificity, not stated in the source.
- Gradient themes, the voice/video views, the profile card and Profile Studio effects (nameplates, avatar decorations, animated avatars), the admin console, the mobile layout (below 768px) and the Meydan 3D space were not specified; they are outside the chat surface or outside the modern preset.
- Diskurt has **no message bubbles and no distinct own-message style**. A messenger that needs incoming/outgoing bubbles has no direct precedent in diskurt; the nearest building blocks are the `card` surface (others) and the own-reaction recipe, `primary` @ 12% fill with a `primary` @ 45% border (yours). That mapping is a suggestion, not something diskurt does.
- Diskurt's conversation row (40px, 28px avatar, one-line subtitle) carries no last-message preview or time. A messenger list that needs both has a closer model in the member row: min 52px, 38px avatar, 15px 600 name over a 12px `muted-foreground` line, 12px gap, radius 10px. Also a suggestion.
