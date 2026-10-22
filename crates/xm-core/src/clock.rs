//! Time source, injectable so session timing can be tested without sleeping.

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    /// Waits for `d`; a fake clock jumps its own time instead of blocking.
    fn sleep(&self, d: Duration);
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// A clock that only moves when told to.
pub struct FakeClock(Mutex<Instant>);

impl FakeClock {
    pub fn new() -> Self {
        Self(Mutex::new(Instant::now()))
    }

    pub fn advance(&self, d: Duration) {
        *self.0.lock().unwrap() += d;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }

    fn sleep(&self, d: Duration) {
        self.advance(d);
        std::thread::yield_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_advances_only_on_request() {
        let c = FakeClock::new();
        let t0 = c.now();
        assert_eq!(c.now(), t0);
        c.advance(Duration::from_secs(3));
        assert_eq!(c.now() - t0, Duration::from_secs(3));
    }
}
