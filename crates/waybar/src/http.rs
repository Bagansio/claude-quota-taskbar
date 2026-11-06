//! HTTP GET through the system `curl`: system TLS, proxy env vars, no HTTP/TLS crates.
//!
//! The bearer token is written to curl's stdin (`--header @-`), never passed as an argument,
//! so it does not show up in `ps` or `/proc/<pid>/cmdline`.

use claude_quota_core::fetch::{BETA_HEADER, HOST, PATH};
use std::io::Write;
use std::process::{Command, Stdio};

const USER_AGENT: &str = concat!("claude-quota-waybar/", env!("CARGO_PKG_VERSION"));

pub fn get(token: &str) -> Result<(u32, String), String> {
    let url = format!("https://{HOST}{PATH}");
    let mut child = Command::new("curl")
        .args(["--silent", "--proto", "=https", "--connect-timeout", "10", "--max-time", "15"])
        .args(["--user-agent", USER_AGENT, "--header", "@-", "--write-out", "\n%{http_code}", &url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "curl not found".to_string())?;
    {
        let mut stdin = child.stdin.take().ok_or("curl stdin")?;
        let headers = format!("Authorization: Bearer {token}\n{}: {}\n", BETA_HEADER.0, BETA_HEADER.1);
        stdin.write_all(headers.as_bytes()).map_err(|_| "curl failed".to_string())?;
    } // dropping stdin closes it so curl starts the request
    let out = child.wait_with_output().map_err(|_| "curl failed".to_string())?;
    if !out.status.success() {
        return Err("offline".into());
    }
    split_status(&String::from_utf8_lossy(&out.stdout)).ok_or_else(|| "bad curl output".into())
}

/// Splits `body\nSTATUS` (from `--write-out "\n%{http_code}"`).
fn split_status(out: &str) -> Option<(u32, String)> {
    let (body, code) = out.rsplit_once('\n')?;
    Some((code.trim().parse().ok()?, body.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split() {
        assert_eq!(split_status("{\"a\":1}\n200"), Some((200, "{\"a\":1}".into())));
        assert_eq!(split_status("line1\nline2\n429"), Some((429, "line1\nline2".into())));
        assert_eq!(split_status("\n000"), Some((0, String::new())));
        assert_eq!(split_status("nonsense"), None);
    }
}
