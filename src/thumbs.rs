//! Preview pictures for videos, made by Windows' own thumbnail service (the one File
//! Explorer uses), so no video decoder has to ship with the app.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use windows::Storage::FileProperties::ThumbnailMode;
use windows::Storage::StorageFile;
use windows::Storage::Streams::DataReader;
use windows::core::HSTRING;

/// Where the preview of a media file is kept: next to it, as `<name>.thumb.jpg`.
pub fn thumb_path(media: &Path) -> PathBuf {
    let mut name = media.as_os_str().to_owned();
    name.push(".thumb.jpg");
    PathBuf::from(name)
}

/// Makes the preview of a video file and returns where it was saved.
///
/// Blocks while Windows produces the picture, so call it off the interface thread.
pub fn make_video_thumb(media: &Path) -> Result<PathBuf> {
    let out = thumb_path(media);
    if out.exists() {
        return Ok(out);
    }
    let absolute = media
        .canonicalize()
        .with_context(|| format!("locating {}", media.display()))?;
    // The storage API wants a plain drive path, not the `\\?\` form canonicalize returns.
    let text = absolute.to_string_lossy();
    let plain = text.strip_prefix(r"\\?\").unwrap_or(&text);

    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(plain))?.join()?;
    let thumb = file
        .GetThumbnailAsyncOverloadDefaultOptions(ThumbnailMode::SingleItem, 480)?
        .join()?;
    let size = thumb.Size()? as u32;
    if size == 0 {
        return Err(anyhow!("Windows returned an empty preview"));
    }
    let reader = DataReader::CreateDataReader(&thumb.GetInputStreamAt(0)?)?;
    reader.LoadAsync(size)?.join()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes)?;

    let picture = image::load_from_memory(&bytes).context("decoding the preview")?;
    picture
        .to_rgb8()
        .save_with_format(&out, image::ImageFormat::Jpeg)
        .context("saving the preview")?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_sits_next_to_its_video() {
        assert_eq!(
            thumb_path(Path::new(r"C:\media\clip.mp4")),
            PathBuf::from(r"C:\media\clip.mp4.thumb.jpg")
        );
    }
}
