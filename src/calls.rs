//! Voice calls.
//!
//! The connection library does the calling itself: signalling, keys, the relay, the
//! codec and encryption. This module gives it a microphone and a speaker ([`CallAudio`])
//! and keeps track of the call in progress and the calls that are ringing ([`Calls`]),
//! so the interface can start, answer, decline, mute and end them.
//!
//! Sound moves in 60 ms pieces of 16 kHz mono, which is what the library exchanges.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use whatsapp_rust::async_channel;
use whatsapp_rust::prelude::Client;
use whatsapp_rust::types::events::Event;
use whatsapp_rust::voip::CallHandle;
use whatsapp_rust::wacore::types::call::{CallAction, IncomingCall};

use crate::events::{CallUpdate, UiEvent};
use crate::store::Store;

/// The library's audio rate and piece size: 16 kHz, 960 samples (60 ms).
const CALL_RATE: u32 = 16_000;
const CALL_FRAME: usize = 960;

/// Changes the sample rate of a stream that arrives in pieces: averages when slowing
/// down, interpolates when speeding up, and carries what is left between pieces.
struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Position of the next output sample, in input samples from the start of `held`.
    at: f64,
    held: Vec<f32>,
}

impl Resampler {
    fn new(from: u32, to: u32) -> Self {
        Resampler {
            step: from as f64 / to as f64,
            at: 0.,
            held: Vec::new(),
        }
    }

    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.held.extend_from_slice(input);
        loop {
            let start = self.at.floor() as usize;
            let end = (self.at + self.step).ceil() as usize;
            if end >= self.held.len() || start + 1 >= self.held.len() {
                break;
            }
            if self.step >= 1. {
                let span = &self.held[start..end.max(start + 1)];
                out.push(span.iter().sum::<f32>() / span.len() as f32);
            } else {
                let blend = (self.at - start as f64) as f32;
                out.push(self.held[start] * (1. - blend) + self.held[start + 1] * blend);
            }
            self.at += self.step;
        }
        // Drop what has been used up.
        let used = (self.at.floor() as usize).min(self.held.len());
        self.held.drain(..used);
        self.at -= used as f64;
    }
}

/// The microphone and the speaker of a call. Dropping it (or [`CallAudio::stop`]) closes
/// both.
pub struct CallAudio {
    stop: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    /// 16 kHz pieces from the microphone, for the library to send.
    pub mic: async_channel::Receiver<Vec<i16>>,
    /// Where the library puts what it received, to be played.
    pub speaker: async_channel::Sender<Vec<i16>>,
}

impl CallAudio {
    /// Opens the default microphone and speaker.
    pub fn start() -> Result<CallAudio> {
        let stop = Arc::new(AtomicBool::new(false));
        let muted = Arc::new(AtomicBool::new(false));
        let (mic_tx, mic) = async_channel::bounded::<Vec<i16>>(8);
        let (speaker, speaker_rx) = async_channel::bounded::<Vec<i16>>(16);
        let (ready_tx, ready) = std::sync::mpsc::channel::<Result<()>>();
        let (thread_stop, thread_muted) = (stop.clone(), muted.clone());
        // The audio streams must stay on the thread that made them.
        std::thread::Builder::new()
            .name("call-audio".into())
            .spawn(move || {
                let streams = open_streams(mic_tx, speaker_rx, thread_muted, thread_stop.clone());
                match streams {
                    Ok(streams) => {
                        let _ = ready_tx.send(Ok(()));
                        while !thread_stop.load(Ordering::Relaxed) {
                            std::thread::sleep(std::time::Duration::from_millis(50));
                        }
                        drop(streams);
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                    }
                }
            })
            .context("starting the call audio thread")?;
        ready
            .recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|_| anyhow!("the sound devices did not open in time"))??;
        Ok(CallAudio {
            stop,
            muted,
            mic,
            speaker,
        })
    }

    /// Sends silence instead of the microphone, or stops doing so.
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for CallAudio {
    fn drop(&mut self) {
        self.stop();
    }
}

