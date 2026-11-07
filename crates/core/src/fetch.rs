//! Credentials lookup and response handling, independent of the HTTP stack.
//!
//! Each frontend passes in its own `http_get` (WinHTTP on Windows, curl on Linux) so TLS and proxy
//! settings come from the system and no HTTP crate is needed.

use crate::model::{self, Usage};
use crate::{POLL_OK_SECS, POLL_RATE_LIMITED_SECS, POLL_RETRY_SECS, WAKE_RETRY_SECS, WAKE_WINDOW_SECS};
use std::path::PathBuf;

pub const HOST: &str = "api.anthropic.com";
pub const PATH: &str = "/api/oauth/usage";
pub const BETA_HEADER: (&str, &str) = ("anthropic-beta", "oauth-2025-04-20");

#[derive(Clone, Debug, PartialEq)]
pub enum FetchError {
    /// Token missing/expired: the user must run Claude Code once.
    Auth(String),
    RateLimited,
    Other(String),
}

impl FetchError {
    pub fn message(&self) -> String {
        match self {
            FetchError::Auth(m) | FetchError::Other(m) => m.clone(),
            FetchError::RateLimited => "rate limited, retrying".into(),
        }
    }
}

/// Seconds to wait before the next poll after this outcome. `since_wake` is how long ago the
/// machine resumed or the session was unlocked, if known.
pub fn next_poll_secs(r: &Result<Usage, FetchError>, since_wake: Option<i64>) -> u64 {
    match r {
        Ok(_) => POLL_OK_SECS,
        Err(FetchError::RateLimited) => POLL_RATE_LIMITED_SECS,
        // Offline/HTTP errors right after a wake: the network is probably reconnecting.
        Err(FetchError::Other(_)) if since_wake.is_some_and(|s| (0..WAKE_WINDOW_SECS).contains(&s)) => WAKE_RETRY_SECS,
        Err(_) => POLL_RETRY_SECS,
    }
}

/// Where Claude Code keeps its OAuth credentials: `$CLAUDE_CONFIG_DIR/.credentials.json`,
/// else `~/.claude/.credentials.json` (`%USERPROFILE%` on Windows, `$HOME` elsewhere).
pub fn credentials_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir).join(".credentials.json"));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    Some(PathBuf::from(home).join(".claude").join(".credentials.json"))
}

/// Reads the token Claude Code stored (never writes or refreshes it), then calls `http_get`
/// with the bearer token. `http_get` returns `(status, body)` or a short error like "offline".
pub fn fetch_usage<F>(http_get: F) -> Result<Usage, FetchError>
where
    F: FnOnce(&str) -> Result<(u32, String), String>,
{
    let path = credentials_path().ok_or_else(|| FetchError::Auth("no home directory".into()))?;
    let text = std::fs::read_to_string(&path).map_err(|_| FetchError::Auth("Claude Code not logged in".into()))?;
    let creds = model::parse_credentials(&text).map_err(FetchError::Auth)?;
    if creds.expires_at_ms.is_some_and(|e| e <= crate::now_secs() * 1000) {
        return Err(FetchError::Auth("token expired, run claude".into()));
    }
    let (status, body) = http_get(&creds.access_token).map_err(FetchError::Other)?;
    classify(status, &body)
}

/// Maps an HTTP response to a result.
pub fn classify(status: u32, body: &str) -> Result<Usage, FetchError> {
    match status {
        200 => model::parse_usage(body).map_err(FetchError::Other),
        401 | 403 => Err(FetchError::Auth("token rejected, run claude".into())),
        429 => Err(FetchError::RateLimited),
        s => Err(FetchError::Other(format!("HTTP {s}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping() {
        let ok = r#"{"five_hour":{"utilization":8.0,"resets_at":null},"seven_day":{"utilization":1.0,"resets_at":null}}"#;
        assert!(classify(200, ok).is_ok());
        assert!(matches!(classify(401, ""), Err(FetchError::Auth(_))));
        assert_eq!(classify(429, ""), Err(FetchError::RateLimited));
        assert_eq!(classify(500, ""), Err(FetchError::Other("HTTP 500".into())));
        assert_eq!(next_poll_secs(&classify(429, ""), None), POLL_RATE_LIMITED_SECS);
        assert_eq!(next_poll_secs(&classify(200, ok), Some(1)), POLL_OK_SECS);
    }

    #[test]
    fn retries_fast_after_wake() {
        let offline: Result<Usage, FetchError> = Err(FetchError::Other("offline".into()));
        assert_eq!(next_poll_secs(&offline, None), POLL_RETRY_SECS);
        assert_eq!(next_poll_secs(&offline, Some(3)), WAKE_RETRY_SECS);
        assert_eq!(next_poll_secs(&offline, Some(WAKE_WINDOW_SECS)), POLL_RETRY_SECS);
        assert_eq!(next_poll_secs(&classify(429, ""), Some(3)), POLL_RATE_LIMITED_SECS, "never hammer a 429");
        let auth = Err(FetchError::Auth("token expired, run claude".into()));
        assert_eq!(next_poll_secs(&auth, Some(3)), POLL_RETRY_SECS, "waiting won't fix auth");
    }
}
