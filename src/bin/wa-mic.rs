//! Development checks for voice messages. Nothing is sent and nothing is played.
//!
//! `wa-mic`          Does the microphone open, and does audio arrive from it? Records
//!                   for half a second, reports how many samples came in, keeps nothing.
//! `wa-mic encode`   Does the encoder child process work? Encodes a two-second tone
//!                   through it, decodes the result here, and compares.

use std::time::Duration;

use whatsapp_gpui::voice::{self, Recorder};

fn main() -> anyhow::Result<()> {
    // Started as the voice encoder child: do that one job and leave.
    if voice::run_child_if_requested() {
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("encode") {
        let rate = 44_100u32;
        let samples: Vec<f32> = (0..rate as usize * 2)
            .map(|i| (i as f32 * 440. * std::f32::consts::TAU / rate as f32).sin() * 0.3)
            .collect();
        let recording = voice::finish_isolated(&samples, rate)?;
        let decoded = voice::decode_ogg_opus(&recording.ogg)?;
        let path = std::env::temp_dir().join(format!("wa-mic-{}.ogg", std::process::id()));
        recording.save(&path)?;
        let details = voice::saved_details(&path);
        let _ = std::fs::remove_file(&path);
        let mut beside = path.clone().into_os_string();
        beside.push(".voice");
        let _ = std::fs::remove_file(beside);
        println!(
            "encoded in a child process: {} bytes, {} s, {} bars; decodes to {:.2} s; saved details read back: {}",
            recording.ogg.len(),
            recording.seconds,
            recording.waveform.len(),
            decoded.len() as f32 / 48_000.,
            details == Some((recording.seconds, recording.waveform.clone()))
        );
        return Ok(());
    }

    let recorder = Recorder::start()?;
    std::thread::sleep(Duration::from_millis(500));
    let error = recorder.error();
    let levels = recorder.levels(100).len();
    let (samples, rate) = recorder.stop();
    println!(
        "microphone opened: {} samples at {} Hz ({:.2} s), {} level readings, error: {}",
        samples.len(),
        rate,
        samples.len() as f32 / rate.max(1) as f32,
        levels,
        error.as_deref().unwrap_or("none")
    );
    Ok(())
}