fn open_streams(
    mic_tx: async_channel::Sender<Vec<i16>>,
    speaker_rx: async_channel::Receiver<Vec<i16>>,
    muted: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> Result<(cpal::Stream, Option<cpal::Stream>)> {
    let host = cpal::default_host();

    // ---- microphone -------------------------------------------------------------------
    let input = host
        .default_input_device()
        .ok_or_else(|| anyhow!("no microphone was found"))?;
    let supported = input
        .default_input_config()
        .context("the microphone did not report a format")?;
    let channels = supported.channels().max(1) as usize;
    let mut resampler = Resampler::new(supported.sample_rate(), CALL_RATE);
    let mut pending: Vec<f32> = Vec::with_capacity(CALL_FRAME * 2);
    let mut mono: Vec<f32> = Vec::new();
    let mut feed = move |data: &[f32]| {
        mono.clear();
        mono.extend(data.chunks(channels).map(|frame| frame.iter().sum::<f32>() / frame.len() as f32));
        resampler.push(&mono, &mut pending);
        while pending.len() >= CALL_FRAME {
            let piece: Vec<i16> = if muted.load(Ordering::Relaxed) {
                vec![0; CALL_FRAME]
            } else {
                pending[..CALL_FRAME]
                    .iter()
                    .map(|sample| (sample.clamp(-1., 1.) * 32767.) as i16)
                    .collect()
            };
            pending.drain(..CALL_FRAME);
            // If the call cannot keep up, the oldest sound is not worth keeping.
            let _ = mic_tx.try_send(piece);
        }
    };
    let on_error = |error: cpal::Error| eprintln!("call microphone: {error}");
    let config = supported.config();
    let input_stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            input.build_input_stream(config, move |data: &[f32], _: &_| feed(data), on_error, None)
        }
        cpal::SampleFormat::I16 => input.build_input_stream(
            config,
            move |data: &[i16], _: &_| {
                let floats: Vec<f32> = data.iter().map(|sample| *sample as f32 / 32768.).collect();
                feed(&floats)
            },
            on_error,
            None,
        ),
        other => return Err(anyhow!("the microphone uses a format this app cannot read ({other:?})")),
    }
    .context("could not open the microphone")?;
    input_stream.play().context("could not start the microphone")?;

    // ---- speaker ----------------------------------------------------------------------
    // Test runs must stay silent: what is received is then read and thrown away.
    if std::env::var("WA_MUTE").is_ok_and(|value| !value.is_empty()) {
        std::thread::spawn(move || while speaker_rx.recv_blocking().is_ok() && !stop.load(Ordering::Relaxed) {});
        return Ok((input_stream, None));
    }
    let output = host
        .default_output_device()
        .ok_or_else(|| anyhow!("no speaker was found"))?;
    let supported = output
        .default_output_config()
        .context("the speaker did not report a format")?;
    let out_channels = supported.channels().max(1) as usize;
    let out_rate = supported.sample_rate();
    let queue: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
    {
        // Received sound is brought to the speaker's rate and queued for it.
        let queue = queue.clone();
        let stop = stop.clone();
        std::thread::Builder::new()
            .name("call-playout".into())
            .spawn(move || {
                let mut resampler = Resampler::new(CALL_RATE, out_rate);
                let mut ready: Vec<f32> = Vec::new();
                // At most 300 ms may wait; beyond that the call would lag behind.
                let limit = out_rate as usize * 3 / 10;
                while let Ok(piece) = speaker_rx.recv_blocking() {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let floats: Vec<f32> = piece.iter().map(|sample| *sample as f32 / 32768.).collect();
                    ready.clear();
                    resampler.push(&floats, &mut ready);
                    if let Ok(mut queue) = queue.lock() {
                        queue.extend(ready.iter().copied());
                        let extra = queue.len().saturating_sub(limit);
                        queue.drain(..extra);
                    }
                }
            })
            .context("starting the call playout thread")?;
    }
    let play = move |data: &mut [f32]| {
        let mut queue = match queue.lock() {
            Ok(queue) => queue,
            Err(_) => return,
        };
        for frame in data.chunks_mut(out_channels) {
            let sample = queue.pop_front().unwrap_or(0.);
            frame.fill(sample);
        }
    };
    let on_error = |error: cpal::Error| eprintln!("call speaker: {error}");
    let config = supported.config();
    let output_stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            let play = play;
            output.build_output_stream(config, move |data: &mut [f32], _: &_| play(data), on_error, None)
        }
        cpal::SampleFormat::I16 => {
            let play = play;
            let mut scratch: Vec<f32> = Vec::new();
            output.build_output_stream(
                config,
                move |data: &mut [i16], _: &_| {
                    scratch.resize(data.len(), 0.);
                    play(&mut scratch);
                    for (slot, sample) in data.iter_mut().zip(&scratch) {
                        *slot = (sample.clamp(-1., 1.) * 32767.) as i16;
                    }
                },
                on_error,
                None,
            )
        }
        other => return Err(anyhow!("the speaker uses a format this app cannot write ({other:?})")),
    }
    .context("could not open the speaker")?;
    output_stream.play().context("could not start the speaker")?;
    Ok((input_stream, Some(output_stream)))
}

