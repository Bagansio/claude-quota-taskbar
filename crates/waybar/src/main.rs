//! Claude plan usage as a Waybar custom module (Hyprland, Sway, river, ...).
//!
//! ```text
//! claude-quota-waybar            # keep running, print a JSON line whenever the readout changes
//! claude-quota-waybar --once     # print one line and exit (for "interval"-style modules)
//! claude-quota-waybar --refresh  # ask the running instances to refetch now (on-click)
//! ```

mod cache;
mod http;
mod output;

use cache::Shared;
use claude_quota_core::fetch::{self, next_poll_secs};
use claude_quota_core::{now_secs, WakeDetector, MIN_MANUAL_REFRESH_SECS};
use output::Style;
use std::io::Write;
use std::time::Duration;

const USAGE: &str = "\
usage: claude-quota-waybar [--once | --refresh] [--compact] [--slim] [--plain]

  (default)   run continuously for a Waybar custom module (\"return-type\": \"json\")
  --once      print a single JSON line and exit
  --refresh   ask running instances to refetch now (use as on-click)
  --compact   percentages and reset times only, no segment bars
  --slim      narrow bars and a dimmed reset time, for crowded bars
  --plain     no Pango colours; style the module from CSS classes instead";

fn is_due(cur: Option<&Shared>, now: i64, woke_at: Option<i64>) -> bool {
    let Some(c) = cur else { return true };
    let since_last = now - c.fetched_at;
    now >= c.next_at
        || (cache::refresh_requested() && since_last >= MIN_MANUAL_REFRESH_SECS as i64)
        // Woke up and nobody has fetched since. Every instance notices the wake within ~1 s,
        // so a fetch from just before our own detection counts.
        || woke_at.is_some_and(|w| c.fetched_at < w - 2 && since_last >= MIN_MANUAL_REFRESH_SECS as i64)
}

/// Fetches if due (only one instance at a time does), then returns the shared state.
fn tick(now: i64, woke_at: Option<i64>) -> Option<Shared> {
    let cur = cache::load();
    if !is_due(cur.as_ref(), now, woke_at) {
        return cur;
    }
    let Some(_lock) = cache::Lock::acquire() else {
        return cur; // another instance is fetching; pick up its result next tick
    };
    let cur = cache::load(); // it may have just finished
    if !is_due(cur.as_ref(), now, woke_at) {
        return cur;
    }
    let r = fetch::fetch_usage(http::get);
    let next = Shared {
        fetched_at: now,
        next_at: now + next_poll_secs(&r, woke_at.map(|w| now - w)) as i64,
        usage: r.as_ref().ok().copied().or(cur.and_then(|c| c.usage)),
        error: r.err().map(|e| e.message()),
    };
    let _ = cache::save(&next);
    cache::clear_refresh();
    Some(next)
}

fn main() {
    let mut style = Style::default();
    let (mut once, mut refresh) = (false, false);
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--once" => once = true,
            "--refresh" => refresh = true,
            "--compact" => style.compact = true,
            "--plain" => style.plain = true,
            "--slim" => style.slim = true,
            "-V" | "--version" => return println!("claude-quota-waybar {}", env!("CARGO_PKG_VERSION")),
            "-h" | "--help" => return println!("{USAGE}"),
            _ => {
                eprintln!("{USAGE}");
                std::process::exit(2);
            }
        }
    }
    if refresh {
        if let Err(e) = cache::request_refresh() {
            eprintln!("claude-quota-waybar: {e}");
            std::process::exit(1);
        }
        return;
    }

    let mut out = std::io::stdout().lock();
    let mut last = String::new();
    let mut wake = WakeDetector::new();
    let mut woke_at = None;
    loop {
        let now = now_secs();
        if wake.check() {
            woke_at = Some(now);
        }
        let line = output::render(tick(now, woke_at).as_ref(), now, style);
        if line != last {
            // Waybar closed the pipe (reload/quit): exit instead of spinning.
            if writeln!(out, "{line}").and_then(|_| out.flush()).is_err() {
                return;
            }
            last = line;
        }
        if once {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
