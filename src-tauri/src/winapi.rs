// Мелочи Windows, которых нет в API Tauri.

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA,
    WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED,
};

fn hwnd(w: &tauri::WebviewWindow) -> Option<HWND> {
    w.hwnd().ok().map(|h| HWND(h.0 as _))
}

/// Без анимации появления: оверлей возникает мгновенно.
pub fn disable_transitions(w: &tauri::WebviewWindow) {
    if let Some(h) = hwnd(w) {
        let on: i32 = 1;
        unsafe {
            let _ = DwmSetWindowAttribute(h, DWMWA_TRANSITIONS_FORCEDISABLED, &on as *const i32 as _, 4);
        }
    }
}

/// Прозрачность окна целиком. None возвращает обычное окно.
/// Скрытое окно WebView2 не отрисовывает, поэтому оверлей показываем прозрачным
/// и проявляем, когда кадр уже нарисован: иначе мелькнёт чёрная подложка.
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

/// Панель записи не попадает в запись и скриншоты.
pub fn exclude_from_capture(w: &tauri::WebviewWindow) {
    if let Some(h) = hwnd(w) {
        unsafe {
            let _ = SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE);
        }
    }
}

/// HMONITOR монитора, в котором лежит точка (физические пиксели).
pub fn monitor_at(x: i32, y: i32) -> *mut std::ffi::c_void {
    unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST).0 }
}