/// The call in progress.
struct Active {
    handle: CallHandle,
    audio: CallAudio,
}

/// The calls this app knows about.
#[derive(Default)]
pub struct Calls {
    /// Offers that are ringing, by call ID.
    ringing: HashMap<String, IncomingCall>,
    active: Option<Active>,
}

fn calls() -> &'static tokio::sync::Mutex<Calls> {
    static CALLS: OnceLock<tokio::sync::Mutex<Calls>> = OnceLock::new();
    CALLS.get_or_init(Default::default)
}

async fn tell(events: &async_channel::Sender<UiEvent>, update: CallUpdate) {
    let _ = events.send(UiEvent::Call(update)).await;
}

/// Looks at an event from the connection for call signalling: a new offer starts
/// ringing; the caller giving up stops it.
pub async fn on_event(store: &Arc<Store>, events: &async_channel::Sender<UiEvent>, event: &Event) {
    let Event::IncomingCall(incoming) = event else {
        return;
    };
    match &incoming.action {
        CallAction::Offer {
            call_id,
            is_video,
            caller_pn,
            group_jid: None,
            ..
        } => {
            let from = caller_pn.as_ref().unwrap_or(&incoming.from).to_non_ad_string();
            let from = store.canonical(&from).unwrap_or(from);
            let mut calls = calls().lock().await;
            // Already in a call: the new one is left to ring out on the other devices.
            if calls.active.is_some() {
                return;
            }
            calls.ringing.insert(call_id.clone(), (**incoming).clone());
            drop(calls);
            tell(
                events,
                CallUpdate::Ringing {
                    call_id: call_id.clone(),
                    from,
                    video: *is_video,
                },
            )
            .await;
        }
        CallAction::Terminate { call_id, .. } | CallAction::Reject { call_id, .. } => {
            let was_ringing = calls().lock().await.ringing.remove(call_id).is_some();
            if was_ringing {
                tell(
                    events,
                    CallUpdate::Ended {
                        reason: "Missed call".to_string(),
                    },
                )
                .await;
            }
        }
        _ => {}
    }
}

/// Follows a call that has started: reports when sound is flowing and when it ends,
/// and closes the sound devices afterwards.
fn watch(handle: CallHandle, events: async_channel::Sender<UiEvent>) {
    use whatsapp_rust::voip::CallEvent;
    let ended = handle.clone();
    let progress = events.clone();
    if let Some(mut call_events) = handle.take_events() {
        tokio::spawn(async move {
            while let Ok(event) = call_events.recv().await {
                match event {
                    CallEvent::MediaConnected => tell(&progress, CallUpdate::Connected).await,
                    CallEvent::MediaSetupFailed(why) => {
                        let _ = progress
                            .send(UiEvent::Error(format!("The call could not connect: {why}")))
                            .await;
                    }
                    CallEvent::RelayAllocateTimedOut | CallEvent::RelayReconnectTimedOut => {
                        let _ = progress
                            .send(UiEvent::Error("The call lost its connection.".to_string()))
                            .await;
                    }
                    _ => {}
                }
            }
        });
    }
    tokio::spawn(async move {
        ended.wait_ended().await;
        let mut calls = calls().lock().await;
        // Only clear the slot if it still holds this call.
        if calls
            .active
            .as_ref()
            .is_some_and(|active| active.handle.call_id() == ended.call_id())
        {
            calls.active = None;
        }
        drop(calls);
        tell(
            &events,
            CallUpdate::Ended {
                reason: "Call ended".to_string(),
            },
        )
        .await;
    });
}

