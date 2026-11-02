// Small Windows bits missing from the Tauri API.

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DwmSetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS, DWMWA_TRANSITIONS_FORCEDISABLED};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsIconic, SetLayeredWindowAttributes, SetWindowDisplayAffinity,
    SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA, WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED,
};

fn hwnd(w: &tauri::WebviewWindow) -> Option<HWND> {
    w.hwnd().ok().map(|h| HWND(h.0 as _))
}

/// No show animation: the overlay appears instantly.
pub fn disable_transitions(w: &tauri::WebviewWindow) {
    if let Some(h) = hwnd(w) {
        let on: i32 = 1;
        unsafe {
            let _ = DwmSetWindowAttribute(h, DWMWA_TRANSITIONS_FORCEDISABLED, &on as *const i32 as _, 4);
        }
    }
}

/// Opacity of the whole window. None restores a normal window.
/// WebView2 doesn't render a hidden window, so the overlay is shown transparent
/// and revealed once the frame is drawn: otherwise a black backdrop would flash.
pub fn set_alpha(w: &tauri::WebviewWindow, alpha: Option<u8>) {
    let Some(h) = hwnd(w) else { return };
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        match alpha {
            Some(a) => {
                SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as isize);
                let _ = SetLayeredWindowAttributes(h, Default::default(), a, LWA_ALPHA);
            }
            None => {
                if ex & WS_EX_LAYERED.0 as isize != 0 {
                    SetWindowLongPtrW(h, GWL_EXSTYLE, ex & !(WS_EX_LAYERED.0 as isize));
                }
            }
        }
    }
}

/// The recording panel is excluded from recordings and screenshots.
pub fn exclude_from_capture(w: &tauri::WebviewWindow) {
    if let Some(h) = hwnd(w) {
        unsafe {
            let _ = SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE);
        }
    }
}

/// HMONITOR of the monitor containing the point (physical pixels).
pub fn monitor_at(x: i32, y: i32) -> *mut std::ffi::c_void {
    unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST).0 }
}

/// Bounds of the active window in physical pixels: (left, top, right, bottom).
/// DWM returns the visible frame without the invisible margins Windows adds around windows.
pub fn foreground_window_rect() -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let h = GetForegroundWindow();
        if h.0.is_null() || IsIconic(h).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        let ok = DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as _, std::mem::size_of::<RECT>() as u32).is_ok()
            || GetWindowRect(h, &mut r).is_ok();
        (ok && r.right > r.left && r.bottom > r.top).then_some((r.left, r.top, r.right, r.bottom))
    }
}
