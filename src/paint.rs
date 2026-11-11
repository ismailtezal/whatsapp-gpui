//! Drawing onto pictures: pen strokes and text, for the photo editor's result.
//!
//! Everything is in picture pixels. Edges are anti-aliased by coverage.

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use image::RgbaImage;

/// A pen stroke: a line through `points`, `width` pixels thick, with round ends.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub points: Vec<(f32, f32)>,
    pub width: f32,
    pub color: [u8; 4],
}

/// A piece of text placed on the picture: `at` is the top-left corner of its first
/// line, `size` the height of the type in pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub at: (f32, f32),
    pub text: String,
    pub size: f32,
    pub color: [u8; 4],
}

/// Height of one line of a label, as a multiple of its type size. The editor lays the
/// preview out with the same figure, so the result matches what was seen.
pub const LINE_HEIGHT: f32 = 1.33;

/// Mixes `color` into a pixel by `coverage` (0 to 1).
fn blend(pixel: &mut image::Rgba<u8>, color: [u8; 4], coverage: f32) {
    let alpha = (color[3] as f32 / 255.) * coverage.clamp(0., 1.);
    if alpha <= 0. {
        return;
    }
    for channel in 0..3 {
        let old = pixel.0[channel] as f32;
        pixel.0[channel] = (old + (color[channel] as f32 - old) * alpha).round() as u8;
    }
    let old = pixel.0[3] as f32 / 255.;
    pixel.0[3] = ((old + (1. - old) * alpha) * 255.).round() as u8;
}

/// Distance from a point to the segment between two points.
fn distance_to_segment(point: (f32, f32), from: (f32, f32), to: (f32, f32)) -> f32 {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx * dx + dy * dy;
    let along = if length <= f32::EPSILON {
        0.
    } else {
        (((point.0 - from.0) * dx + (point.1 - from.1) * dy) / length).clamp(0., 1.)
    };
    let (x, y) = (from.0 + dx * along - point.0, from.1 + dy * along - point.1);
    (x * x + y * y).sqrt()
}

/// Draws a pen stroke. The coverage of the whole stroke is worked out first and mixed
/// in once, so places the pen crossed twice are not darker than the rest.
pub fn draw_stroke(image: &mut RgbaImage, stroke: &Stroke) {
    if stroke.points.is_empty() {
        return;
    }
    let radius = (stroke.width / 2.).max(0.5);
    let reach = radius + 1.;
    let (width, height) = (image.width() as i64, image.height() as i64);
    let bound = |pick: fn(f32, f32) -> f32, start: f32, axis: usize| {
        stroke
            .points
            .iter()
            .map(|point| if axis == 0 { point.0 } else { point.1 })
            .fold(start, pick)
    };
    let left = ((bound(f32::min, f32::MAX, 0) - reach).floor() as i64).clamp(0, width);
    let right = ((bound(f32::max, f32::MIN, 0) + reach).ceil() as i64).clamp(0, width);
    let top = ((bound(f32::min, f32::MAX, 1) - reach).floor() as i64).clamp(0, height);
    let bottom = ((bound(f32::max, f32::MIN, 1) + reach).ceil() as i64).clamp(0, height);
    if right <= left || bottom <= top {
        return;
    }
    let (mask_width, mask_height) = ((right - left) as usize, (bottom - top) as usize);
    let mut mask = vec![0f32; mask_width * mask_height];
    let mut cover = |from: (f32, f32), to: (f32, f32)| {
        let x0 = ((from.0.min(to.0) - reach).floor() as i64).clamp(left, right);
        let x1 = ((from.0.max(to.0) + reach).ceil() as i64).clamp(left, right);
        let y0 = ((from.1.min(to.1) - reach).floor() as i64).clamp(top, bottom);
        let y1 = ((from.1.max(to.1) + reach).ceil() as i64).clamp(top, bottom);
        for y in y0..y1 {
            for x in x0..x1 {
                // The centre of the pixel.
                let distance = distance_to_segment((x as f32 + 0.5, y as f32 + 0.5), from, to);
                let coverage = (radius + 0.5 - distance).clamp(0., 1.);
                let slot = &mut mask[(y - top) as usize * mask_width + (x - left) as usize];
                if coverage > *slot {
                    *slot = coverage;
                }
            }
        }
    };
    if stroke.points.len() == 1 {
        cover(stroke.points[0], stroke.points[0]);
    }
    for pair in stroke.points.windows(2) {
        cover(pair[0], pair[1]);
    }
    for y in 0..mask_height {
        for x in 0..mask_width {
            let coverage = mask[y * mask_width + x];
            if coverage > 0. {
                let pixel = image.get_pixel_mut(left as u32 + x as u32, top as u32 + y as u32);
                blend(pixel, stroke.color, coverage);
            }
        }
    }
}

