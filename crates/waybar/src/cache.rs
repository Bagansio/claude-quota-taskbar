//! State shared by every running instance (Waybar starts one per bar/monitor), so the
//! rate-limited endpoint is polled once per interval no matter how many bars there are.
//!
//! Lives in `$XDG_RUNTIME_DIR/claude-quota-waybar/` (tmpfs, per user, 0700). It only ever holds
//! percentages, reset times and error messages — never the token.

use claude_quota_core::model::{Limit, Usage};
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct Shared {
    /// Unix seconds of the last fetch attempt.
    pub fetched_at: i64,
    /// Unix seconds when the next fetch is due.
    pub next_at: i64,
    /// Last good reading (kept while later fetches fail).
    pub usage: Option<Usage>,
    /// Error from the most recent fetch, if it failed.
    pub error: Option<String>,
}

fn limit_json(l: &Limit) -> Value {
    json!({ "percent": l.percent, "resets_at": l.resets_at })
}

fn limit_from(v: &Value) -> Option<Limit> {
    Some(Limit { percent: v.get("percent")?.as_f64()?, resets_at: v.get("resets_at").and_then(Value::as_i64) })
}

impl Shared {
    pub fn to_json(&self) -> String {
        json!({
            "fetched_at": self.fetched_at,
            "next_at": self.next_at,
            "usage": self.usage.map(|u| json!({ "session": limit_json(&u.session), "weekly": limit_json(&u.weekly) })),
            "error": self.error,
        })
        .to_string()
    }

    pub fn from_json(s: &str) -> Option<Shared> {
        let v: Value = serde_json::from_str(s).ok()?;
        let usage = v.get("usage").filter(|u| u.is_object()).and_then(|u| {
            Some(Usage { session: limit_from(u.get("session")?)?, weekly: limit_from(u.get("weekly")?)? })
        });
        Some(Shared {
            fetched_at: v.get("fetched_at")?.as_i64()?,
            next_at: v.get("next_at")?.as_i64()?,
            usage,
            error: v.get("error").and_then(Value::as_str).map(str::to_owned),
        })
    }
}

pub fn dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("claude-quota-waybar")
}

fn ensure_dir() -> std::io::Result<PathBuf> {
    let d = dir();
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&d)?;
    Ok(d)
}

pub fn load() -> Option<Shared> {
    Shared::from_json(&fs::read_to_string(dir().join("state.json")).ok()?)
}

/// Atomic replace, so readers never see a half-written file.
pub fn save(s: &Shared) -> std::io::Result<()> {
    let d = ensure_dir()?;
    let tmp = d.join(format!("state.json.{}", std::process::id()));
    let mut f = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    f.write_all(s.to_json().as_bytes())?;
    fs::rename(&tmp, d.join("state.json"))
}

/// Asks every running instance to refetch (`--refresh`, e.g. from `on-click`).
pub fn request_refresh() -> std::io::Result<()> {
    let d = ensure_dir()?;
    OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(d.join("refresh")).map(|_| ())
}

pub fn refresh_requested() -> bool {
    dir().join("refresh").exists()
}

pub fn clear_refresh() {
    let _ = fs::remove_file(dir().join("refresh"));
}

/// Exclusive "I am fetching" marker; removed on drop. A lock older than a minute is from a
/// process that died mid-fetch and is taken over.
pub struct Lock(PathBuf);

impl Lock {
    pub fn acquire() -> Option<Lock> {
        let p = ensure_dir().ok()?.join("lock");
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).mode(0o600).open(&p) {
                Ok(_) => return Some(Lock(p)),
                Err(_) => {
                    let stale = File::open(&p)
                        .and_then(|f| f.metadata())
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > Duration::from_secs(60));
                    if !stale {
                        return None;
                    }
                    let _ = fs::remove_file(&p);
                }
            }
        }
        None
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let u = Usage {
            session: Limit { percent: 36.5, resets_at: Some(1793483999) },
            weekly: Limit { percent: 3.0, resets_at: None },
        };
        let s = Shared { fetched_at: 10, next_at: 190, usage: Some(u), error: Some("offline".into()) };
        assert_eq!(Shared::from_json(&s.to_json()), Some(s));
        let e = Shared { fetched_at: 1, next_at: 2, usage: None, error: None };
        assert_eq!(Shared::from_json(&e.to_json()), Some(e));
        assert_eq!(Shared::from_json("garbage"), None);
    }
}
