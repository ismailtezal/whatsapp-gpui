//! Playback of voice messages and audio files through the Windows media player.
//!
//! One player for the whole app: starting a clip stops the previous one.

use std::path::Path;

use anyhow::{Context, Result};
use windows::Foundation::Uri;
use windows::Media::Core::MediaSource;
use windows::Media::Playback::{MediaPlaybackState, MediaPlayer};
use windows::core::HSTRING;

pub struct AudioPlayer {
    player: MediaPlayer,
    /// ID of the message being played.
    current: Option<String>,
}

/// Where playback stands, for drawing the progress bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Playback {
    pub id: String,
    pub playing: bool,
    pub position_secs: f64,
    pub duration_secs: f64,
}

/// Where the sound of the current clip stands, readable from another thread, so that a
/// video can keep its pictures in step with it.
#[derive(Clone)]
pub struct SoundClock {
    player: MediaPlayer,
}

/// What a [`SoundClock`] reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SoundState {
    /// The clip is being opened; nothing is heard yet.
    Starting,
    /// Playing, at this many seconds.
    Playing(f64),
    /// Not moving (paused, or at its end), at this many seconds.
    Paused(f64),
    /// There is no sound to follow.
    Absent,
}

impl SoundClock {
    pub fn state(&self) -> SoundState {
        let Ok(session) = self.player.PlaybackSession() else {
            return SoundState::Absent;
        };
        let at = session
            .Position()
            .map(|span| span.Duration as f64 / 10_000_000.)
            .unwrap_or(0.);
        match session.PlaybackState() {
            Ok(MediaPlaybackState::Playing) => SoundState::Playing(at),
            Ok(MediaPlaybackState::Paused) => SoundState::Paused(at),
            Ok(MediaPlaybackState::Opening) | Ok(MediaPlaybackState::Buffering) => SoundState::Starting,
            _ => SoundState::Absent,
        }
    }
}

/// Whether this run must stay silent (`WA_MUTE` is set).
fn silenced() -> bool {
    std::env::var("WA_MUTE").is_ok_and(|value| !value.is_empty())
}

fn file_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from("file:///");
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'.' | b'-' | b'_' | b'~' => {
                uri.push(byte as char)
            }
            other => uri.push_str(&format!("%{other:02X}")),
        }
    }
    uri
}

impl AudioPlayer {
    pub fn new() -> Result<Self> {
        let player = MediaPlayer::new().context("creating the media player")?;
        // Test runs set WA_MUTE so nothing comes out of the speakers.
        if silenced() {
            let _ = player.SetIsMuted(true);
        }
        Ok(AudioPlayer {
            player,
            current: None,
        })
    }

    /// Starts a clip from the beginning.
    pub fn play(&mut self, id: &str, path: &Path) -> Result<()> {
        let uri = Uri::CreateUri(&HSTRING::from(file_uri(path))).context("building the file address")?;
        let source = MediaSource::CreateFromUri(&uri).context("opening the audio file")?;
        self.player.SetSource(&source)?;
        self.player.Play()?;
        self.current = Some(id.to_string());
        Ok(())
    }

    /// A handle another thread can read this player's position through.
    pub fn clock(&self) -> SoundClock {
        SoundClock {
            player: self.player.clone(),
        }
    }

    /// Pauses or resumes the current clip. Returns whether it is now playing.
    pub fn toggle(&mut self) -> Result<bool> {
        if self.is_playing() {
            self.player.Pause()?;
            Ok(false)
        } else {
            self.player.Play()?;
            Ok(true)
        }
    }

    /// Jumps to a position in the current clip, in seconds.
    pub fn seek(&mut self, seconds: f64) -> Result<()> {
        let session = self.player.PlaybackSession()?;
        session.SetPosition(windows::Foundation::TimeSpan {
            Duration: (seconds.max(0.) * 10_000_000.) as i64,
        })?;
        Ok(())
    }

    /// Plays faster or slower: 1 is normal speed.
    pub fn set_rate(&mut self, rate: f64) {
        if let Ok(session) = self.player.PlaybackSession() {
            let _ = session.SetPlaybackRate(rate);
        }
    }

    pub fn rate(&self) -> f64 {
        self.player
            .PlaybackSession()
            .and_then(|session| session.PlaybackRate())
            .unwrap_or(1.)
    }

    /// Turns the sound off or back on. Stays off in test runs.
    pub fn set_muted(&mut self, muted: bool) {
        let _ = self.player.SetIsMuted(muted || silenced());
    }

    pub fn is_muted(&self) -> bool {
        self.player.IsMuted().unwrap_or(false)
    }

    pub fn stop(&mut self) {
        let _ = self.player.Pause();
        self.current = None;
    }

    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    pub fn is_playing(&self) -> bool {
        self.player
            .PlaybackSession()
            .and_then(|session| session.PlaybackState())
            .is_ok_and(|state| {
                state == MediaPlaybackState::Playing
                    || state == MediaPlaybackState::Opening
                    || state == MediaPlaybackState::Buffering
            })
    }

    pub fn playback(&self) -> Option<Playback> {
        let id = self.current.clone()?;
        let session = self.player.PlaybackSession().ok()?;
        // Time spans are in units of 100 nanoseconds.
        let seconds = |span: windows::Foundation::TimeSpan| span.Duration as f64 / 10_000_000.;
        let position_secs = session.Position().map(seconds).unwrap_or(0.);
        let duration_secs = session.NaturalDuration().map(seconds).unwrap_or(0.);
        Some(Playback {
            id,
            playing: self.is_playing(),
            position_secs,
            duration_secs,
        })
    }

    /// Whether the current clip has reached its end.
    pub fn finished(&self) -> bool {
        self.playback()
            .is_some_and(|p| !p.playing && p.duration_secs > 0. && p.position_secs >= p.duration_secs - 0.05)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_addresses_are_escaped() {
        assert_eq!(
            file_uri(Path::new(r"C:\Users\me\My Files\a#1.ogg")),
            "file:///C:/Users/me/My%20Files/a%231.ogg"
        );
    }
}
