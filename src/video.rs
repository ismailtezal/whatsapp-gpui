//! Decoding video frames with Windows Media Foundation, for in-app playback.
//!
//! Frames come out as top-down BGRA, one per call, with their presentation time.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFCreateAttributes,
    MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Video, MFSTARTUP_NOSOCKET, MFStartup,
    MFVideoFormat_RGB32,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::HSTRING;

/// One decoded picture.
pub struct Frame {
    /// When to show it, in seconds from the start.
    pub at_secs: f64,
    /// Top-down BGRA, `width * height * 4` bytes, alpha set to opaque.
    pub bgra: Vec<u8>,
}

pub struct VideoReader {
    reader: IMFSourceReader,
    pub width: u32,
    pub height: u32,
    /// Whether rows arrive bottom-up and need flipping.
    bottom_up: bool,
}

// The reader is used from one decoding thread at a time; Media Foundation objects created
// in the multithreaded apartment may move between threads.
unsafe impl Send for VideoReader {}

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

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

            let wanted: IMFMediaType = MFCreateMediaType()?;
            wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            reader
                .SetCurrentMediaType(VIDEO, None, &wanted)
                .context("asking for RGB frames")?;

            let actual = reader.GetCurrentMediaType(VIDEO)?;
            let size = actual.GetUINT64(&MF_MT_FRAME_SIZE)?;
            let (width, height) = ((size >> 32) as u32, size as u32);
            if width == 0 || height == 0 {
                return Err(anyhow!("the video reports no picture size"));
            }
            // A negative stride means rows are stored bottom-up. RGB without a stated
            // stride is bottom-up by convention.
            let bottom_up = match actual.GetUINT32(&MF_MT_DEFAULT_STRIDE) {
                Ok(stride) => (stride as i32) < 0,
                Err(_) => true,
            };
            Ok(VideoReader {
                reader,
                width,
                height,
                bottom_up,
            })
        }
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
                // A gap in the stream yields no sample; ask again.
                let Some(sample) = sample else {
                    continue;
                };
                let buffer = sample.ConvertToContiguousBuffer()?;
                let mut data = std::ptr::null_mut();
                let mut length = 0u32;
                buffer.Lock(&mut data, None, Some(&mut length))?;
                let row = self.width as usize * 4;
                let wanted = row * self.height as usize;
                let available = (length as usize).min(wanted);
                let source = std::slice::from_raw_parts(data, available);
                let mut bgra = vec![0u8; wanted];
                if self.bottom_up {
                    for (index, line) in source.chunks_exact(row).enumerate() {
                        let target = (self.height as usize - 1 - index) * row;
                        bgra[target..target + row].copy_from_slice(line);
                    }
                } else {
                    bgra[..available].copy_from_slice(source);
                }
                buffer.Unlock()?;
                // The fourth byte is padding, not alpha: make every pixel opaque.
                for pixel in bgra.chunks_exact_mut(4) {
                    pixel[3] = 255;
                }
                return Ok(Some(Frame {
                    // Media Foundation counts time in units of 100 nanoseconds.
                    at_secs: timestamp as f64 / 10_000_000.,
                    bgra,
                }));
            }
        }
    }
}
