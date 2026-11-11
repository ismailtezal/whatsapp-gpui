//! Development check: can Windows make a preview picture for a video file?
//! Usage: wa-thumb <video file>

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_default();
    let out = whatsapp_gpui::thumbs::make_video_thumb(std::path::Path::new(&path))?;
    let size = std::fs::metadata(&out)?.len();
    let dimensions = image::image_dimensions(&out)?;
    println!("preview saved: {size} bytes, {}x{}", dimensions.0, dimensions.1);
    Ok(())
}