/// Calls someone. Voice only.
pub async fn start(client: &Arc<Client>, events: &async_channel::Sender<UiEvent>, peer: &str) -> Result<()> {
    let mut calls = calls().lock().await;
    if calls.active.is_some() {
        return Err(anyhow!("there is already a call in progress"));
    }
    let audio = CallAudio::start()?;
    let to = crate::backend::jid(peer)?;
    tell(
        events,
        CallUpdate::Calling {
            peer: peer.to_string(),
        },
    )
    .await;
    let started = client
        .voip()
        .call(&to)
        .audio(audio.mic.clone(), audio.speaker.clone())
        .start()
        .await;
    let handle = match started {
        Ok(handle) => handle,
        Err(error) => {
            drop(calls);
            tell(
                events,
                CallUpdate::Ended {
                    reason: "Call failed".to_string(),
                },
            )
            .await;
            return Err(anyhow!("the call could not be placed: {error}"));
        }
    };
    watch(handle.clone(), events.clone());
    calls.active = Some(Active { handle, audio });
    Ok(())
}

/// Answers a ringing call. Voice only: a video call is answered without a camera.
pub async fn answer(client: &Arc<Client>, events: &async_channel::Sender<UiEvent>, call_id: &str) -> Result<()> {
    let mut calls = calls().lock().await;
    let incoming = calls
        .ringing
        .remove(call_id)
        .ok_or_else(|| anyhow!("that call is no longer ringing"))?;
    let audio = CallAudio::start()?;
    let started = client
        .voip()
        .accept(&incoming)
        .audio(audio.mic.clone(), audio.speaker.clone())
        .start()
        .await;
    let handle = match started {
        Ok(handle) => handle,
        Err(error) => {
            drop(calls);
            tell(
                events,
                CallUpdate::Ended {
                    reason: "Call failed".to_string(),
                },
            )
            .await;
            return Err(anyhow!("the call could not be answered: {error}"));
        }
    };
    watch(handle.clone(), events.clone());
    calls.active = Some(Active { handle, audio });
    tell(events, CallUpdate::Answered).await;
    Ok(())
}

/// Declines a ringing call.
pub async fn decline(client: &Arc<Client>, events: &async_channel::Sender<UiEvent>, call_id: &str) -> Result<()> {
    let incoming = calls().lock().await.ringing.remove(call_id);
    if let Some(incoming) = incoming {
        client
            .voip()
            .reject(&incoming)
            .await
            .map_err(|error| anyhow!("the call could not be declined: {error}"))?;
    }
    tell(
        events,
        CallUpdate::Ended {
            reason: "Call declined".to_string(),
        },
    )
    .await;
    Ok(())
}

/// Ends the call in progress.
pub async fn end(events: &async_channel::Sender<UiEvent>) -> Result<()> {
    let active = calls().lock().await.active.take();
    if let Some(active) = active {
        active.audio.stop();
        let _ = active.handle.terminate().await;
    }
    tell(
        events,
        CallUpdate::Ended {
            reason: "Call ended".to_string(),
        },
    )
    .await;
    Ok(())
}

/// Mutes or unmutes the microphone of the call in progress, and tells the other side.
pub async fn mute(events: &async_channel::Sender<UiEvent>, muted: bool) -> Result<()> {
    let calls = calls().lock().await;
    if let Some(active) = calls.active.as_ref() {
        active.audio.set_muted(muted);
        // Before the call is answered this only takes effect locally; that is fine.
        let _ = active.handle.set_muted(muted).await;
        tell(events, CallUpdate::Muted(muted)).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, seconds: f32, hz: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .map(|i| (i as f32 * hz * std::f32::consts::TAU / rate as f32).sin())
            .collect()
    }

    /// Counts how often the signal crosses zero going up: its pitch, roughly.
    fn cycles(samples: &[f32]) -> usize {
        samples.windows(2).filter(|pair| pair[0] <= 0. && pair[1] > 0.).count()
    }

    #[test]
    fn resampling_in_pieces_keeps_length_and_pitch() {
        for (from, to) in [(48_000u32, 16_000u32), (44_100, 16_000), (16_000, 48_000), (16_000, 44_100)] {
            let input = tone(from, 1., 300.);
            let mut resampler = Resampler::new(from, to);
            let mut out = Vec::new();
            // Uneven pieces, the way a sound device delivers them.
            for piece in input.chunks(441) {
                resampler.push(piece, &mut out);
            }
            let expected = to as i64;
            assert!(
                (out.len() as i64 - expected).abs() < 40,
                "{from}->{to}: {} samples, expected about {expected}",
                out.len()
            );
            let pitch = cycles(&out) as i64;
            assert!((pitch - 300).abs() <= 3, "{from}->{to}: pitch {pitch}");
        }
    }
}
