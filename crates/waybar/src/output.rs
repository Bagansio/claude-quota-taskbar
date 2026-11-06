//! Waybar custom-module JSON (`"return-type": "json"`), one object per line:
//!
//! ```text
//! 5h ■■■■□□□□□□ 36% · 3h   7d ■■□□□□□□□□ 23% · 3d
//! ```
//! `text` and `tooltip` are Pango markup. `class` is `ok`/`warn`/`critical` (highest of the two
//! limits) plus `stale` while the latest refresh fails, or `error`/`loading` with no data yet.

use crate::cache::Shared;
use claude_quota_core::model::{self, Level, Limit};
use serde_json::json;

#[derive(Clone, Copy, Debug, Default)]
pub struct Style {
    /// Hide the segment bars: `5h 36% · 3h  7d 23% · 3d`.
    pub compact: bool,
    /// No Pango colours, ■ filled / □ empty; style everything from CSS instead.
    pub plain: bool,
}

const ORANGE: &str = "#e8845c";
const AMBER: &str = "#e0a030";
const RED: &str = "#d9473f";
const EMPTY: &str = "#4a4a4a";
const LABEL: &str = "#969696";

fn level_name(l: Level) -> &'static str {
    match l {
        Level::Normal => "ok",
        Level::Warn => "warn",
        Level::Critical => "critical",
    }
}

fn fill_color(l: Level) -> &'static str {
    match l {
        Level::Normal => ORANGE,
        Level::Warn => AMBER,
        Level::Critical => RED,
    }
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn span(color: &str, s: &str, plain: bool) -> String {
    if plain || s.is_empty() {
        s.to_owned()
    } else {
        format!("<span color='{color}'>{s}</span>")
    }
}

fn bar(l: &Limit, style: Style) -> String {
    // Whole segments only: a text cell can't be half filled.
    let filled = model::segment_fills(l.percent).iter().filter(|f| **f >= 0.5).count();
    let empty = model::SEGMENTS - filled;
    let (on, off) = ("■".repeat(filled), if style.plain { "□" } else { "■" }.repeat(empty));
    format!("{}{}", span(fill_color(model::level(l.percent)), &on, style.plain), span(EMPTY, &off, style.plain))
}

fn row(label: &str, l: &Limit, now: i64, style: Style) -> String {
    let mut s = span(LABEL, label, style.plain);
    if !style.compact {
        s.push(' ');
        s.push_str(&bar(l, style));
    }
    s.push(' ');
    s.push_str(&escape(&model::format_limit(l, now)));
    s
}

fn tooltip_row(name: &str, l: &Limit, now: i64) -> String {
    let resets = match l.resets_at {
        Some(t) => format!("resets in {}", model::format_remaining_long(t - now)),
        None => "no reset scheduled".into(),
    };
    format!("{name}  <b>{:.0}%</b> used · {resets}", l.percent)
}

pub fn render(shared: Option<&Shared>, now: i64, style: Style) -> String {
    let Some(sh) = shared else {
        return line("Claude …", "Claude usage: loading…", &["loading"], 0);
    };
    let error = sh.error.as_deref().map(escape);
    let Some(u) = sh.usage else {
        let msg = error.unwrap_or_else(|| "loading…".into());
        let class = if sh.error.is_some() { "error" } else { "loading" };
        return line(&format!("Claude: {msg}"), &format!("<b>Claude usage</b>\n{msg}"), &[class], 0);
    };

    let text = format!("{}   {}", row("5h", &u.session, now, style), row("7d", &u.weekly, now, style));
    let mut tip = format!(
        "<b>Claude usage</b>\n<tt>{}\n{}</tt>\nupdated {} ago",
        tooltip_row("5h session", &u.session, now),
        tooltip_row("7d weekly ", &u.weekly, now),
        model::format_remaining_long(now - sh.fetched_at),
    );
    if let Some(e) = &error {
        tip.push_str(&format!("\n<span color='{AMBER}'>refresh failing: {e}</span>"));
    }
    tip.push_str("\n<small>click to refresh</small>");

    let worst = model::level(u.session.percent.max(u.weekly.percent));
    let mut class = vec![level_name(worst)];
    if error.is_some() {
        class.push("stale");
    }
    let pct = u.session.percent.max(u.weekly.percent).clamp(0.0, 100.0).round() as u32;
    line(&text, &tip, &class, pct)
}

fn line(text: &str, tooltip: &str, class: &[&str], percentage: u32) -> String {
    json!({ "text": text, "tooltip": tooltip, "class": class, "percentage": percentage }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_quota_core::model::Usage;
    use serde_json::Value;

    fn shared(s: f64, w: f64, error: Option<&str>) -> Shared {
        Shared {
            fetched_at: 1000,
            next_at: 1180,
            usage: Some(Usage {
                session: Limit { percent: s, resets_at: Some(1000 + 3 * 3600 + 600) },
                weekly: Limit { percent: w, resets_at: Some(1000 + 3 * 86400) },
            }),
            error: error.map(str::to_owned),
        }
    }

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn plain_compact() {
        let v = parse(&render(Some(&shared(36.0, 23.0, None)), 1060, Style { compact: true, plain: true }));
        assert_eq!(v["text"], "5h 36% · 3h   7d 23% · 2d");
        assert_eq!(v["class"], json!(["ok"]));
        assert_eq!(v["percentage"], 36);
    }

    #[test]
    fn plain_bars() {
        let v = parse(&render(Some(&shared(36.0, 95.0, None)), 1000, Style { compact: false, plain: true }));
        assert_eq!(v["text"], "5h ■■■■□□□□□□ 36% · 3h   7d ■■■■■■■■■■ 95% · 3d");
        assert_eq!(v["class"], json!(["critical"]));
        assert_eq!(v["percentage"], 95);
    }

    #[test]
    fn coloured_and_stale() {
        let v = parse(&render(Some(&shared(75.0, 0.0, Some("offline <x>"))), 1000, Style::default()));
        let text = v["text"].as_str().unwrap();
        assert!(text.contains(&format!("<span color='{AMBER}'>■■■■■■■■</span>")));
        assert!(!text.contains(&format!("<span color='{ORANGE}'></span>")), "no empty spans");
        assert_eq!(v["class"], json!(["warn", "stale"]));
        assert!(v["tooltip"].as_str().unwrap().contains("offline &lt;x&gt;"));
        assert!(v["tooltip"].as_str().unwrap().contains("resets in 3h 10m"));
    }

    #[test]
    fn no_data() {
        assert_eq!(parse(&render(None, 0, Style::default()))["class"], json!(["loading"]));
        let e = Shared { fetched_at: 0, next_at: 60, usage: None, error: Some("token expired, run claude".into()) };
        let v = parse(&render(Some(&e), 0, Style::default()));
        assert_eq!(v["text"], "Claude: token expired, run claude");
        assert_eq!(v["class"], json!(["error"]));
    }
}
