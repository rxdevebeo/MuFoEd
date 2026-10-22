//! In-process Windows media playback. No helper executable or network service.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)] // Windows uses 100ns integer timestamps.
#![allow(unsafe_code)] // Only the balanced WinRT apartment initialization uses FFI.
use std::{
    marker::PhantomData,
    path::Path,
    rc::Rc,
    sync::{Arc, Mutex},
};
use windows::{
    core::{Result, HSTRING},
    Foundation::{TimeSpan, TypedEventHandler, Uri},
    Media::{
        Core::MediaSource,
        Playback::{MediaPlayer, MediaPlayerFailedEventArgs},
    },
    Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
};

struct Apartment {
    initialized: bool,
    _thread: PhantomData<Rc<()>>,
}
impl Apartment {
    fn new() -> Result<Self> {
        // SAFETY: called on the UI thread. Successful initialization is balanced
        // on this same thread. An existing STA can also use agile MediaPlayer.
        let initialized = match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
            Ok(()) => true,
            Err(error) if error.code() == windows::core::HRESULT(-2_147_417_850) => false,
            Err(error) => return Err(error),
        };
        Ok(Self {
            initialized,
            _thread: PhantomData,
        })
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: this guard cannot leave its creating thread and owns one initialization.
            unsafe {
                RoUninitialize();
            }
        }
    }
}

pub(crate) struct Audio {
    player: MediaPlayer,
    failure: Arc<Mutex<Option<String>>>,
    seek: Option<f64>,
    _apartment: Apartment,
}
impl Audio {
    pub(crate) fn new(path: &Path, muted: bool) -> Result<Self> {
        let apartment = Apartment::new()?;
        let mut native = Self {
            player: MediaPlayer::new()?,
            failure: Arc::default(),
            seek: None,
            _apartment: apartment,
        };
        native.player.SetAutoPlay(false)?;
        native.player.SetIsMuted(muted)?;
        native.player.CommandManager()?.SetIsEnabled(false)?;
        let failure = native.failure.clone();
        native.player.MediaFailed(
            &TypedEventHandler::<MediaPlayer, MediaPlayerFailedEventArgs>::new(move |_, args| {
                let message = args
                    .as_ref()
                    .and_then(|a| a.ErrorMessage().ok())
                    .map_or_else(|| "Windows media playback failed".into(), |s| s.to_string());
                *failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(message);
                Ok(())
            }),
        )?;
        let path = url::Url::from_file_path(path).map_err(|()| {
            windows::core::Error::from_hresult(windows::core::HRESULT(-2_147_024_809))
        })?;
        let uri = Uri::CreateUri(&HSTRING::from(path.as_str()))?;
        native
            .player
            .SetSource(&MediaSource::CreateFromUri(&uri)?)?;
        native.seek = Some(0.0);
        Ok(native)
    }
    pub(crate) fn play(&mut self, seconds: f64, muted: bool) -> Result<()> {
        self.player.SetIsMuted(muted)?;
        self.seek = Some(seconds.max(0.0));
        self.apply_seek()?;
        self.player.Play()
    }
    fn apply_seek(&mut self) -> Result<()> {
        if let Some(seconds) = self.seek {
            let session = self.player.PlaybackSession()?;
            if session.CanSeek()? {
                session.SetPosition(TimeSpan {
                    Duration: (seconds * 10_000_000.0) as i64,
                })?;
                self.seek = None;
            }
        }
        Ok(())
    }
    pub(crate) fn pause(&self) {
        let _ = self.player.Pause();
    }
    pub(crate) fn position(&self) -> Option<f64> {
        self.player
            .PlaybackSession()
            .ok()?
            .Position()
            .ok()
            .map(|p| p.Duration as f64 / 10_000_000.0)
    }
    pub(crate) fn poll(&mut self) -> std::result::Result<f64, String> {
        if let Some(error) = self
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            return Err(error);
        }
        self.apply_seek().map_err(|e| e.to_string())?;
        let duration = self
            .player
            .PlaybackSession()
            .and_then(|s| s.NaturalDuration())
            .map_err(|e| e.to_string())?;
        Ok(duration.Duration as f64 / 10_000_000.0)
    }
    #[cfg(test)]
    pub(crate) fn handle(&self) -> MediaPlayer {
        self.player.clone()
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        let _ = self.player.Pause();
        let _ = self.player.Close();
    }
}

#[cfg(test)]
pub(crate) fn silent_wav(path: &Path) {
    let data_size = 16000_u32 * 2 * 3;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&16000_u32.to_le_bytes());
    bytes.extend_from_slice(&32000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    bytes.resize(44 + data_size as usize, 0);
    std::fs::write(path, bytes).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn native_audio_play_seek_pause_and_close() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("silent audio.wav");
        silent_wav(&path);
        let mut audio = Audio::new(&path, true).unwrap();
        let handle = audio.handle();
        audio.play(0.0, true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while audio.poll().unwrap() < 2.9 || audio.position().unwrap_or(0.0) < 0.05 {
            assert!(Instant::now() < deadline, "native playback timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        audio.pause();
        audio.play(1.0, true).unwrap();
        assert!(audio.position().unwrap() >= 0.9);
        audio.pause();
        drop(audio);
        assert!(
            handle.PlaybackSession().is_err(),
            "closed player must release session"
        );
    }
}
