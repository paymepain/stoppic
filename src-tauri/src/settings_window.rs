use crate::i18n::{self, t};
use crate::shortcuts::{self, HotkeyResult, Name};
use crate::{donate, files, record, settings, tray};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;

const LABEL: &str = "settings";

#[derive(Serialize)]
pub struct State {
    settings: settings::AppSettings,
    strings: HashMap<String, String>,
    platform: &'static str,
    version: String,
    defaults: settings::Hotkeys,
    /// Audio devices for the lists in the recording section
    mics: Vec<record::AudioDevice>,
    outputs: Vec<record::AudioDevice>,
}

fn state() -> State {
    let (mics, outputs) = record::audio_devices();
    State {
        settings: settings::get(),
        strings: i18n::strings(),
        // Platform names as in Node.js: the settings window expects them
        platform: if cfg!(windows) {
            "win32"
        } else if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "linux"
        },
        version: crate::app().package_info().version.to_string(),
        defaults: settings::default_hotkeys(),
        mics,
        outputs,
    }
}

const WIN_W: f64 = 480.0;
/// Initial height until the page reports its content height (see `settings_fit`)
const WIN_H: f64 = 860.0;
const MIN_H: f64 = 360.0;

/// Tallest the window may be on the current monitor: the work area minus the frame and title bar.
/// On small screens it must not go behind the taskbar.
fn max_height() -> f64 {
    let app = crate::app();
    let mon = app
        .cursor_position()
        .ok()
        .and_then(|c| app.monitor_from_point(c.x, c.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(mon) = mon else { return WIN_H };
    let scale = mon.scale_factor().max(0.5);
    let work = mon.work_area().size.height as f64 / scale;
    (work - 60.0).max(420.0)
}

fn fitted_size() -> LogicalSize<f64> {
    LogicalSize::new(WIN_W, WIN_H.min(max_height()))
}

/// The page reports its content height (CSS px) after every render. The window is never taller
/// than the content: that is both the size and the upper limit for manual resizing. A window the
/// user made shorter is left alone, so a toggle does not snap it back.
#[tauri::command]
pub async fn settings_fit(window: WebviewWindow, height: f64) {
    if !from_settings(&window) || !height.is_finite() {
        return;
    }
    let h = height.ceil().clamp(MIN_H, max_height());
    let _ = window.set_max_size(Some(LogicalSize::new(WIN_W, h)));
    let scale = window.scale_factor().unwrap_or(1.0);
    let current = window.inner_size().map(|s| s.height as f64 / scale).unwrap_or(WIN_H);
    let first = !window.is_visible().unwrap_or(true);
    if first || current > h + 0.5 {
        let _ = window.set_size(LogicalSize::new(WIN_W, h));
    }
    if first {
        let _ = window.center();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn from_settings(w: &WebviewWindow) -> bool {
    w.label() == LABEL
}

/// Sync autostart with the saved value
pub fn sync_autostart() {
    set_autostart(settings::get().auto_launch);
}

fn set_autostart(on: bool) {
    let al = crate::app().autolaunch();
    let res = if on { al.enable() } else { al.disable() };
    if let Err(e) = res {
        eprintln!("[settings] автозапуск: {e}");
    }
}

pub fn open() {
    let app = crate::app();
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let size = fitted_size();
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("settings.html".into()))
        .title(t("settingsTitle"))
        .inner_size(size.width, size.height)
        .min_inner_size(WIN_W, MIN_H)
        .max_inner_size(WIN_W, size.height)
        .resizable(true)
        .maximizable(false)
        .center()
        // Shown by `settings_fit` once the page knows its height, so it does not flash at the wrong size
        .visible(false)
        .disable_drag_drop_handler()
        .build();
    match built {
        Ok(w) => {
            w.on_window_event(|e| {
                // In case the window was closed while a combination was being recorded
                if let tauri::WindowEvent::Destroyed = e {
                    shortcuts::resume();
                }
            });
            // Safety net: if the page never reports its height, show the window anyway
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                if w.is_visible().is_ok_and(|v| !v) {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            });
        }
        Err(e) => eprintln!("[settings] окно: {e}"),
    }
}

#[tauri::command]
pub async fn settings_get(window: WebviewWindow) -> Option<State> {
    from_settings(&window).then(state)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    auto_save: Option<bool>,
    auto_launch: Option<bool>,
    language: Option<String>,
    rec_format: Option<String>,
    rec_fps: Option<u32>,
    rec_mic: Option<bool>,
    rec_system_audio: Option<bool>,
    rec_mic_device: Option<String>,
    rec_sys_device: Option<String>,
    rec_countdown: Option<bool>,
}

#[tauri::command]
pub async fn settings_set(window: WebviewWindow, patch: Patch) -> State {
    if !from_settings(&window) {
        return state();
    }
    settings::update(|s| {
        if let Some(v) = patch.auto_save {
            s.auto_save = v;
        }
        if let Some(v) = patch.auto_launch {
            s.auto_launch = v;
        }
        if let Some(v) = patch.rec_format.as_deref().filter(|v| matches!(*v, "mp4" | "gif")) {
            s.rec_format = v.to_string();
        }
        if let Some(v) = patch.rec_fps.filter(|v| matches!(v, 15 | 30 | 60)) {
            s.rec_fps = v;
        }
        if let Some(v) = patch.rec_mic {
            s.rec_mic = v;
        }
        if let Some(v) = patch.rec_system_audio {
            s.rec_system_audio = v;
        }
        if let Some(v) = patch.rec_mic_device.filter(|v| v.len() <= 512) {
            s.rec_mic_device = v;
        }
        if let Some(v) = patch.rec_sys_device.filter(|v| v.len() <= 512) {
            s.rec_sys_device = v;
        }
        if let Some(v) = patch.rec_countdown {
            s.rec_countdown = v;
        }
        if let Some(v) = patch.language.as_deref().filter(|v| matches!(*v, "ru" | "en")) {
            s.language = v.to_string();
        }
    });
    if let Some(v) = patch.auto_launch {
        set_autostart(v);
    }
    if patch.language.is_some() {
        i18n::set_language(&settings::get().language);
        tray::rebuild();
        let _ = window.set_title(&t("settingsTitle"));
    }
    state()
}

#[tauri::command]
pub async fn settings_set_hotkey(window: WebviewWindow, name: String, accel: String) -> HotkeyResult {
    let name = match Name::from_key(&name) {
        Some(n) if from_settings(&window) => n,
        _ => return shortcuts::set_hotkey(Name::Screenshot, ""), // deliberately "invalid"
    };
    let res = shortcuts::set_hotkey(name, &accel);
    if res.is_ok() {
        tray::rebuild();
    }
    res
}

#[tauri::command]
pub async fn settings_hotkeys_pause(window: WebviewWindow, paused: bool) {
    if !from_settings(&window) {
        return;
    }
    if paused {
        shortcuts::pause();
    } else {
        shortcuts::resume();
    }
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum DirResult {
    Ok { ok: bool, state: State },
    Err { ok: bool, #[serde(skip_serializing_if = "Option::is_none")] error: Option<&'static str> },
}

#[tauri::command]
pub async fn settings_choose_dir(window: WebviewWindow) -> DirResult {
    use tauri_plugin_dialog::DialogExt;
    let fail = |error| DirResult::Err { ok: false, error };
    if !from_settings(&window) {
        return fail(None);
    }
    let current = PathBuf::from(settings::get().save_dir);
    let w = window.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        crate::app().dialog().file().set_parent(&w).set_directory(current).blocking_pick_folder()
    })
    .await
    .ok()
    .flatten();
    let Some(dir) = picked.and_then(|p| p.into_path().ok()) else { return fail(None) };
    if !files::usable_dir(&dir) {
        return fail(Some("dir"));
    }
    settings::update(|s| s.save_dir = dir.to_string_lossy().into_owned());
    DirResult::Ok { ok: true, state: state() }
}

#[tauri::command]
pub async fn settings_open_dir(window: WebviewWindow) {
    if from_settings(&window) {
        files::open_dir(&PathBuf::from(settings::get().save_dir));
    }
}

#[tauri::command]
pub async fn settings_donate(window: WebviewWindow) {
    if from_settings(&window) {
        donate::open();
    }
}
