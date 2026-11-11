//! Development check: decode a video with the app's reader, report what came out, and save
//! one frame as a picture. Usage: wa-video <video file> <frame.jpg>

use whatsapp_gpui::video::VideoReader;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut reader = VideoReader::open(std::path::Path::new(&args[0]))?;
    let (width, height) = (reader.width, reader.height);
    let mut count = 0usize;
    let mut last = 0.;
    let started = std::time::Instant::now();
    while let Some(frame) = reader.next_frame()? {
        count += 1;
        last = frame.at_secs;
        if count == 5
            && let Some(out) = args.get(1)
        {
            // BGRA to RGB for saving.
            let rgb: Vec<u8> = frame
                .bgra
                .chunks_exact(4)
                .flat_map(|p| [p[2], p[1], p[0]])
                .collect();
            image::RgbImage::from_raw(width, height, rgb)
                .ok_or_else(|| anyhow::anyhow!("frame size mismatch"))?
                .save(out)?;
        }
    }
    println!(
        "{width}x{height}, {count} frames, last at {last:.2}s, decoded in {:?}",
        started.elapsed()
    );
    Ok(())
}
