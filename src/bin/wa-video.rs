//! Development aid: reports how Media Foundation describes video files, and can save one
//! decoded picture, to check the in-app player against real files.
//!
//!   wa-video <folder or file>            one line per video: sizes, rotation, row length
//!   wa-video <file> --frame 30 out.png   also saves that picture as the player shows it

use std::path::{Path, PathBuf};

use whatsapp_gpui::video::VideoReader;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(target) = args.next() else {
        eprintln!("usage: wa-video <folder or file> [--frame N out.png]");
        std::process::exit(2);
    };
    let mut frame: Option<(usize, PathBuf)> = None;
    if args.next().as_deref() == Some("--frame") {
        let index = args.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let out = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("frame.png"));
        frame = Some((index, out));
    }
    let target = PathBuf::from(target);
    let mut files = Vec::new();
    collect(&target, &mut files);
    for (number, file) in files.iter().enumerate() {
        // Names are private; a number identifies the file within this run.
        match VideoReader::open(file) {
            Ok(mut reader) => {
                let details = reader.describe();
                let first = reader.next_frame();
                let decoded = match &first {
                    Ok(Some(picture)) => format!("first picture at {:.2}s, {} bytes", picture.at_secs, picture.bgra.len()),
                    Ok(None) => "no pictures".to_string(),
                    Err(error) => format!("decode error: {error:#}"),
                };
                println!("#{number} shown {}x{} | {details} | {decoded}", reader.width, reader.height);
                if let Some((index, out)) = &frame {
                    let mut picture = first.ok().flatten();
                    for _ in 0..*index {
                        match reader.next_frame() {
                            Ok(Some(next)) => picture = Some(next),
                            _ => break,
                        }
                    }
                    if let Some(picture) = picture {
                        let mut rgba = picture.bgra;
                        for pixel in rgba.chunks_exact_mut(4) {
                            pixel.swap(0, 2);
                        }
                        match image::RgbaImage::from_raw(reader.width, reader.height, rgba) {
                            Some(image) => match image.save(out) {
                                Ok(()) => println!("saved {}", out.display()),
                                Err(error) => println!("could not save: {error}"),
                            },
                            None => println!("picture size does not match its bytes"),
                        }
                    }
                }
            }
            Err(error) => println!("#{number} cannot open: {error:#}"),
        }
    }
    println!("{} video files", files.len());
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect(&entry.path(), out);
        }
    } else if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "mp4" | "mov" | "3gp" | "m4v" | "webm" | "mkv"))
    {
        out.push(path.to_path_buf());
    }
}
