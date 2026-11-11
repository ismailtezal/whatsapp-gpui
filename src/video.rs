//! Decoding video frames with Windows Media Foundation, for in-app playback.
//!
//! Frames come out as top-down BGRA, one per call, with their presentation time, already
//! cropped to the visible part of the picture and turned the way the file says it must
//! be shown (phones record upright video as a sideways picture plus a rotation).

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_PIXEL_ASPECT_RATIO,
    MF_MT_SUBTYPE, MF_MT_VIDEO_ROTATION, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFCreateAttributes,
    MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Video, MFSTARTUP_NOSOCKET, MFStartup,
    MFVideoArea, MFVideoFormat_RGB32,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::{HSTRING, Interface};

/// One decoded picture.
pub struct Frame {
    /// When to show it, in seconds from the start.
    pub at_secs: f64,
    /// Top-down BGRA, `width * height * 4` bytes, alpha set to opaque.
    pub bgra: Vec<u8>,
}

/// How the decoder lays out the pictures it hands over.
#[derive(Clone, Copy, Debug)]
struct Layout {
    /// Size of the decoded picture, which may be padded beyond what is shown.
    coded: (u32, u32),
    /// The part that is shown: left, top, width, height.
    visible: (u32, u32, u32, u32),
    /// Bytes from one row to the next when the buffer does not say; negative when rows
    /// are stored bottom-up.
    stride: i32,
}

pub struct VideoReader {
    reader: IMFSourceReader,
    /// Size of the pictures this reader returns (after cropping and turning).
    pub width: u32,
    pub height: u32,
    layout: Layout,
    /// Clockwise quarter turns needed to show the picture upright (0 to 3).
    turns: u32,
    /// Width of a pixel divided by its height; 1 for square pixels.
    pub pixel_aspect: f32,
    /// The rotation the file states, in degrees, for reports.
    stated_rotation: u32,
}

// The reader is used from one decoding thread at a time; Media Foundation objects created
// in the multithreaded apartment may move between threads.
unsafe impl Send for VideoReader {}

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// Reads the picture layout from the type the reader currently produces.
///
/// SAFETY: `kind` must be a live media type.
unsafe fn layout_of(kind: &IMFMediaType) -> Result<Layout> {
    unsafe {
        let size = kind.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let coded = ((size >> 32) as u32, size as u32);
        if coded.0 == 0 || coded.1 == 0 {
            return Err(anyhow!("the video reports no picture size"));
        }
        // Decoders pad pictures to whole blocks (1080 rows become 1088); the display
        // aperture says which part is the picture.
        let mut visible = (0, 0, coded.0, coded.1);
        let mut area = MFVideoArea::default();
        let bytes = std::slice::from_raw_parts_mut(
            (&mut area as *mut MFVideoArea).cast::<u8>(),
            std::mem::size_of::<MFVideoArea>(),
        );
        if kind.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, bytes, None).is_ok() {
            let (left, top) = (area.OffsetX.value.max(0) as u32, area.OffsetY.value.max(0) as u32);
            let (width, height) = (area.Area.cx.max(0) as u32, area.Area.cy.max(0) as u32);
            if width > 0 && height > 0 && left + width <= coded.0 && top + height <= coded.1 {
                visible = (left, top, width, height);
            }
        }
        // RGB without a stated stride is bottom-up by convention.
        let stride = match kind.GetUINT32(&MF_MT_DEFAULT_STRIDE) {
            Ok(stride) => stride as i32,
            Err(_) => -((coded.0 * 4) as i32),
        };
        Ok(Layout { coded, visible, stride })
    }
}

