//! Thin WinAPI helpers: strings, message boxes, clipboard, screen geometry and capture.

use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::thread::sleep;
use std::time::Duration;

use hover_copy::bgr::{BgrImage, Rect};
use windows_sys::Win32::Foundation::{GetLastError, GlobalFree, HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// NUL-terminated UTF-16 for WinAPI.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// `owner` may be null; an owned box stays above its window.
pub fn message_box(owner: HWND, text: &str, error: bool) {
    let style = if error { MB_OK | MB_ICONERROR } else { MB_OK | MB_ICONINFORMATION };
    unsafe { MessageBoxW(owner, wide(text).as_ptr(), wide("HoverCopy").as_ptr(), style) };
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

pub fn virtual_screen() -> Rect {
    unsafe {
        Rect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            w: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            h: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

pub struct Monitor {
    pub work: Rect,
    pub dpi: u32,
}

pub fn monitor_at(x: i32, y: i32) -> Monitor {
    unsafe {
        let hmon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        let work = if GetMonitorInfoW(hmon, &mut info) != 0 { rect(&info.rcWork) } else { virtual_screen() };
        let (mut dpi, mut dpi_y) = (96, 96);
        if GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y) != 0 {
            dpi = 96;
        }
        Monitor { work, dpi }
    }
}

pub fn create_font(dpi: u32) -> HFONT {
    let height = -(11 * dpi as i32 / 72); // 11 pt
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            FW_SEMIBOLD as _,
            0,
            0,
            0,
            DEFAULT_CHARSET as _,
            OUT_DEFAULT_PRECIS as _,
            CLIP_DEFAULT_PRECIS as _,
            CLEARTYPE_QUALITY as _,
            DEFAULT_PITCH as _,
            wide("Segoe UI").as_ptr(),
        )
    }
}

/// Screen pixels of `r` (virtual-screen coordinates, physical pixels).
pub fn capture(r: Rect) -> Result<BgrImage, String> {
    unsafe {
        let screen = GetDC(null_mut());
        if screen.is_null() {
            return Err("GetDC failed".into());
        }
        let mem = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, r.w, r.h);
        let old = SelectObject(mem, bitmap);
        let blitted = BitBlt(mem, 0, 0, r.w, r.h, screen, r.x, r.y, SRCCOPY) != 0;
        SelectObject(mem, old);

        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = r.w;
        info.bmiHeader.biHeight = -r.h; // top-down rows
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bgra = vec![0u8; r.w as usize * r.h as usize * 4];
        let rows = GetDIBits(mem, bitmap, 0, r.h as u32, bgra.as_mut_ptr() as *mut c_void, &mut info, DIB_RGB_COLORS);

        DeleteObject(bitmap);
        DeleteDC(mem);
        ReleaseDC(null_mut(), screen);
        if !blitted || rows != r.h {
            return Err(format!("BitBlt/GetDIBits failed (error {})", GetLastError()));
        }
        Ok(BgrImage::from_bgra(r.w as usize, r.h as usize, &bgra))
    }
}

pub fn set_clipboard(owner: HWND, text: &str) -> Result<(), String> {
    let data = wide(text);
    unsafe {
        open_clipboard(owner)?;
        let result = (|| {
            if EmptyClipboard() == 0 {
                return Err("EmptyClipboard");
            }
            let mem = GlobalAlloc(GMEM_MOVEABLE, data.len() * 2);
            if mem.is_null() {
                return Err("GlobalAlloc");
            }
            let ptr = GlobalLock(mem) as *mut u16;
            if ptr.is_null() {
                GlobalFree(mem);
                return Err("GlobalLock");
            }
            ptr.copy_from_nonoverlapping(data.as_ptr(), data.len());
            GlobalUnlock(mem);
            // On success the clipboard owns the memory.
            if SetClipboardData(CF_UNICODETEXT as u32, mem).is_null() {
                GlobalFree(mem);
                return Err("SetClipboardData");
            }
            Ok(())
        })();
        CloseClipboard();
        result.map_err(|step| format!("{step} failed"))
    }
}

pub fn get_clipboard(owner: HWND) -> Option<String> {
    unsafe {
        open_clipboard(owner).ok()?;
        let handle = GetClipboardData(CF_UNICODETEXT as u32);
        let ptr = if handle.is_null() { null() } else { GlobalLock(handle) as *const u16 };
        let text = (!ptr.is_null()).then(|| {
            let len = (0..).take_while(|&i| *ptr.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
        });
        if !ptr.is_null() {
            GlobalUnlock(handle);
        }
        CloseClipboard();
        text
    }
}

/// For `--selftest`: puts the text on the clipboard and reads it back.
pub fn clipboard_roundtrip(text: &str) -> String {
    unsafe {
        // SetClipboardData needs an owner window; a message-only one is enough.
        let owner = CreateWindowExW(0, wide("STATIC").as_ptr(), null(), 0, 0, 0, 0, 0, HWND_MESSAGE, null_mut(), null_mut(), null());
        let report = match set_clipboard(owner, text) {
            Err(e) => format!("set failed: {e}"),
            Ok(()) => match get_clipboard(owner) {
                Some(back) if back == text => "ok".to_owned(),
                other => format!("read back {other:?}"),
            },
        };
        DestroyWindow(owner);
        report
    }
}

/// Another program may hold the clipboard for a moment.
fn open_clipboard(owner: HWND) -> Result<(), String> {
    for _ in 0..10 {
        if unsafe { OpenClipboard(owner) } != 0 {
            return Ok(());
        }
        sleep(Duration::from_millis(20));
    }
    Err("буфер обмена занят другой программой".into())
}

fn rect(r: &RECT) -> Rect {
    Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
}
