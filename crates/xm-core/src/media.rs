//! Media transport control, so the model can pause and resume playback without knowing how.

pub trait MediaControl: Send {
    /// Pauses playback; true only if something was playing and is now paused.
    fn pause(&self) -> bool;
    /// Resumes playback; true only if the command took effect.
    fn play(&self) -> bool;
}

pub struct NullMediaControl;

impl MediaControl for NullMediaControl {
    fn pause(&self) -> bool {
        false
    }

    fn play(&self) -> bool {
        false
    }
}

/// Controls whatever app Windows currently treats as the active media session.
/// Calls block on WinRT, so use it off the UI thread.
#[cfg(windows)]
pub struct GsmtcMediaControl;

#[cfg(windows)]
impl GsmtcMediaControl {
    fn session(
    ) -> windows::core::Result<windows::Media::Control::GlobalSystemMediaTransportControlsSession>
    {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as Manager;
        Manager::RequestAsync()?.join()?.GetCurrentSession()
    }

    fn try_pause() -> windows::core::Result<bool> {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status;
        let s = Self::session()?;
        if s.GetPlaybackInfo()?.PlaybackStatus()? != Status::Playing {
            return Ok(false);
        }
        s.TryPauseAsync()?.join()
    }
}

#[cfg(windows)]
const WINRT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

#[cfg(windows)]
impl MediaControl for GsmtcMediaControl {
    fn pause(&self) -> bool {
        run_bounded(WINRT_TIMEOUT, || Self::try_pause().unwrap_or(false))
    }

    fn play(&self) -> bool {
        run_bounded(WINRT_TIMEOUT, || {
            Self::session()
                .and_then(|s| s.TryPlayAsync()?.join())
                .unwrap_or(false)
        })
    }
}

/// Runs `f` on a short-lived thread so a stalled media service cannot block the caller;
/// on timeout it returns false and the worker is left to finish (or hang) on its own.
fn run_bounded(timeout: std::time::Duration, f: impl FnOnce() -> bool + Send + 'static) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(timeout).unwrap_or_else(|_| {
        crate::log_info!("media control timed out after {timeout:?}");
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn run_bounded_returns_the_result_or_false_on_timeout() {
        assert!(run_bounded(Duration::from_secs(1), || true));
        let t = Instant::now();
        let slow = || {
            std::thread::sleep(Duration::from_millis(500));
            true
        };
        assert!(!run_bounded(Duration::from_millis(50), slow));
        assert!(t.elapsed() < Duration::from_millis(400));
    }
}