/// The typeface labels are drawn with: Segoe UI Semibold, which every Windows 11 has;
/// the regular weight or Arial failing that.
pub fn label_font() -> Option<FontVec> {
    let folder = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".to_string());
    ["seguisb.ttf", "segoeui.ttf", "arial.ttf"].iter().find_map(|name| {
        let bytes = std::fs::read(std::path::Path::new(&folder).join("Fonts").join(name)).ok()?;
        FontVec::try_from_vec(bytes).ok()
    })
}

/// Draws a label. Lines break only where the text has line breaks.
pub fn draw_label(image: &mut RgbaImage, label: &Label, font: &FontVec) {
    // `size` is the em size, as in the editor's preview; the font library scales by
    // the distance from ascender to descender instead.
    let scale = match font.units_per_em() {
        Some(units) if units > 0. => PxScale::from(label.size * font.height_unscaled() / units),
        _ => PxScale::from(label.size),
    };
    let scaled = font.as_scaled(scale);
    let (width, height) = (image.width() as i32, image.height() as i32);
    for (line_index, line) in label.text.lines().enumerate() {
        let line_top = label.at.1 + line_index as f32 * label.size * LINE_HEIGHT;
        // The preview centres the type in its line box.
        let spare = label.size * LINE_HEIGHT - (scaled.ascent() - scaled.descent());
        let baseline = line_top + spare / 2. + scaled.ascent();
        let mut pen = label.at.0;
        let mut previous = None;
        for character in line.chars() {
            let id = font.glyph_id(character);
            if let Some(previous) = previous {
                pen += scaled.kern(previous, id);
            }
            let glyph = id.with_scale_and_position(scale, ab_glyph::point(pen, baseline));
            pen += scaled.h_advance(id);
            previous = Some(id);
            let Some(outline) = font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            outline.draw(|x, y, coverage| {
                let (x, y) = (bounds.min.x as i32 + x as i32, bounds.min.y as i32 + y as i32);
                if x >= 0 && y >= 0 && x < width && y < height {
                    blend(image.get_pixel_mut(x as u32, y as u32), label.color, coverage);
                }
            });
        }
    }
}

/// How wide and tall a label is, in pixels, for hit-testing it in the editor.
pub fn label_extent(label: &Label, font: Option<&FontVec>) -> (f32, f32) {
    let lines = label.text.lines().count().max(1) as f32;
    let widest = match font {
        Some(font) => {
            let scale = match font.units_per_em() {
                Some(units) if units > 0. => PxScale::from(label.size * font.height_unscaled() / units),
                _ => PxScale::from(label.size),
            };
            let scaled = font.as_scaled(scale);
            label
                .text
                .lines()
                .map(|line| line.chars().map(|c| scaled.h_advance(font.glyph_id(c))).sum::<f32>())
                .fold(0., f32::max)
        }
        // Without a font: a fair guess for a proportional typeface.
        None => label.text.lines().map(|line| line.chars().count()).max().unwrap_or(0) as f32 * label.size * 0.55,
    };
    (widest, lines * label.size * LINE_HEIGHT)
}

