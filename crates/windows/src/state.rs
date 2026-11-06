//! State shared between the fetch thread and the UI thread.

use claude_quota_core::model::Usage;
use std::sync::Mutex;

#[derive(Clone, Default)]
pub struct Snapshot {
    /// Last good reading (kept while later fetches fail).
    pub usage: Option<Usage>,
    /// Error from the most recent fetch, if it failed.
    pub error: Option<String>,
}

pub static STATE: Mutex<Snapshot> = Mutex::new(Snapshot { usage: None, error: None });

pub fn snapshot() -> Snapshot {
    STATE.lock().map(|s| s.clone()).unwrap_or_default()
}
