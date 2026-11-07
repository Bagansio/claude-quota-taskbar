//! Shared, platform-independent logic for the Claude quota frontends.
//!
//! Frontends (Windows taskbar widget, Waybar module) only supply an HTTP GET and a way to draw;
//! everything else (credentials, status handling, parsing, formatting, polling policy) lives here.

pub mod fetch;
pub mod model;

/// Poll gently: the endpoint is undocumented and rate-limits aggressively.
pub const POLL_OK_SECS: u64 = 180;
pub const POLL_RATE_LIMITED_SECS: u64 = 300;
pub const POLL_RETRY_SECS: u64 = 60;
/// Manual refreshes are honoured at most this often.
pub const MIN_MANUAL_REFRESH_SECS: u64 = 15;
/// Right after a wake (resume from suspend, session unlock) the network is often still coming
/// up: failed fetches are retried this often for `WAKE_WINDOW_SECS` instead of `POLL_RETRY_SECS`.
pub const WAKE_RETRY_SECS: u64 = 5;
pub const WAKE_WINDOW_SECS: i64 = 90;

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Detects a resume from suspend by comparing the wall clock with the monotonic clock, which
/// stops while the machine sleeps. Call `check()` regularly (every second or so) from a loop.
pub struct WakeDetector {
    wall_ms: i64,
    mono: std::time::Instant,
}

impl Default for WakeDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl WakeDetector {
    pub fn new() -> Self {
        WakeDetector { wall_ms: now_ms(), mono: std::time::Instant::now() }
    }

    /// True if the machine slept since the previous call.
    pub fn check(&mut self) -> bool {
        let (wall_ms, mono) = (now_ms(), std::time::Instant::now());
        let jumped = slept(wall_ms - self.wall_ms, mono.duration_since(self.mono).as_millis() as i64);
        (self.wall_ms, self.mono) = (wall_ms, mono);
        jumped
    }
}

/// The wall clock ran at least 10 s ahead of the monotonic clock.
fn slept(wall_delta_ms: i64, mono_delta_ms: i64) -> bool {
    wall_delta_ms - mono_delta_ms > 10_000
}

#[cfg(test)]
mod tests {
    #[test]
    fn slept() {
        assert!(!super::slept(1_000, 1_000));
        assert!(!super::slept(16_000, 15_000), "a slow fetch is not a suspend");
        assert!(super::slept(3_600_000, 1_000));
        assert!(!super::slept(-3_600_000, 1_000), "clock set backwards");
        assert!(!super::WakeDetector::new().check());
    }
}