/// Turns a point a quarter turn counter-clockwise with a picture `width` pixels wide.
pub fn turn_left(point: (f32, f32), width: f32) -> (f32, f32) {
    (point.1, width - point.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_pixel(width, height, image::Rgba([255, 255, 255, 255]))
    }

    #[test]
    fn a_stroke_colours_its_line_and_nothing_far_from_it() {
        let mut picture = white(40, 20);
        draw_stroke(
            &mut picture,
            &Stroke {
                points: vec![(5., 10.3), (35., 10.3)],
                width: 4.,
                color: [255, 0, 0, 255],
            },
        );
        assert_eq!(picture.get_pixel(20, 10).0, [255, 0, 0, 255]);
        assert_eq!(picture.get_pixel(20, 9).0, [255, 0, 0, 255]);
        assert_eq!(picture.get_pixel(20, 3).0, [255, 255, 255, 255]);
        assert_eq!(picture.get_pixel(1, 10).0, [255, 255, 255, 255]);
        // The edge is softened, not jagged: neither pure red nor pure white.
        let edge = picture.get_pixel(20, 12).0;
        assert!(edge[1] > 0 && edge[1] < 255, "edge was {edge:?}");
    }

    #[test]
    fn a_single_point_makes_a_dot_and_a_stroke_off_the_picture_is_harmless() {
        let mut picture = white(20, 20);
        draw_stroke(
            &mut picture,
            &Stroke {
                points: vec![(10., 10.)],
                width: 6.,
                color: [0, 0, 255, 255],
            },
        );
        assert_eq!(picture.get_pixel(10, 10).0, [0, 0, 255, 255]);
        assert_eq!(picture.get_pixel(16, 10).0, [255, 255, 255, 255]);
        draw_stroke(
            &mut picture,
            &Stroke {
                points: vec![(-50., -50.), (-40., -45.)],
                width: 6.,
                color: [0, 0, 0, 255],
            },
        );
        assert_eq!(picture.get_pixel(0, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn crossing_the_same_place_twice_is_not_darker() {
        let mut once = white(30, 30);
        let mut twice = white(30, 30);
        let color = [0, 0, 0, 128];
        draw_stroke(&mut once, &Stroke { points: vec![(5., 15.), (25., 15.)], width: 6., color });
        draw_stroke(
            &mut twice,
            &Stroke {
                points: vec![(5., 15.), (25., 15.), (5., 15.)],
                width: 6.,
                color,
            },
        );
        assert_eq!(once.get_pixel(15, 15), twice.get_pixel(15, 15));
    }

    #[test]
    fn a_label_puts_ink_inside_its_extent() {
        let Some(font) = label_font() else {
            return; // No Windows fonts on this machine.
        };
        let mut picture = white(200, 80);
        let label = Label {
            at: (10., 10.),
            text: "Hi".to_string(),
            size: 40.,
            color: [0, 0, 0, 255],
        };
        draw_label(&mut picture, &label, &font);
        let (wide, tall) = label_extent(&label, Some(&font));
        let dark = |x: u32, y: u32| picture.get_pixel(x, y).0[0] < 128;
        let mut inside = 0;
        let mut outside = 0;
        for y in 0..80 {
            for x in 0..200 {
                if dark(x, y) {
                    let within = x as f32 >= 9. && (x as f32) <= 11. + wide && y as f32 >= 9. && (y as f32) <= 11. + tall;
                    if within { inside += 1 } else { outside += 1 }
                }
            }
        }
        assert!(inside > 100, "only {inside} dark pixels");
        assert_eq!(outside, 0);
    }

    #[test]
    fn turning_left_moves_the_top_right_corner_to_the_top_left() {
        assert_eq!(turn_left((100., 0.), 100.), (0., 0.));
        assert_eq!(turn_left((0., 0.), 100.), (0., 100.));
    }
}
