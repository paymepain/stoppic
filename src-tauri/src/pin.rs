// «Закрепить»: снимок в маленьком окне поверх всех на том же месте экрана.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::ipc::Response;
use tauri::{PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

static PINS: LazyLock<Mutex<HashMap<String, Vec<u8>>>> = LazyLock::new(Default::default);
static SEQ: AtomicU64 = AtomicU64::new(0);

/// place: экранные координаты и размер в физических пикселях.
pub fn create(png: Vec<u8>, place: (i32, i32, u32, u32)) {
    let label = format!("pin-{}", SEQ.fetch_add(1, Ordering::Relaxed) + 1);
    PINS.lock().unwrap().insert(label.clone(), png);
    let built = WebviewWindowBuilder::new(crate::app(), &label, WebviewUrl::App("pin.html".into()))
        .title("StopPic")
        .decorations(false)
        .resizable(true)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .shadow(true)
        .visible(false)
        .focused(false)
        .min_inner_size(60.0, 40.0)
        .background_color(tauri::window::Color(17, 17, 17, 255))
        .disable_drag_drop_handler()
        .build();
    let win = match built {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[pin] окно: {e}");
            PINS.lock().unwrap().remove(&label);
            return;
        }
    };
    let (x, y, w, h) = place;
    let scale = win.scale_factor().unwrap_or(1.0);
    let w = w.max((60.0 * scale) as u32);
    let h = h.max((40.0 * scale) as u32);
    for _ in 0..2 {
        let _ = win.set_position(PhysicalPosition::new(x, y));
        let _ = win.set_size(PhysicalSize::new(w, h));
    }
    let lbl = label.clone();
    win.on_window_event(move |e| {
        if let tauri::WindowEvent::Destroyed = e {
            PINS.lock().unwrap().remove(&lbl);
        }
    });
    let _ = win.show();
}

#[tauri::command]
pub async fn pin_get_init(window: WebviewWindow) -> Response {
    Response::new(PINS.lock().unwrap().get(window.label()).cloned().unwrap_or_default())
}

#[tauri::command]
pub async fn pin_close(window: WebviewWindow) {
    if PINS.lock().unwrap().contains_key(window.label()) {
        let _ = window.destroy();
    }
}
