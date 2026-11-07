//! Fetching usage over WinHTTP (system TLS + proxy settings, no extra crates).

use claude_quota_core::fetch::{self, FetchError};
use claude_quota_core::model::Usage;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Networking::WinHttp::*;

struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Credentials, expiry and status handling are shared with the other frontends.
pub fn fetch_usage() -> Result<Usage, FetchError> {
    fetch::fetch_usage(http_get)
}

fn http_get(token: &str) -> Result<(u32, String), String> {
    unsafe {
        let agent = wide(concat!("claude-quota-taskbar/", env!("CARGO_PKG_VERSION")));
        let session = Handle(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0));
        if session.0.is_null() {
            return Err("WinHttpOpen failed".into());
        }
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 15_000);
        let host = wide(fetch::HOST);
        let conn = Handle(WinHttpConnect(session.0, host.as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0));
        if conn.0.is_null() {
            return Err("offline".into());
        }
        let (verb, path) = (wide("GET"), wide(fetch::PATH));
        let req = Handle(WinHttpOpenRequest(conn.0, verb.as_ptr(), path.as_ptr(), null(), null(), null(), WINHTTP_FLAG_SECURE));
        if req.0.is_null() {
            return Err("request failed".into());
        }
        let headers = wide(&format!("Authorization: Bearer {token}\r\nanthropic-beta: oauth-2025-04-20\r\n"));
        if WinHttpSendRequest(req.0, headers.as_ptr(), u32::MAX, null(), 0, 0, 0) == 0
            || WinHttpReceiveResponse(req.0, null_mut()) == 0
        {
            return Err("offline".into());
        }
        let mut status: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            &mut status as *mut u32 as *mut c_void,
            &mut size,
            null_mut(),
        );
        let mut body = Vec::new();
        loop {
            let mut avail = 0u32;
            if WinHttpQueryDataAvailable(req.0, &mut avail) == 0 || avail == 0 {
                break;
            }
            let mut buf = vec![0u8; avail as usize];
            let mut read = 0u32;
            if WinHttpReadData(req.0, buf.as_mut_ptr() as *mut c_void, avail, &mut read) == 0 || read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read as usize]);
        }
        Ok((status, String::from_utf8_lossy(&body).into_owned()))
    }
}
