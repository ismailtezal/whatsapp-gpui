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
