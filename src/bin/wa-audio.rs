//! Development check: can this machine play a given audio file through the app's player?
//! Usage: wa-audio <file> [seconds]

use std::time::Duration;

use whatsapp_gpui::audio::AudioPlayer;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::PathBuf::from(args.first().map(String::as_str).unwrap_or_default());
    let seconds: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2);
    let mut player = AudioPlayer::new()?;
    player.play("check", &path)?;
    for _ in 0..seconds * 4 {
        std::thread::sleep(Duration::from_millis(250));
    }
    match player.playback() {
        Some(state) => println!(
            "playing={} position={:.2}s duration={:.2}s",
            state.playing, state.position_secs, state.duration_secs
        ),
        None => println!("no playback state"),
    }
    player.stop();
    Ok(())
}
