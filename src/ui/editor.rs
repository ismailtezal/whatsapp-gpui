//! The photo editor: crop, turn, draw and write on a picture before it is sent.
//!
//! The picture is held in memory; strokes and text are kept as shapes in picture pixels
//! and shown on top of it, and are only drawn into it when the result is saved. Turning
//! and cropping change the picture itself (and move the shapes with it). Every change
//! can be undone.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ContentMask, Context, Entity, EventEmitter, FocusHandle, FontWeight, Hsla,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, PathStyle, Pixels,
    Point, RenderImage, SharedString, StrokeOptions, Window, canvas, div, point, px, size,
};
use gpui_component::input::{Input, InputState};
use image::RgbaImage;

use super::theme::{R_LG, R_MD, TITLE_BAR_H, tokens};
use super::widgets::{icon, tip};
use crate::paint::{self, LINE_HEIGHT, Label, Stroke};

/// What the editor tells the conversation.
pub enum EditorEvent {
    /// The edited picture was saved to this file.
    Done(PathBuf),
    /// Closed without changes.
    Cancel,
}

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    /// No tool: text can still be moved.
    Hand,
    Crop,
    Draw,
    Text,
}

/// The state one undo step returns to.
#[derive(Clone)]
struct Snapshot {
    picture: Arc<RgbaImage>,
    strokes: Vec<Stroke>,
    labels: Vec<Label>,
}

/// Which sides of the crop frame a drag moves; none of them means the whole frame.
#[derive(Clone, Copy, PartialEq, Default)]
struct Grip {
    left: bool,
    top: bool,
    right: bool,
    bottom: bool,
}

enum Drag {
    /// The last stroke grows.
    Stroke,
    Crop {
        grip: Grip,
        start: [f32; 4],
        from: (f32, f32),
    },
    Label {
        index: usize,
        offset: (f32, f32),
    },
}

/// Pen and text colours.
const PALETTE: [[u8; 4]; 8] = [
    [255, 255, 255, 255],
    [17, 17, 17, 255],
    [255, 59, 48, 255],
    [255, 149, 0, 255],
    [255, 204, 0, 255],
    [52, 199, 89, 255],
    [10, 132, 255, 255],
    [191, 90, 242, 255],
];
/// Pen widths and type sizes, as fractions of the picture's longer side.
const PEN: [f32; 3] = [0.004, 0.009, 0.018];
const TYPE: [f32; 3] = [0.035, 0.055, 0.085];
/// The longest side a picture is edited at; larger ones are scaled down when opened.
const MAX_SIDE: u32 = 3000;

pub struct PhotoEditor {
    source: PathBuf,
    picture: Option<Arc<RgbaImage>>,
    shown: Option<Arc<RenderImage>>,
    strokes: Vec<Stroke>,
    labels: Vec<Label>,
    history: Vec<Snapshot>,
    tool: Tool,
    color: usize,
    weight: usize,
    /// The crop frame while the crop tool is in use: left, top, right, bottom.
    crop: [f32; 4],
    square: bool,
    drag: Option<Drag>,
    /// Where the picture was last drawn in the window.
    area: Rc<Cell<Bounds<Pixels>>>,
    text: Entity<InputState>,
    font: Option<Arc<ab_glyph::FontVec>>,
    focus: FocusHandle,
    busy: bool,
    problem: Option<String>,
}

impl EventEmitter<EditorEvent> for PhotoEditor {}

/// Opens a picture upright (cameras store a sideways picture plus a note to turn it)
/// and no larger than the editor works with.
fn open_picture(path: &Path) -> anyhow::Result<RgbaImage> {
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::open(path)?.with_guessed_format()?.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut picture = image::DynamicImage::from_decoder(decoder)?;
    picture.apply_orientation(orientation);
    if picture.width().max(picture.height()) > MAX_SIDE {
        picture = picture.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    Ok(picture.to_rgba8())
}

/// The picture as the window draws it (blue and red swapped, as GPUI wants them).
fn to_shown(picture: &RgbaImage) -> Arc<RenderImage> {
    let mut copy = picture.clone();
    for pixel in copy.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(copy)]))
}

