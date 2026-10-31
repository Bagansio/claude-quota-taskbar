//! Platform-independent logic: parsing the usage payload, time maths, formatting.
//! Everything here is unit-tested (`cargo test --target x86_64-unknown-linux-gnu`).

use serde_json::Value;

pub const SEGMENTS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limit {
    /// 0..=100
    pub percent: f64,
    /// Unix seconds when the window resets.
    pub resets_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Usage {
    pub session: Limit,
    pub weekly: Limit,
}

pub struct Credentials {
    pub access_token: String,
    pub expires_at_ms: Option<i64>,
}

pub fn parse_credentials(json: &str) -> Result<Credentials, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("credentials: {e}"))?;
    let o = v.get("claudeAiOauth").ok_or("not logged in to Claude Code")?;
    let access_token = o
        .get("accessToken")
        .and_then(Value::as_str)
        .ok_or("no access token")?
        .to_owned();
    Ok(Credentials { access_token, expires_at_ms: o.get("expiresAt").and_then(Value::as_i64) })
}

pub fn parse_usage(body: &str) -> Result<Usage, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("bad response: {e}"))?;
    let session = limit(&v, "five_hour", "session").ok_or("no session limit in response")?;
    let weekly = limit(&v, "seven_day", "weekly_all").ok_or("no weekly limit in response")?;
    Ok(Usage { session, weekly })
}

/// Reads `top[key]` ({utilization, resets_at}), falling back to the `limits[]` array.
fn limit(v: &Value, key: &str, kind: &str) -> Option<Limit> {
    if let Some(o) = v.get(key).filter(|o| o.is_object()) {
        if let Some(p) = o.get("utilization").and_then(Value::as_f64) {
            return Some(Limit { percent: p, resets_at: o.get("resets_at").and_then(Value::as_str).and_then(parse_rfc3339) });
        }
    }
    v.get("limits")?.as_array()?.iter().find(|l| l["kind"] == kind).and_then(|l| {
        Some(Limit {
            percent: l.get("percent")?.as_f64()?,
            resets_at: l.get("resets_at").and_then(Value::as_str).and_then(parse_rfc3339),
        })
    })
}

/// Minimal RFC 3339 -> unix seconds ("2026-10-31T21:59:59.859430+00:00", "...Z").
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    let mut rest = &s[19..];
    if let Some(r) = rest.strip_prefix('.') {
        rest = r.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest.as_bytes().first() {
        None | Some(b'Z') | Some(b'z') => 0,
        Some(&sign @ (b'+' | b'-')) => {
            let oh: i64 = rest.get(1..3)?.parse().ok()?;
            let om: i64 = rest.get(4..6).and_then(|x| x.parse().ok()).unwrap_or(0);
            let o = oh * 3600 + om * 60;
            if sign == b'+' { o } else { -o }
        }
        _ => return None,
    };
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se - offset)
}

/// Howard Hinnant's days_from_civil.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// "3d", "3h", "42m" — coarse on purpose, like a status bar.
pub fn format_remaining(secs: i64) -> String {
    let secs = secs.max(0);
    if secs >= 86400 {
        format!("{}d", secs / 86400)
    } else if secs >= 3600 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}m", secs / 60)
    }
}

/// Text on the right of a bar: "36% · 3h".
pub fn format_limit(l: &Limit, now: i64) -> String {
    match l.resets_at {
        Some(t) => format!("{:.0}% · {}", l.percent, format_remaining(t - now)),
        None => format!("{:.0}%", l.percent),
    }
}

/// How full each of the `SEGMENTS` segments is (0.0..=1.0).
pub fn segment_fills(percent: f64) -> [f64; SEGMENTS] {
    let filled = (percent / 100.0 * SEGMENTS as f64).clamp(0.0, SEGMENTS as f64);
    let mut out = [0.0; SEGMENTS];
    for (i, f) in out.iter_mut().enumerate() {
        *f = (filled - i as f64).clamp(0.0, 1.0);
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    Normal,
    Warn,
    Critical,
}

pub fn level(percent: f64) -> Level {
    if percent >= 90.0 {
        Level::Critical
    } else if percent >= 70.0 {
        Level::Warn
    } else {
        Level::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2026-10-31T21:59:59.859430+00:00"), Some(1793483999));
        assert_eq!(parse_rfc3339("2026-10-31T23:59:59+02:00"), Some(1793483999));
        assert_eq!(parse_rfc3339("garbage"), None);
    }

    #[test]
    fn remaining() {
        assert_eq!(format_remaining(-5), "0m");
        assert_eq!(format_remaining(59 * 60), "59m");
        assert_eq!(format_remaining(3 * 3600 + 1799), "3h");
        assert_eq!(format_remaining(3 * 86400 + 5), "3d");
    }

    #[test]
    fn fills() {
        let f = segment_fills(36.0);
        assert_eq!(f[..3], [1.0, 1.0, 1.0]);
        assert!((f[3] - 0.6).abs() < 1e-9);
        assert_eq!(f[4..], [0.0; 6]);
        assert_eq!(segment_fills(150.0), [1.0; SEGMENTS]);
        assert_eq!(segment_fills(-3.0), [0.0; SEGMENTS]);
    }

    #[test]
    fn levels() {
        assert_eq!(level(69.9), Level::Normal);
        assert_eq!(level(70.0), Level::Warn);
        assert_eq!(level(90.0), Level::Critical);
    }

    #[test]
    fn usage_top_level_and_fallback() {
        let a = r#"{"five_hour":{"utilization":8.0,"resets_at":"2026-10-31T21:59:59+00:00"},
                    "seven_day":{"utilization":1.0,"resets_at":null}}"#;
        let u = parse_usage(a).unwrap();
        assert_eq!(u.session.percent, 8.0);
        assert_eq!(u.weekly.resets_at, None);
        let b = r#"{"limits":[{"kind":"session","percent":25,"resets_at":"2026-10-31T21:59:59Z"},
                              {"kind":"weekly_all","percent":3,"resets_at":"2026-11-01T10:59:59Z"}]}"#;
        let u = parse_usage(b).unwrap();
        assert_eq!((u.session.percent, u.weekly.percent), (25.0, 3.0));
        assert!(parse_usage("{}").is_err());
    }

    #[test]
    fn credentials() {
        let c = parse_credentials(r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":123}}"#).unwrap();
        assert_eq!((c.access_token.as_str(), c.expires_at_ms), ("t", Some(123)));
        assert!(parse_credentials("{}").is_err());
    }
}
