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

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
