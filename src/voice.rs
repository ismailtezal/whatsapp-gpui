//! Voice messages: record from the microphone and encode what was said the way WhatsApp
//! expects it, as Opus audio in an OGG file.
//!
//! [`Recorder`] captures from the default microphone. [`encode_ogg_opus`] turns the
//! captured samples into the file, and [`waveform`] makes the 64 bars WhatsApp shows on
//! a voice message.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Opus works at 48 kHz internally; voice is encoded at that rate, one channel.
const RATE: u32 = 48_000;
/// 20 ms of audio per Opus packet.
const FRAME: usize = 960;
/// Plenty for speech, and what phones use for voice notes.
const BITRATE: i32 = 32_000;
/// The longest recording kept: 30 minutes. Past that the microphone is ignored.
const MAX_SAMPLES: usize = RATE as usize * 60 * 30;

struct Captured {
    /// One channel, at the microphone's own rate.
    samples: Vec<f32>,
    /// The loudest sample of each recent 50 ms, newest last, for the live meter.
    levels: Vec<f32>,
    level_fill: usize,
    level_peak: f32,
    error: Option<String>,
}

/// A recording in progress. Dropping it stops the microphone.
pub struct Recorder {
    stream: cpal::Stream,
    captured: Arc<Mutex<Captured>>,
    device_rate: u32,
    started: Instant,
}

/// A finished voice message, ready to send.
pub struct Recording {
    /// The OGG file.
    pub ogg: Vec<u8>,
    pub seconds: u32,
    /// 64 loudness bars, 0 to 100.
    pub waveform: Vec<u8>,
}

