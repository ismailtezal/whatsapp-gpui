# Vendored GPUI

`gpui-pre` and `gpui-pre-windows` are copies of the crates.io releases at version
0.3.12 (a snapshot of Zed's `gpui` and `gpui_windows`, Apache-2.0; see the
`LICENSE-APACHE` file in each directory). `Cargo.toml` at the project root points the
build at these copies with `[patch.crates-io]`.

They are here because the app needs things upstream GPUI does not have. Every change is
marked with a `whatsapp-gpui patch` comment. Search for that phrase to list them.

## Changes

**Glass backgrounds** (the Liquid Glass style)

- `gpui-pre/src/color.rs`: `BackgroundTag::Glass`, the `Glass` settings struct and the
  `glass()` constructor; `Background::is_glass` and `scale_glass`.
- `gpui-pre/src/style.rs`, `gpui-pre/src/window.rs`: glass backgrounds are painted like
  other quads, with their lengths converted to device pixels.
- `gpui-pre-windows/src/directx_renderer.rs`: before a glass quad is drawn, the frame so
  far is copied into a mip-mapped texture (`Backdrop`) and bound to the quad shader.
- `gpui-pre-windows/src/shaders.hlsl`: `glass_fragment` treats the quad as a lens: flat in
  the middle (the copy shows through nearly unblurred), with a squircle bezel that bends
  the copy by Snell's law, parts the colours slightly, catches the light, and carries a
  hairline of light on two opposite sides. The tint thickens where the neighbourhood
  underneath (a small mip of the copy) is far from the glass's own lightness.

Glass is drawn only by the Windows DirectX renderer. This app is Windows only.

**Smooth wheel scrolling**

- `gpui-pre/src/wheel_glide.rs` (new): queues mouse-wheel distance and pays it out over
  the next frames.
- `gpui-pre/src/elements/list.rs`, `gpui-pre/src/elements/div.rs`: lists and scrolling
  elements use it for wheel input. Upstream applies each notch at once (a 60px jump).

**Middle-button auto-scroll**

- `gpui-pre/src/window.rs`: a middle click drops an anchor (`Window::auto_scroll_anchor`)
  and, each frame, a scroll sized by the pointer's distance from it is sent to whatever
  is under the anchor. Any click, key or wheel notch ends it; so does releasing the
  button after a press-and-drag. Upstream has no auto-scroll.

**Frame timing for development**

- `gpui-pre/src/window.rs`: with `WA_PERF` set, prints the average and worst time of the
  layout, prepaint, paint and present phases every 120 frames.

## Updating

To move to a newer GPUI: copy the new crate sources over these directories, then
re-apply the patches (search the old copy for `whatsapp-gpui patch`), and bump the
pinned versions in the root `Cargo.toml`.