impl VideoReader {
    pub fn open(path: &Path) -> Result<Self> {
        let absolute = path
            .canonicalize()
            .with_context(|| format!("locating {}", path.display()))?;
        let text = absolute.to_string_lossy();
        let plain = text.strip_prefix(r"\\?\").unwrap_or(&text).to_string();
        unsafe {
            // Both calls are reference counted and harmless to repeat.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).context("starting Media Foundation")?;

            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 1)?;
            let attributes = attributes.ok_or_else(|| anyhow!("no attribute store"))?;
            // Lets the reader convert whatever the file holds into plain RGB.
            attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;

            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(plain), &attributes)
                .context("opening the video")?;
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader.SetStreamSelection(VIDEO, true)?;

            // What the file itself says: how the picture must be turned, and the shape
            // of its pixels. The converted type below does not always carry these over.
            let native = reader.GetNativeMediaType(VIDEO, 0).ok();
            let stated_rotation = native
                .as_ref()
                .and_then(|kind| kind.GetUINT32(&MF_MT_VIDEO_ROTATION).ok())
                .unwrap_or(0)
                % 360;
            let pixel_aspect = native
                .as_ref()
                .and_then(|kind| kind.GetUINT64(&MF_MT_PIXEL_ASPECT_RATIO).ok())
                .map(|ratio| ((ratio >> 32) as u32, ratio as u32))
                .filter(|(across, down)| *across > 0 && *down > 0)
                .map(|(across, down)| across as f32 / down as f32)
                .unwrap_or(1.);

            let wanted: IMFMediaType = MFCreateMediaType()?;
            wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            reader
                .SetCurrentMediaType(VIDEO, None, &wanted)
                .context("asking for RGB frames")?;

            let layout = layout_of(&reader.GetCurrentMediaType(VIDEO)?)?;
            // The stated rotation is counter-clockwise from upright, so the picture is
            // turned clockwise by the same amount to stand it up.
            let turns = stated_rotation / 90;
            let mut video = VideoReader {
                reader,
                width: 0,
                height: 0,
                layout,
                turns,
                pixel_aspect,
                stated_rotation,
            };
            video.apply_layout(layout);
            Ok(video)
        }
    }

    fn apply_layout(&mut self, layout: Layout) {
        self.layout = layout;
        let (_, _, width, height) = layout.visible;
        (self.width, self.height) = if self.turns % 2 == 1 { (height, width) } else { (width, height) };
    }

    /// A line for reports: what the file and the decoder state.
    pub fn describe(&self) -> String {
        format!(
            "decoded {}x{}, visible {:?}, stride {}, rotation {} degrees, pixel shape {:.3}",
            self.layout.coded.0, self.layout.coded.1, self.layout.visible, self.layout.stride,
            self.stated_rotation, self.pixel_aspect
        )
    }

    /// Jumps to a position, in seconds. The next picture decoded is the key frame at or
    /// before it; the caller shows the ones up to the position without waiting.
    pub fn seek(&mut self, seconds: f64) -> Result<()> {
        let position = windows::Win32::System::Com::StructuredStorage::PROPVARIANT::from((seconds.max(0.) * 10_000_000.) as i64);
        // SAFETY: the reader is live; a null time format means 100-nanosecond units.
        unsafe {
            self.reader
                .SetCurrentPosition(&windows::core::GUID::zeroed(), &position)
                .context("jumping in the video")?;
        }
        Ok(())
    }

    /// Decodes the next picture. `None` at the end of the video.
    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        unsafe {
            loop {
                let mut flags = 0u32;
                let mut timestamp = 0i64;
                let mut sample: Option<IMFSample> = None;
                self.reader.ReadSample(
                    VIDEO,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut timestamp),
                    Some(&mut sample),
                )?;
                if flags & (MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
                    return Ok(None);
                }
                // The picture size can change in mid-stream; the layout follows it.
                if flags & (MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32) != 0
                    && let Ok(kind) = self.reader.GetCurrentMediaType(VIDEO)
                    && let Ok(layout) = layout_of(&kind)
                {
                    self.apply_layout(layout);
                }
                // A gap in the stream yields no sample; ask again.
                let Some(sample) = sample else {
                    continue;
                };
                let buffer = sample.ConvertToContiguousBuffer()?;
                let upright = self.copy_visible(&buffer)?;
                let bgra = self.turned(upright);
                return Ok(Some(Frame {
                    // Media Foundation counts time in units of 100 nanoseconds.
                    at_secs: timestamp as f64 / 10_000_000.,
                    bgra,
                }));
            }
        }
    }

    /// Copies the visible part of a decoded picture as top-down, opaque BGRA.
    ///
    /// SAFETY: `buffer` must be a live media buffer holding one RGB32 picture.
    unsafe fn copy_visible(&self, buffer: &windows::Win32::Media::MediaFoundation::IMFMediaBuffer) -> Result<Vec<u8>> {
        let (left, top, width, height) = self.layout.visible;
        let (width, height) = (width as usize, height as usize);
        let row = width * 4;
        let mut out = vec![0u8; row * height];
        // Copies the rows given where the top row starts and the signed distance from
        // one row to the next; `limit` bounds how far the source may be read.
        let copy = |out: &mut [u8], first: *const u8, pitch: isize, begin: *const u8, end: *const u8| {
            for y in 0..height {
                // SAFETY: each row is checked against the buffer before it is read.
                unsafe {
                    let line = first.offset((top as usize + y) as isize * pitch).add(left as usize * 4);
                    if line < begin || line.add(row) > end {
                        break;
                    }
                    std::ptr::copy_nonoverlapping(line, out[y * row..].as_mut_ptr(), row);
                }
            }
        };
        unsafe {
            if let Ok(plane) = buffer.cast::<IMF2DBuffer>() {
                // The buffer knows its own row length and direction.
                let mut first = std::ptr::null_mut();
                let mut pitch = 0i32;
                plane.Lock2D(&mut first, &mut pitch)?;
                let span = pitch.unsigned_abs() as usize * self.layout.coded.1 as usize;
                let (begin, end) = if pitch >= 0 {
                    (first as *const u8, (first as *const u8).add(span))
                } else {
                    // Bottom-up: the top row is the last one in memory.
                    let begin = (first as *const u8).offset(pitch as isize * (self.layout.coded.1 as isize - 1));
                    (begin, begin.add(span))
                };
                copy(&mut out, first, pitch as isize, begin, end);
                plane.Unlock2D()?;
            } else {
                let mut data = std::ptr::null_mut();
                let mut length = 0u32;
                buffer.Lock(&mut data, None, Some(&mut length))?;
                let pitch = self.layout.stride;
                let begin = data as *const u8;
                let end = begin.add(length as usize);
                let first = if pitch >= 0 {
                    begin
                } else {
                    begin.add(pitch.unsigned_abs() as usize * (self.layout.coded.1 as usize - 1))
                };
                copy(&mut out, first, pitch as isize, begin, end);
                buffer.Unlock()?;
            }
        }
        // The fourth byte is padding, not alpha: make every pixel opaque.
        for pixel in out.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Ok(out)
    }

    /// Turns a picture clockwise by this reader's quarter turns.
    fn turned(&self, source: Vec<u8>) -> Vec<u8> {
        let (_, _, width, height) = self.layout.visible;
        let (width, height) = (width as usize, height as usize);
        let pixels: &[[u8; 4]] = bytemuck_cast(&source);
        match self.turns % 4 {
            0 => source,
            2 => {
                let mut out = source;
                bytemuck_cast_mut(&mut out).reverse();
                out
            }
            turns => {
                // A quarter turn: the result is `height` wide and `width` tall.
                let mut out = vec![0u8; source.len()];
                let target: &mut [[u8; 4]] = bytemuck_cast_mut(&mut out);
                for y in 0..height {
                    let line = &pixels[y * width..(y + 1) * width];
                    for (x, pixel) in line.iter().enumerate() {
                        let (to_x, to_y) = if turns == 1 {
                            // Clockwise: the top row becomes the right column.
                            (height - 1 - y, x)
                        } else {
                            // Counter-clockwise: the top row becomes the left column.
                            (y, width - 1 - x)
                        };
                        target[to_y * height + to_x] = *pixel;
                    }
                }
                out
            }
        }
    }
}

/// Views BGRA bytes as whole pixels.
fn bytemuck_cast(bytes: &[u8]) -> &[[u8; 4]] {
    // SAFETY: [u8; 4] has the alignment of u8 and the length is a whole number of pixels.
    unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast(), bytes.len() / 4) }
}

fn bytemuck_cast_mut(bytes: &mut [u8]) -> &mut [[u8; 4]] {
    // SAFETY: as above, and the borrow is exclusive.
    unsafe { std::slice::from_raw_parts_mut(bytes.as_mut_ptr().cast(), bytes.len() / 4) }
}