impl Recorder {
    /// Starts recording from the default microphone.
    pub fn start() -> Result<Self> {
        let device = cpal::default_host()
            .default_input_device()
            .ok_or_else(|| anyhow!("no microphone was found"))?;
        let supported = device
            .default_input_config()
            .context("the microphone did not report a format")?;
        let device_rate = supported.sample_rate();
        let channels = supported.channels().max(1) as usize;
        let format = supported.sample_format();
        let config = supported.config();

        let captured = Arc::new(Mutex::new(Captured {
            samples: Vec::with_capacity(device_rate as usize * 30),
            levels: Vec::new(),
            level_fill: 0,
            level_peak: 0.,
            error: None,
        }));
        let per_level = (device_rate as usize / 20).max(1);

        // Every format ends up here as interleaved f32 samples.
        let sink = captured.clone();
        let push = move |data: &[f32]| {
            let Ok(mut captured) = sink.lock() else {
                return;
            };
            if captured.samples.len() >= MAX_SAMPLES {
                return;
            }
            for frame in data.chunks(channels) {
                let sample = frame.iter().sum::<f32>() / frame.len() as f32;
                captured.samples.push(sample);
                captured.level_peak = captured.level_peak.max(sample.abs());
                captured.level_fill += 1;
                if captured.level_fill >= per_level {
                    let peak = captured.level_peak;
                    captured.levels.push(peak);
                    if captured.levels.len() > 400 {
                        captured.levels.remove(0);
                    }
                    captured.level_fill = 0;
                    captured.level_peak = 0.;
                }
            }
        };
        let failed = captured.clone();
        let on_error = move |error: cpal::Error| {
            if let Ok(mut captured) = failed.lock() {
                captured.error = Some(error.to_string());
            }
        };

        let stream = match format {
            cpal::SampleFormat::F32 => {
                device.build_input_stream(config, move |data: &[f32], _: &_| push(data), on_error, None)
            }
            cpal::SampleFormat::I16 => device.build_input_stream(
                config,
                move |data: &[i16], _: &_| {
                    let floats: Vec<f32> = data.iter().map(|s| *s as f32 / 32768.).collect();
                    push(&floats)
                },
                on_error,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                config,
                move |data: &[u16], _: &_| {
                    let floats: Vec<f32> = data.iter().map(|s| (*s as f32 - 32768.) / 32768.).collect();
                    push(&floats)
                },
                on_error,
                None,
            ),
            other => bail!("the microphone uses a sample format this app cannot read ({other:?})"),
        }
        .context("could not open the microphone")?;
        stream.play().context("could not start the microphone")?;

        Ok(Recorder {
            stream,
            captured,
            device_rate,
            started: Instant::now(),
        })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// The most recent loudness readings (0 to 1), oldest first, at 20 per second.
    pub fn levels(&self, count: usize) -> Vec<f32> {
        match self.captured.lock() {
            Ok(captured) => {
                let from = captured.levels.len().saturating_sub(count);
                captured.levels[from..].to_vec()
            }
            Err(_) => Vec::new(),
        }
    }

    /// Something the microphone reported while recording, if anything went wrong.
    pub fn error(&self) -> Option<String> {
        self.captured.lock().ok().and_then(|captured| captured.error.clone())
    }

    /// Stops the microphone and returns what was captured: one channel of samples and
    /// their rate. Encoding is a separate step so it can run off the interface thread.
    pub fn stop(self) -> (Vec<f32>, u32) {
        let _ = self.stream.pause();
        drop(self.stream);
        let samples = match self.captured.lock() {
            Ok(mut captured) => std::mem::take(&mut captured.samples),
            Err(_) => Vec::new(),
        };
        (samples, self.device_rate)
    }
}

/// Encodes captured samples into a voice message, in this process.
///
/// The app itself does not call this: the Opus encoder is a C library translated to
/// Rust by a tool, full of raw pointer code, and it has no business running in the
/// process that holds the chat database and the account's keys. The app uses
/// [`finish_isolated`], which runs this in a short-lived child process.
pub fn finish(samples: &[f32], rate: u32) -> Result<Recording> {
    let samples = prepare(samples, rate)?;
    Ok(Recording {
        ogg: encode_ogg_opus(&samples)?,
        seconds: seconds_of(&samples),
        waveform: waveform(&samples),
    })
}

/// What is encoded: the recording at 48 kHz, raised to a normal loudness.
fn prepare(samples: &[f32], rate: u32) -> Result<Vec<f32>> {
    if samples.len() < rate as usize / 2 {
        bail!("the recording is too short");
    }
    Ok(normalise(&resample(samples, rate, RATE)))
}

fn seconds_of(prepared: &[f32]) -> u32 {
    (prepared.len() as f32 / RATE as f32).round().max(1.) as u32
}

/// The command-line flag that turns any of the app's programs into the encoder child.
const CHILD_FLAG: &str = "--encode-voice";

/// Encodes captured samples into a voice message in a child process (this same
/// program started with a flag), so the encoder cannot touch this process's memory.
/// The length and the waveform are worked out here, in ordinary safe code.
pub fn finish_isolated(samples: &[f32], rate: u32) -> Result<Recording> {
    let prepared = prepare(samples, rate)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let folder = std::env::temp_dir();
    let input = folder.join(format!("wa-voice-{}-{stamp}.f32", std::process::id()));
    let output = folder.join(format!("wa-voice-{}-{stamp}.ogg", std::process::id()));
    let mut bytes = Vec::with_capacity(prepared.len() * 4);
    for sample in &prepared {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(&input, bytes).context("handing the recording to the encoder")?;

    let mut command = std::process::Command::new(std::env::current_exe()?);
    command.arg(CHILD_FLAG).arg(&input).arg(&output);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashes up for the child.
        command.creation_flags(0x0800_0000);
    }
    let result = command.output();
    let _ = std::fs::remove_file(&input);
    let result = result.context("starting the encoder")?;
    if !result.status.success() {
        let _ = std::fs::remove_file(&output);
        bail!(
            "the encoder failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }
    let ogg = std::fs::read(&output).context("reading the encoded recording")?;
    let _ = std::fs::remove_file(&output);
    if !ogg.starts_with(b"OggS") {
        bail!("the encoder did not produce a voice file");
    }
    Ok(Recording {
        ogg,
        seconds: seconds_of(&prepared),
        waveform: waveform(&prepared),
    })
}

/// If this process was started as the encoder child, does that job and returns true:
/// the caller must then exit instead of starting normally. Call it first in `main`.
pub fn run_child_if_requested() -> bool {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.get(1).and_then(|arg| arg.to_str()) != Some(CHILD_FLAG) {
        return false;
    }
    let work = || -> Result<()> {
        let input = args.get(2).context("missing input file")?;
        let output = args.get(3).context("missing output file")?;
        let bytes = std::fs::read(input)?;
        let samples: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();
        std::fs::write(output, encode_ogg_opus(&samples)?)?;
        Ok(())
    };
    if let Err(error) = work() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
    true
}

impl Recording {
    /// Saves the voice file, and next to it a small file with its length and waveform,
    /// which the sender reads instead of decoding the audio again.
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        std::fs::write(path, &self.ogg)?;
        let bars: String = self.waveform.iter().map(|bar| format!("{bar:02x}")).collect();
        std::fs::write(details_path(path), format!("{}\n{bars}\n", self.seconds))?;
        Ok(())
    }
}

fn details_path(voice_file: &std::path::Path) -> std::path::PathBuf {
    let mut name = voice_file.as_os_str().to_owned();
    name.push(".voice");
    std::path::PathBuf::from(name)
}

/// The length in seconds and the waveform saved beside a voice file by
/// [`Recording::save`], if they are there.
pub fn saved_details(voice_file: &std::path::Path) -> Option<(u32, Vec<u8>)> {
    let text = std::fs::read_to_string(details_path(voice_file)).ok()?;
    let mut lines = text.lines();
    let seconds: u32 = lines.next()?.trim().parse().ok()?;
    let bars = lines.next()?.trim();
    let waveform: Option<Vec<u8>> = (0..bars.len() / 2)
        .map(|index| u8::from_str_radix(bars.get(index * 2..index * 2 + 2)?, 16).ok())
        .collect();
    Some((seconds, waveform?))
}

/// Changes the sample rate by linear interpolation. When slowing down (fewer samples
/// out than in) each output sample averages the input it covers, which keeps high
/// frequencies from folding back as noise.
pub fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = (samples.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for index in 0..out_len {
        let start = index as f64 * ratio;
        if ratio > 1. {
            let first = start.floor() as usize;
            let last = (((index + 1) as f64 * ratio).ceil() as usize).min(samples.len());
            let span = &samples[first..last.max(first + 1).min(samples.len())];
            out.push(span.iter().sum::<f32>() / span.len() as f32);
        } else {
            let left = start.floor() as usize;
            let right = (left + 1).min(samples.len() - 1);
            let blend = (start - left as f64) as f32;
            out.push(samples[left] * (1. - blend) + samples[right] * blend);
        }
    }
    out
}

/// Raises a quiet recording so its loudest moment is near full scale, as phones do.
/// A recording that is already loud, or silent, is left alone.
fn normalise(samples: &[f32]) -> Vec<f32> {
    let peak = samples.iter().fold(0f32, |peak, s| peak.max(s.abs()));
    if !(0.01..0.6).contains(&peak) {
        return samples.to_vec();
    }
    let gain = (0.89 / peak).min(8.);
    samples.iter().map(|s| (s * gain).clamp(-1., 1.)).collect()
}

/// 64 loudness bars (0 to 100) across the whole recording.
pub fn waveform(samples: &[f32]) -> Vec<u8> {
    const BARS: usize = 64;
    if samples.is_empty() {
        return vec![0; BARS];
    }
    let per_bar = (samples.len() / BARS).max(1);
    let mut bars: Vec<f32> = samples
        .chunks(per_bar)
        .take(BARS)
        .map(|chunk| (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt())
        .collect();
    bars.resize(BARS, 0.);
    let loudest = bars.iter().cloned().fold(0f32, f32::max).max(1e-6);
    bars.iter().map(|bar| (bar / loudest * 100.).round() as u8).collect()
}

/// Encodes 48 kHz mono samples as Opus in an OGG container.
pub fn encode_ogg_opus(samples: &[f32]) -> Result<Vec<u8>> {
    use ogg::writing::{PacketWriteEndInfo, PacketWriter};
    use unsafe_libopus::{
        OPUS_APPLICATION_VOIP, OPUS_GET_LOOKAHEAD_REQUEST, OPUS_OK, OPUS_SET_BITRATE_REQUEST,
        opus_encode_float, opus_encoder_create, opus_encoder_destroy,
    };

    /// Frees the encoder however the function is left.
    struct Encoder(*mut unsafe_libopus::OpusEncoder);
    impl Drop for Encoder {
        fn drop(&mut self) {
            // SAFETY: the pointer came from opus_encoder_create and is freed once.
            unsafe { opus_encoder_destroy(self.0) }
        }
    }

    let mut error = 0;
    // SAFETY: plain constructor call; the result is checked before use.
    let raw = unsafe { opus_encoder_create(RATE as i32, 1, OPUS_APPLICATION_VOIP, &mut error) };
    if raw.is_null() || error != OPUS_OK {
        bail!("could not create the Opus encoder (code {error})");
    }
    let encoder = Encoder(raw);
    let mut lookahead: i32 = 0;
    // SAFETY: the encoder is live; each request gets the argument type it expects.
    unsafe {
        unsafe_libopus::opus_encoder_ctl!(encoder.0, OPUS_SET_BITRATE_REQUEST, BITRATE);
        unsafe_libopus::opus_encoder_ctl!(encoder.0, OPUS_GET_LOOKAHEAD_REQUEST, &mut lookahead);
    }
    let pre_skip = lookahead.max(0) as u16;

    let serial = 0x5741_7631; // any number; one stream per file
    let mut file = Vec::new();
    {
        let mut writer = PacketWriter::new(&mut file);

        // Identification header.
        let mut head = Vec::with_capacity(19);
        head.extend_from_slice(b"OpusHead");
        head.push(1); // version
        head.push(1); // channels
        head.extend_from_slice(&pre_skip.to_le_bytes());
        head.extend_from_slice(&RATE.to_le_bytes());
        head.extend_from_slice(&0i16.to_le_bytes()); // output gain
        head.push(0); // channel mapping family: mono or stereo
        writer.write_packet(head, serial, PacketWriteEndInfo::EndPage, 0)?;

        // Comment header.
        let vendor = b"whatsapp-gpui";
        let mut tags = Vec::new();
        tags.extend_from_slice(b"OpusTags");
        tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        tags.extend_from_slice(vendor);
        tags.extend_from_slice(&0u32.to_le_bytes());
        writer.write_packet(tags, serial, PacketWriteEndInfo::EndPage, 0)?;

        // Audio: 20 ms packets. The encoder delays its output by `pre_skip` samples,
        // so enough silence is appended for the last real samples to come out.
        let mut padded = samples.to_vec();
        padded.extend(std::iter::repeat_n(0., pre_skip as usize));
        let frames = padded.len().div_ceil(FRAME);
        padded.resize(frames * FRAME, 0.);
        let total = pre_skip as u64 + samples.len() as u64;
        let mut packet = vec![0u8; 4000];
        for (index, frame) in padded.chunks(FRAME).enumerate() {
            // SAFETY: `frame` holds FRAME samples and `packet` is as long as stated.
            let written = unsafe {
                opus_encode_float(
                    encoder.0,
                    frame.as_ptr(),
                    FRAME as i32,
                    packet.as_mut_ptr(),
                    packet.len() as i32,
                )
            };
            if written < 0 {
                bail!("Opus encoding failed (code {written})");
            }
            let last = index + 1 == frames;
            // The position after this packet, counted in samples; the last one states
            // the true length so players stop exactly where the recording ended.
            let position = if last {
                total
            } else {
                ((index + 1) * FRAME) as u64
            };
            writer.write_packet(
                packet[..written as usize].to_vec(),
                serial,
                if last {
                    PacketWriteEndInfo::EndStream
                } else {
                    PacketWriteEndInfo::NormalPacket
                },
                position,
            )?;
        }
    }
    Ok(file)
}

/// Decodes an OGG Opus file back to 48 kHz mono samples. Used to check the encoder.
pub fn decode_ogg_opus(file: &[u8]) -> Result<Vec<f32>> {
    use unsafe_libopus::{OPUS_OK, opus_decode_float, opus_decoder_create, opus_decoder_destroy};

    let mut reader = ogg::reading::PacketReader::new(std::io::Cursor::new(file));
    let head = reader.read_packet()?.ok_or_else(|| anyhow!("empty file"))?;
    if !head.data.starts_with(b"OpusHead") || head.data.len() < 19 {
        bail!("not an Opus file");
    }
    let pre_skip = u16::from_le_bytes([head.data[10], head.data[11]]) as usize;
    let tags = reader.read_packet()?.ok_or_else(|| anyhow!("missing comment header"))?;
    if !tags.data.starts_with(b"OpusTags") {
        bail!("missing comment header");
    }

    let mut error = 0;
    // SAFETY: plain constructor call; the result is checked before use.
    let decoder = unsafe { opus_decoder_create(RATE as i32, 1, &mut error) };
    if decoder.is_null() || error != OPUS_OK {
        bail!("could not create the Opus decoder (code {error})");
    }
    let mut samples = Vec::new();
    let mut end = None;
    let mut buffer = vec![0f32; 5760];
    let result = (|| -> Result<()> {
        while let Some(packet) = reader.read_packet()? {
            // SAFETY: the decoder is live and both buffers are as long as stated.
            let decoded = unsafe {
                opus_decode_float(
                    decoder,
                    packet.data.as_ptr(),
                    packet.data.len() as i32,
                    buffer.as_mut_ptr(),
                    buffer.len() as i32,
                    0,
                )
            };
            if decoded < 0 {
                bail!("Opus decoding failed (code {decoded})");
            }
            samples.extend_from_slice(&buffer[..decoded as usize]);
            if packet.last_in_stream() {
                end = Some(packet.absgp_page() as usize);
            }
        }
        Ok(())
    })();
    // SAFETY: the pointer came from opus_decoder_create and is freed once.
    unsafe { opus_decoder_destroy(decoder) };
    result?;
    if let Some(end) = end {
        samples.truncate(end.min(samples.len()));
    }
    Ok(samples.split_off(pre_skip.min(samples.len())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f32, hz: f32, rate: u32) -> Vec<f32> {
        (0..(seconds * rate as f32) as usize)
            .map(|i| (i as f32 * hz * std::f32::consts::TAU / rate as f32).sin() * 0.5)
            .collect()
    }

    /// How strongly `samples` contains a tone of `hz` (0 to about 0.5 for a full tone).
    fn strength(samples: &[f32], hz: f32) -> f32 {
        let (mut re, mut im) = (0f32, 0f32);
        for (i, sample) in samples.iter().enumerate() {
            let angle = i as f32 * hz * std::f32::consts::TAU / RATE as f32;
            re += sample * angle.cos();
            im += sample * angle.sin();
        }
        (re * re + im * im).sqrt() / samples.len() as f32
    }

    #[test]
    fn a_tone_survives_encoding_and_decoding() {
        let original = tone(1.5, 440., RATE);
        let file = encode_ogg_opus(&original).expect("encode");
        assert!(file.starts_with(b"OggS"));
        // 1.5 seconds at 32 kbit/s is about 6 kB; far more means something is wrong.
        assert!(file.len() > 2_000 && file.len() < 12_000, "size {}", file.len());

        let decoded = decode_ogg_opus(&file).expect("decode");
        assert_eq!(decoded.len(), original.len(), "length must be exact");
        let kept = strength(&decoded, 440.);
        let other = strength(&decoded, 1000.);
        assert!(kept > 0.15, "the tone was lost: {kept}");
        assert!(other < kept / 10., "energy appeared elsewhere: {other} vs {kept}");
    }

    #[test]
    fn resampling_keeps_the_pitch() {
        let at_44k = tone(1., 440., 44_100);
        let at_48k = resample(&at_44k, 44_100, RATE);
        assert!((at_48k.len() as i64 - 48_000).abs() <= 2);
        assert!(strength(&at_48k, 440.) > 0.2);

        let at_96k = tone(1., 440., 96_000);
        let down = resample(&at_96k, 96_000, RATE);
        assert!((down.len() as i64 - 48_000).abs() <= 2);
        assert!(strength(&down, 440.) > 0.2);
    }

    #[test]
    fn the_waveform_follows_loudness() {
        let mut samples = vec![0f32; RATE as usize];
        samples.extend(tone(1., 300., RATE));
        let bars = waveform(&samples);
        assert_eq!(bars.len(), 64);
        assert!(bars[..30].iter().all(|bar| *bar == 0), "silence must be flat");
        assert!(bars[34..].iter().all(|bar| *bar > 80), "the tone must be tall");
    }

    #[test]
    fn a_recording_shorter_than_half_a_second_is_refused() {
        assert!(finish(&tone(0.2, 440., RATE), RATE).is_err());
        let recording = finish(&tone(2., 440., 44_100), 44_100).expect("finish");
        assert_eq!(recording.seconds, 2);
        assert_eq!(recording.waveform.len(), 64);
    }
}
