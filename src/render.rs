//! GDI drawing. Layout mirrors a status-bar readout:
//!
//! ```text
//! |  5h  ▮▮▮▯▯▯▯▯▯▯  36% · 3h
//! |  7d  ▮▮▯▯▯▯▯▯▯▯  23% · 3d
//! ```
//! The background is the colour key of the layered window, so only the glyphs are visible.

use crate::api::wide;
use crate::model::{self, Level, Limit, SEGMENTS};
use crate::state::Snapshot;
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::*;

pub const KEY: u32 = rgb(1, 1, 1);
const ORANGE: u32 = rgb(232, 132, 92);
const AMBER: u32 = rgb(224, 160, 48);
const RED: u32 = rgb(217, 71, 63);
const EMPTY: u32 = rgb(58, 58, 58);
const LABEL: u32 = rgb(150, 150, 150);
const VALUE: u32 = rgb(185, 185, 185);
const BAR: u32 = rgb(110, 110, 110);

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

pub fn width(scale: f32) -> i32 {
    (300.0 * scale).round() as i32
}

struct Ctx {
    hdc: HDC,
    scale: f32,
    font: HFONT,
}

impl Ctx {
    fn s(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    unsafe fn rect(&self, x: i32, y: i32, w: i32, h: i32, color: u32, radius: i32) {
        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(self.hdc, brush);
        let old_pen = SelectObject(self.hdc, GetStockObject(NULL_PEN));
        RoundRect(self.hdc, x, y, x + w + 1, y + h + 1, radius * 2, radius * 2);
        SelectObject(self.hdc, old_pen);
        SelectObject(self.hdc, old_brush);
        DeleteObject(brush);
    }

    unsafe fn text(&self, s: &str, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let w16 = wide(s);
        let mut r = RECT { left: x, top: y, right: x + w, bottom: y + h };
        SetTextColor(self.hdc, color);
        DrawTextW(self.hdc, w16.as_ptr(), (w16.len() - 1) as i32, &mut r, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
    }

    unsafe fn row(&self, label: &str, l: &Limit, now: i64, y: i32, row_h: i32) {
        let fill = match model::level(l.percent) {
            Level::Normal => ORANGE,
            Level::Warn => AMBER,
            Level::Critical => RED,
        };
        self.text(label, self.s(14.0), y, self.s(30.0), row_h, LABEL);
        let (x0, seg_w, gap) = (self.s(46.0), self.s(12.0), self.s(3.0));
        let seg_h = self.s(14.0).min(row_h - 2);
        let sy = y + (row_h - seg_h) / 2;
        let r = self.s(3.0);
        for (i, f) in model::segment_fills(l.percent).iter().enumerate() {
            let x = x0 + i as i32 * (seg_w + gap);
            self.rect(x, sy, seg_w, seg_h, EMPTY, r);
            if *f > 0.0 {
                let fw = ((seg_w as f64 * f).round() as i32).max(2);
                self.rect(x, sy, fw, seg_h, fill, if fw < seg_w { 1 } else { r });
            }
        }
        let tx = x0 + SEGMENTS as i32 * (seg_w + gap) + self.s(6.0);
        self.text(&model::format_limit(l, now), tx, y, self.s(110.0), row_h, VALUE);
    }
}

pub unsafe fn paint(target: HDC, w: i32, h: i32, scale: f32, snap: &Snapshot, now: i64) {
    // Double buffer to avoid flicker.
    let hdc = CreateCompatibleDC(target);
    let bmp = CreateCompatibleBitmap(target, w, h);
    let old_bmp = SelectObject(hdc, bmp);

    let bg = CreateSolidBrush(KEY);
    FillRect(hdc, &RECT { left: 0, top: 0, right: w, bottom: h }, bg);
    DeleteObject(bg);

    let face = wide("Segoe UI");
    let font = CreateFontW(-((13.0 * scale).round() as i32), 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, ANTIALIASED_QUALITY as u32, 0, face.as_ptr());
    let old_font = SelectObject(hdc, font);
    SetBkMode(hdc, 1);
    let c = Ctx { hdc, scale, font };

    // Left rail turns amber while the latest fetch is failing.
    let rail = if snap.error.is_some() && snap.usage.is_some() { AMBER } else { BAR };
    c.rect(c.s(2.0), h * 15 / 100, c.s(2.0), h * 70 / 100, rail, 0);

    let row_h = h / 2;
    match snap.usage {
        Some(u) => {
            c.row("5h", &u.session, now, 0, row_h);
            c.row("7d", &u.weekly, now, row_h, row_h);
        }
        None => {
            let msg = snap.error.as_deref().unwrap_or("loading…");
            c.text("Claude usage", c.s(14.0), 0, w, row_h, LABEL);
            c.text(msg, c.s(14.0), row_h, w - c.s(14.0), row_h, VALUE);
        }
    }

    BitBlt(target, 0, 0, w, h, hdc, 0, 0, SRCCOPY);
    SelectObject(hdc, old_font);
    DeleteObject(c.font);
    SelectObject(hdc, old_bmp);
    DeleteObject(bmp);
    DeleteDC(hdc);
}
