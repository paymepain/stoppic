use crate::i18n::{self, t};
use crate::shortcuts::{self, HotkeyResult, Name};
use crate::{capture, donate, files, settings, tray};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;

const LABEL: &str = "settings";

#[derive(Serialize)]
pub struct State {
    settings: settings::AppSettings,
    strings: HashMap<String, String>,
    platform: &'static str,
    version: String,
    defaults: settings::Hotkeys,
}

fn state() -> State {
    State {
        settings: settings::get(),
        strings: i18n::strings(),
        // Имена платформ как в Node.js: их ждёт окно настроек
        platform: if cfg!(windows) {
            "win32"
        } else if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "linux"
        },
        version: crate::app().package_info().version.to_string(),
        defaults: settings::default_hotkeys(),
    }
}

fn from_settings(w: &WebviewWindow) -> bool {
    w.label() == LABEL
}

/// Синхронизируем автозапуск с сохранённым значением
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
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("settings.html".into()))
        .title(t("settingsTitle"))
        .inner_size(480.0, 700.0)
        .resizable(false)
        .maximizable(false)
        .center()
        .visible(true)
        .disable_drag_drop_handler()
        .build();
    match built {
        Ok(w) => {
            w.on_window_event(|e| {
                // На случай, если окно закрыли во время записи сочетания
                if let tauri::WindowEvent::Destroyed = e {
                    shortcuts::resume();
                }
            });
            let _ = w.set_focus();
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
    fast_start: Option<bool>,
    language: Option<String>,
    rec_format: Option<String>,
    rec_fps: Option<u32>,
    rec_mic: Option<bool>,
    rec_system_audio: Option<bool>,
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
        if let Some(v) = patch.fast_start {
            s.fast_start = v;
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
        if let Some(v) = patch.rec_countdown {
            s.rec_countdown = v;
        }
        if let Some(v) = patch.language.as_deref().filter(|v| matches!(*v, "auto" | "ru" | "en")) {
            s.language = v.to_string();
        }
    });
    if let Some(v) = patch.auto_launch {
        set_autostart(v);
    }
    if patch.fast_start.is_some() {
        capture::rebuild_pool();
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
    let name = match name.as_str() {
        "screenshot" if from_settings(&window) => Name::Screenshot,
        "record" if from_settings(&window) => Name::Record,
        _ => return shortcuts::set_hotkey(Name::Screenshot, ""), // заведомо «invalid»
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