fn hsla(color: [u8; 4]) -> Hsla {
    gpui::Rgba {
        r: color[0] as f32 / 255.,
        g: color[1] as f32 / 255.,
        b: color[2] as f32 / 255.,
        a: color[3] as f32 / 255.,
    }
    .into()
}

impl PhotoEditor {
    pub fn new(source: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = cx.new(|cx| InputState::new(window, cx).placeholder("Type, then click on the photo"));
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let editor = PhotoEditor {
            source: source.clone(),
            picture: None,
            shown: None,
            strokes: Vec::new(),
            labels: Vec::new(),
            history: Vec::new(),
            tool: Tool::Hand,
            color: 2,
            weight: 1,
            crop: [0.; 4],
            square: false,
            drag: None,
            area: Rc::new(Cell::new(Bounds::default())),
            text,
            font: paint::label_font().map(Arc::new),
            focus,
            busy: true,
            problem: None,
        };
        let opening = cx.background_executor().spawn(async move {
            let picture = open_picture(&source)?;
            let shown = to_shown(&picture);
            anyhow::Ok((Arc::new(picture), shown))
        });
        cx.spawn_in(window, async move |this, cx| {
            let opened = opening.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match opened {
                    Ok((picture, shown)) => this.set_picture(picture, shown, window),
                    Err(error) => this.problem = Some(format!("This picture cannot be edited: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        editor
    }

    fn set_picture(&mut self, picture: Arc<RgbaImage>, shown: Arc<RenderImage>, window: &mut Window) {
        self.crop = [0., 0., picture.width() as f32, picture.height() as f32];
        self.picture = Some(picture);
        // Each version is its own texture; release the one it replaces.
        if let Some(old) = self.shown.replace(shown) {
            let _ = window.drop_image(old);
        }
    }

    /// Releases the picture's texture; call before dropping the editor.
    pub fn release(&mut self, window: &mut Window) {
        if let Some(old) = self.shown.take() {
            let _ = window.drop_image(old);
        }
    }

    fn picture_size(&self) -> (f32, f32) {
        self.picture
            .as_ref()
            .map(|picture| (picture.width() as f32, picture.height() as f32))
            .unwrap_or((1., 1.))
    }

    /// Screen pixels per picture pixel, as last drawn.
    fn scale(&self) -> f32 {
        let wide = self.area.get().size.width / px(1.);
        (wide / self.picture_size().0).max(0.0001)
    }

    /// A window position as a point in the picture.
    fn in_picture(&self, position: Point<Pixels>) -> (f32, f32) {
        let area = self.area.get();
        let scale = self.scale();
        (
            (position.x - area.origin.x) / px(1.) / scale,
            (position.y - area.origin.y) / px(1.) / scale,
        )
    }

    fn remember(&mut self) {
        if let Some(picture) = self.picture.clone() {
            self.history.push(Snapshot {
                picture,
                strokes: self.strokes.clone(),
                labels: self.labels.clone(),
            });
        }
    }

    fn changed(&self) -> bool {
        !self.history.is_empty()
    }

    fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(snapshot) = self.history.pop() else {
            return;
        };
        self.strokes = snapshot.strokes;
        self.labels = snapshot.labels;
        let same = self
            .picture
            .as_ref()
            .is_some_and(|picture| Arc::ptr_eq(picture, &snapshot.picture));
        if same {
            cx.notify();
            return;
        }
        // The picture itself was turned or cropped: bring the earlier one back.
        let picture = snapshot.picture;
        self.replace_picture(window, cx, move || picture);
    }

    /// Swaps the picture for one made off the main thread.
    fn replace_picture(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        make: impl FnOnce() -> Arc<RgbaImage> + Send + 'static,
    ) {
        self.busy = true;
        let making = cx.background_executor().spawn(async move {
            let picture = make();
            let shown = to_shown(&picture);
            (picture, shown)
        });
        cx.spawn_in(window, async move |this, cx| {
            let (picture, shown) = making.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                this.set_picture(picture, shown, window);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Turns the picture, and everything drawn on it, a quarter turn counter-clockwise.
    fn turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picture) = self.picture.clone() else {
            return;
        };
        if self.busy {
            return;
        }
        self.remember();
        let wide = picture.width() as f32;
        for stroke in &mut self.strokes {
            for point in &mut stroke.points {
                *point = paint::turn_left(*point, wide);
            }
        }
        let font = self.font.clone();
        for label in &mut self.labels {
            // Text stays upright; its middle moves with the picture.
            let (across, down) = paint::label_extent(label, font.as_deref());
            let middle = paint::turn_left((label.at.0 + across / 2., label.at.1 + down / 2.), wide);
            label.at = (middle.0 - across / 2., middle.1 - down / 2.);
        }
        self.replace_picture(window, cx, move || Arc::new(image::imageops::rotate270(&*picture)));
    }

    /// Cuts the picture down to the crop frame.
    fn apply_crop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picture) = self.picture.clone() else {
            return;
        };
        let [left, top, right, bottom] = self.crop;
        let (x, y) = (left.round().max(0.) as u32, top.round().max(0.) as u32);
        let wide = ((right - left).round() as u32).min(picture.width().saturating_sub(x));
        let tall = ((bottom - top).round() as u32).min(picture.height().saturating_sub(y));
        self.tool = Tool::Hand;
        if self.busy || wide < 2 || tall < 2 || (wide == picture.width() && tall == picture.height()) {
            cx.notify();
            return;
        }
        self.remember();
        for stroke in &mut self.strokes {
            for point in &mut stroke.points {
                *point = (point.0 - x as f32, point.1 - y as f32);
            }
        }
        for label in &mut self.labels {
            label.at = (label.at.0 - x as f32, label.at.1 - y as f32);
        }
        self.replace_picture(window, cx, move || {
            Arc::new(image::imageops::crop_imm(&*picture, x, y, wide, tall).to_image())
        });
    }

    /// Saves the result and reports it. Without changes the original file is kept.
    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        // A crop frame still open counts as wanted.
        let [left, top, right, bottom] = self.crop;
        let (wide, tall) = self.picture_size();
        let cropping = self.tool == Tool::Crop && (left > 0.5 || top > 0.5 || right < wide - 0.5 || bottom < tall - 0.5);
        let Some(picture) = self.picture.clone() else {
            cx.emit(EditorEvent::Cancel);
            return;
        };
        if !self.changed() && !cropping {
            cx.emit(EditorEvent::Cancel);
            return;
        }
        self.busy = true;
        let (strokes, labels, font) = (self.strokes.clone(), self.labels.clone(), self.font.clone());
        let keep_png = self
            .source
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("png"));
        let saving = cx.background_executor().spawn(async move {
            let mut out = (*picture).clone();
            for stroke in &strokes {
                paint::draw_stroke(&mut out, stroke);
            }
            if let Some(font) = &font {
                for label in &labels {
                    paint::draw_label(&mut out, label, font);
                }
            }
            if cropping {
                let (x, y) = (left.round().max(0.) as u32, top.round().max(0.) as u32);
                let wide = ((right - left).round() as u32).clamp(1, out.width().saturating_sub(x).max(1));
                let tall = ((bottom - top).round() as u32).clamp(1, out.height().saturating_sub(y).max(1));
                out = image::imageops::crop_imm(&out, x, y, wide, tall).to_image();
            }
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let folder = crate::paths::media_dir();
            if keep_png {
                let path = folder.join(format!("edited-{stamp}.png"));
                out.save(&path)?;
                anyhow::Ok(path)
            } else {
                let path = folder.join(format!("edited-{stamp}.jpg"));
                let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
                let rgb = image::DynamicImage::ImageRgba8(out).to_rgb8();
                image::codecs::jpeg::JpegEncoder::new_with_quality(file, 92).encode_image(&rgb)?;
                anyhow::Ok(path)
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let saved = saving.await;
            let _ = this.update_in(cx, |this, _, cx| {
                this.busy = false;
                match saved {
                    Ok(path) => cx.emit(EditorEvent::Done(path)),
                    Err(error) => this.problem = Some(format!("The picture could not be saved: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn pick_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        if self.tool == Tool::Crop && tool != Tool::Crop {
            // Leaving the crop tool keeps the frame that was set.
            self.apply_crop(window, cx);
        }
        self.tool = if self.tool == tool { Tool::Hand } else { tool };
        if self.tool == Tool::Crop {
            let (wide, tall) = self.picture_size();
            self.crop = [0., 0., wide, tall];
            self.square = false;
        }
        if self.tool == Tool::Text {
            self.text.update(cx, |input, cx| input.focus(window, cx));
        }
        cx.notify();
    }

    /// The label under a point of the picture, topmost first.
    fn label_at(&self, at: (f32, f32)) -> Option<usize> {
        self.labels.iter().enumerate().rev().find_map(|(index, label)| {
            let (wide, tall) = paint::label_extent(label, self.font.as_deref());
            let inside = at.0 >= label.at.0 && at.0 <= label.at.0 + wide && at.1 >= label.at.1 && at.1 <= label.at.1 + tall;
            inside.then_some(index)
        })
    }

    fn press(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.picture.is_none() {
            return;
        }
        let at = self.in_picture(event.position);
        let (wide, tall) = self.picture_size();
        let longer = wide.max(tall);
        let on_picture = at.0 >= 0. && at.1 >= 0. && at.0 <= wide && at.1 <= tall;
        if !on_picture && self.tool != Tool::Crop {
            return;
        }
        match self.tool {
            Tool::Draw => {
                self.remember();
                self.strokes.push(Stroke {
                    points: vec![at],
                    width: (longer * PEN[self.weight]).max(1.5),
                    color: PALETTE[self.color],
                });
                self.drag = Some(Drag::Stroke);
            }
            Tool::Text | Tool::Hand => {
                if let Some(index) = self.label_at(at) {
                    self.remember();
                    let label = &self.labels[index];
                    self.drag = Some(Drag::Label {
                        index,
                        offset: (at.0 - label.at.0, at.1 - label.at.1),
                    });
                } else if self.tool == Tool::Text {
                    let typed = self.text.read(cx).value().trim().to_string();
                    if typed.is_empty() {
                        self.text.update(cx, |input, cx| input.focus(window, cx));
                    } else {
                        self.remember();
                        let size = longer * TYPE[self.weight];
                        self.labels.push(Label {
                            // The click is the middle of the first line's left edge.
                            at: (at.0, at.1 - size * LINE_HEIGHT / 2.),
                            text: typed,
                            size,
                            color: PALETTE[self.color],
                        });
                        self.text.update(cx, |input, cx| input.set_value("", window, cx));
                    }
                }
            }
            Tool::Crop => {
                let reach = 14. / self.scale();
                let [left, top, right, bottom] = self.crop;
                let near = |a: f32, b: f32| (a - b).abs() <= reach;
                let within_x = at.0 >= left - reach && at.0 <= right + reach;
                let within_y = at.1 >= top - reach && at.1 <= bottom + reach;
                let grip = Grip {
                    left: near(at.0, left) && within_y,
                    right: near(at.0, right) && within_y,
                    top: near(at.1, top) && within_x,
                    bottom: near(at.1, bottom) && within_x,
                };
                let inside = at.0 > left && at.0 < right && at.1 > top && at.1 < bottom;
                if grip != Grip::default() || inside {
                    self.drag = Some(Drag::Crop {
                        grip,
                        start: self.crop,
                        from: at,
                    });
                }
            }
        }
        cx.notify();
    }

    fn drag_to(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.drag.is_none() {
            return;
        }
        if !event.dragging() {
            // The button was let go outside the window.
            self.drag = None;
            cx.notify();
            return;
        }
        let at = self.in_picture(event.position);
        let (wide, tall) = self.picture_size();
        let scale = self.scale();
        match self.drag.as_ref() {
            Some(Drag::Stroke) => {
                if let Some(stroke) = self.strokes.last_mut() {
                    let far_enough = stroke
                        .points
                        .last()
                        .is_none_or(|last| ((last.0 - at.0).powi(2) + (last.1 - at.1).powi(2)).sqrt() * scale >= 1.5);
                    if far_enough {
                        stroke.points.push(at);
                    }
                }
            }
            Some(Drag::Label { index, offset }) => {
                let (index, offset) = (*index, *offset);
                if let Some(label) = self.labels.get_mut(index) {
                    label.at = (at.0 - offset.0, at.1 - offset.1);
                }
            }
            Some(Drag::Crop { grip, start, from }) => {
                let (grip, [left, top, right, bottom], from) = (*grip, *start, *from);
                let (dx, dy) = (at.0 - from.0, at.1 - from.1);
                let least = (24. / scale).min(wide).min(tall);
                let mut frame = [left, top, right, bottom];
                if grip == Grip::default() {
                    // Move the whole frame, keeping it inside the picture.
                    let dx = dx.clamp(-left, wide - right);
                    let dy = dy.clamp(-top, tall - bottom);
                    frame = [left + dx, top + dy, right + dx, bottom + dy];
                } else {
                    if grip.left {
                        frame[0] = (left + dx).clamp(0., right - least);
                    }
                    if grip.right {
                        frame[2] = (right + dx).clamp(left + least, wide);
                    }
                    if grip.top {
                        frame[1] = (top + dy).clamp(0., bottom - least);
                    }
                    if grip.bottom {
                        frame[3] = (bottom + dy).clamp(top + least, tall);
                    }
                    if self.square {
                        // Keep it square: the shorter side decides, anchored at the
                        // sides that are not being dragged.
                        let side = (frame[2] - frame[0]).min(frame[3] - frame[1]);
                        if grip.left && !grip.right {
                            frame[0] = frame[2] - side;
                        } else {
                            frame[2] = frame[0] + side;
                        }
                        if grip.top && !grip.bottom {
                            frame[1] = frame[3] - side;
                        } else {
                            frame[3] = frame[1] + side;
                        }
                    }
                }
                self.crop = frame;
            }
            None => {}
        }
        cx.notify();
    }

    fn release_drag(&mut self, _: &MouseUpEvent, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            cx.notify();
        }
    }

    fn make_square(&mut self, cx: &mut Context<Self>) {
        self.square = !self.square;
        if self.square {
            let [left, top, right, bottom] = self.crop;
            let side = (right - left).min(bottom - top);
            let (middle_x, middle_y) = ((left + right) / 2., (top + bottom) / 2.);
            self.crop = [middle_x - side / 2., middle_y - side / 2., middle_x + side / 2., middle_y + side / 2.];
        }
        cx.notify();
    }

    // ---- drawing ---------------------------------------------------------------------------

    /// The strokes and the crop frame, painted over the picture.
    fn overlay(&self) -> impl IntoElement {
        let strokes = self.strokes.clone();
        let crop = (self.tool == Tool::Crop).then_some(self.crop);
        let (wide, _) = self.picture_size();
        let area = self.area.clone();
        canvas(
            move |bounds, _, _| {
                area.set(bounds);
            },
            move |bounds, _, window, _| {
                let scale = (bounds.size.width / px(1.)) / wide.max(1.);
                let place = |at: (f32, f32)| point(bounds.origin.x + px(at.0 * scale), bounds.origin.y + px(at.1 * scale));
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    for stroke in &strokes {
                        let thick = (stroke.width * scale).max(1.);
                        let color = hsla(stroke.color);
                        if stroke.points.len() < 2 {
                            if let Some(only) = stroke.points.first() {
                                let middle = place(*only);
                                let dot = Bounds::new(
                                    point(middle.x - px(thick / 2.), middle.y - px(thick / 2.)),
                                    size(px(thick), px(thick)),
                                );
                                window.paint_quad(gpui::fill(dot, color).corner_radii(px(thick / 2.)));
                            }
                            continue;
                        }
                        let options = StrokeOptions::default()
                            .with_line_width(thick)
                            .with_line_cap(lyon::path::LineCap::Round)
                            .with_line_join(lyon::path::LineJoin::Round);
                        let mut builder = PathBuilder::stroke(px(thick)).with_style(PathStyle::Stroke(options));
                        builder.move_to(place(stroke.points[0]));
                        for at in &stroke.points[1..] {
                            builder.line_to(place(*at));
                        }
                        if let Ok(path) = builder.build() {
                            window.paint_path(path, color);
                        }
                    }
                    let Some([left, top, right, bottom]) = crop else {
                        return;
                    };
                    let (from, to) = (place((left, top)), place((right, bottom)));
                    let shade = gpui::hsla(0., 0., 0., 0.6);
                    let end = point(bounds.origin.x + bounds.size.width, bounds.origin.y + bounds.size.height);
                    // What will be cut away is dimmed: above, below, left and right.
                    for (a, b) in [
                        (bounds.origin, point(end.x, from.y)),
                        (point(bounds.origin.x, to.y), end),
                        (point(bounds.origin.x, from.y), point(from.x, to.y)),
                        (point(to.x, from.y), point(end.x, to.y)),
                    ] {
                        if b.x > a.x && b.y > a.y {
                            window.paint_quad(gpui::fill(Bounds::from_corners(a, b), shade));
                        }
                    }
                    let white = gpui::white();
                    let faint = gpui::hsla(0., 0., 1., 0.35);
                    let frame = Bounds::from_corners(from, to);
                    // Thirds, to compose by.
                    for step in 1..3 {
                        let x = from.x + (to.x - from.x) * (step as f32 / 3.);
                        let y = from.y + (to.y - from.y) * (step as f32 / 3.);
                        window.paint_quad(gpui::fill(Bounds::new(point(x, from.y), size(px(1.), to.y - from.y)), faint));
                        window.paint_quad(gpui::fill(Bounds::new(point(from.x, y), size(to.x - from.x, px(1.))), faint));
                    }
                    window.paint_quad(gpui::outline(frame, white, gpui::BorderStyle::default()));
                    // Corner marks to grab.
                    let (long, thick) = (px(18.), px(3.));
                    for (corner, right_side, bottom_side) in [
                        (from, false, false),
                        (point(to.x, from.y), true, false),
                        (point(from.x, to.y), false, true),
                        (to, true, true),
                    ] {
                        let x = if right_side { corner.x - long } else { corner.x };
                        let y = if bottom_side { corner.y - long } else { corner.y };
                        let bar_y = if bottom_side { corner.y - thick } else { corner.y };
                        let bar_x = if right_side { corner.x - thick } else { corner.x };
                        window.paint_quad(gpui::fill(Bounds::new(point(x, bar_y), size(long, thick)), white));
                        window.paint_quad(gpui::fill(Bounds::new(point(bar_x, y), size(thick, long)), white));
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    fn tool_button(
        &self,
        id: &'static str,
        name: &'static str,
        label: &'static str,
        active: bool,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let t = tokens(cx);
        let hover = gpui::hsla(0., 0., 1., 0.12);
        div()
            .id(id)
            .h(px(36.))
            .px(px(10.))
            .rounded(R_MD)
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(13.))
            .text_color(gpui::white())
            .cursor_pointer()
            .when(active, |this| this.bg(t.primary).text_color(t.primary_foreground))
            .when(!active, |this| this.hover(move |style| style.bg(hover)))
            .child(icon(name))
            .child(label)
    }

    /// Colour and size choices for the pen and for text.
    fn choices(&self, cx: &mut Context<Self>) -> gpui::Div {
        let mut row = div().flex().items_center().gap(px(6.));
        for (index, color) in PALETTE.iter().enumerate() {
            let chosen = index == self.color;
            row = row.child(
                div()
                    .id(("editor-color", index))
                    .size(px(26.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .border_2()
                    .border_color(if chosen { gpui::white() } else { gpui::transparent_black() })
                    .child(
                        div()
                            .size(px(16.))
                            .rounded_full()
                            .border_1()
                            .border_color(gpui::hsla(0., 0., 1., 0.35))
                            .bg(hsla(*color)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.color = index;
                        cx.notify();
                    })),
            );
        }
        row = row.child(div().w(px(10.)));
        for (index, dot) in [6., 10., 16.].into_iter().enumerate() {
            let chosen = index == self.weight;
            row = row.child(
                div()
                    .id(("editor-weight", index))
                    .size(px(28.))
                    .rounded(R_MD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .when(chosen, |this| this.bg(gpui::hsla(0., 0., 1., 0.18)))
                    .child(div().size(px(dot)).rounded_full().bg(gpui::white()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.weight = index;
                        cx.notify();
                    })),
            );
        }
        row
    }
}

impl Render for PhotoEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx).clone();
        let view = window.viewport_size();
        let (wide, tall) = self.picture_size();
        // The room for the picture: the window less the bars and a margin.
        let room_wide = (view.width / px(1.) - 64.).max(80.);
        let room_tall = ((view.height - TITLE_BAR_H) / px(1.) - 56. - 64. - 40.).max(80.);
        let scale = (room_wide / wide).min(room_tall / tall).min(3.);
        let muted = gpui::hsla(0., 0., 1., 0.6);

        let top = div()
            .h(px(56.))
            .flex_shrink_0()
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                self.tool_button("editor-close", "x", "Cancel", false, cx)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(EditorEvent::Cancel))),
            )
            .child(div().flex_1())
            .child(
                self.tool_button("editor-crop", "crop", "Crop", self.tool == Tool::Crop, cx)
                    .on_click(cx.listener(|this, _, window, cx| this.pick_tool(Tool::Crop, window, cx))),
            )
            .child(
                self.tool_button("editor-turn", "rotate-ccw", "Turn", false, cx)
                    .on_click(cx.listener(|this, _, window, cx| this.turn(window, cx))),
            )
            .child(
                self.tool_button("editor-draw", "pencil", "Draw", self.tool == Tool::Draw, cx)
                    .on_click(cx.listener(|this, _, window, cx| this.pick_tool(Tool::Draw, window, cx))),
            )
            .child(
                self.tool_button("editor-text", "type", "Text", self.tool == Tool::Text, cx)
                    .on_click(cx.listener(|this, _, window, cx| this.pick_tool(Tool::Text, window, cx))),
            )
            .child(
                self.tool_button("editor-undo", "undo-2", "Undo", false, cx)
                    .when(!self.changed(), |this| this.opacity(0.4))
                    .tooltip(tip("Undo (Ctrl+Z)"))
                    .on_click(cx.listener(|this, _, window, cx| this.undo(window, cx))),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("editor-done")
                    .h(px(36.))
                    .px(px(16.))
                    .rounded(R_MD)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .bg(t.primary)
                    .text_color(t.primary_foreground)
                    .cursor_pointer()
                    .child(icon("check"))
                    .child("Done")
                    .on_click(cx.listener(|this, _, window, cx| this.finish(window, cx))),
            );

        let stage: AnyElement = match (&self.shown, &self.problem) {
            (_, Some(problem)) => div()
                .max_w(px(420.))
                .text_color(gpui::white())
                .child(SharedString::from(problem.clone()))
                .into_any_element(),
            (None, None) => div().text_color(muted).child("Opening…").into_any_element(),
            (Some(shown), None) => {
                let mut frame = div()
                    .id("editor-picture")
                    .relative()
                    .w(px(wide * scale))
                    .h(px(tall * scale))
                    .overflow_hidden()
                    .cursor(match self.tool {
                        Tool::Draw | Tool::Crop => gpui::CursorStyle::Crosshair,
                        Tool::Text => gpui::CursorStyle::IBeam,
                        Tool::Hand => gpui::CursorStyle::Arrow,
                    })
                    .child(
                        gpui::img(shown.clone())
                            .w(px(wide * scale))
                            .h(px(tall * scale))
                            .object_fit(gpui::ObjectFit::Fill),
                    );
                for label in &self.labels {
                    let size = label.size * scale;
                    frame = frame.child(
                        div()
                            .absolute()
                            .left(px(label.at.0 * scale))
                            .top(px(label.at.1 * scale))
                            .font_family("Segoe UI")
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(size))
                            .line_height(px(size * LINE_HEIGHT))
                            .text_color(hsla(label.color))
                            .whitespace_nowrap()
                            .child(SharedString::from(label.text.clone())),
                    );
                }
                frame.child(self.overlay()).into_any_element()
            }
        };

        let hint = |text: &'static str| div().text_size(px(13.)).text_color(muted).child(text);
        let bottom = div()
            .h(px(64.))
            .flex_shrink_0()
            .px(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(12.));
        let bottom = match self.tool {
            Tool::Hand => bottom.child(hint(if self.busy {
                "Working…"
            } else {
                "Crop, turn, draw or add text. Done puts the result back with your message."
            })),
            Tool::Draw => bottom.child(self.choices(cx)),
            Tool::Text => bottom
                .child(
                    div()
                        .w(px(300.))
                        .rounded(R_LG)
                        .bg(gpui::hsla(0., 0., 1., 0.1))
                        .text_color(gpui::white())
                        .child(Input::new(&self.text).appearance(false).bordered(false)),
                )
                .child(self.choices(cx)),
            Tool::Crop => bottom
                .child(
                    self.tool_button("editor-square", "square", "Square", self.square, cx)
                        .on_click(cx.listener(|this, _, _, cx| this.make_square(cx))),
                )
                .child(
                    self.tool_button("editor-crop-reset", "maximize-2", "Whole photo", false, cx).on_click(
                        cx.listener(|this, _, _, cx| {
                            let (wide, tall) = this.picture_size();
                            this.crop = [0., 0., wide, tall];
                            this.square = false;
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    self.tool_button("editor-crop-apply", "check", "Apply", true, cx)
                        .on_click(cx.listener(|this, _, window, cx| this.apply_crop(window, cx))),
                ),
        };

        div()
            .id("photo-editor")
            .track_focus(&self.focus)
            .size_full()
            .occlude()
            .bg(gpui::hsla(0., 0., 0.04, 0.97))
            .flex()
            .flex_col()
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| this.drag_to(event, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, event: &MouseUpEvent, _, cx| this.release_drag(event, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if key == "escape" {
                    cx.emit(EditorEvent::Cancel);
                } else if key == "z" && event.keystroke.modifiers.control {
                    this.undo(window, cx);
                }
            }))
            .child(top)
            .child(
                div()
                    .id("editor-stage")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    // Presses count on the whole stage, so a crop corner at the very edge
                    // of the picture can be grabbed from just outside it.
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::press))
                    .child(stage),
            )
            .child(bottom)
    }
}
