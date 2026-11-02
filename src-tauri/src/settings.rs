// Settings in a JSON file. The format matches electron-store, so on first launch
// the settings of the Electron version are picked up.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotkeys {
    /// Region screenshot (with overlay)
    pub screenshot: String,
    /// Full-screen screenshot straight to the clipboard, without overlay
    pub screenshot_full: String,
    /// Active window screenshot straight to the clipboard
    pub screenshot_window: String,
    /// Region recording (with overlay)
    pub record: String,
    /// Full-screen recording without region selection
    pub record_full: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        default_hotkeys()
    }
}

// Ctrl+Shift+S ("Save as") and Ctrl+Shift+R (hard refresh) must not be intercepted globally
const OLD_DEFAULTS: (&str, &str) = ("CommandOrControl+Shift+S", "CommandOrControl+Shift+R");

/// Shift in the combination means "whole screen": easier to remember
pub fn default_hotkeys() -> Hotkeys {
    Hotkeys {
        screenshot: "CommandOrControl+Alt+A".into(),
        screenshot_full: "CommandOrControl+Alt+Shift+A".into(),
        screenshot_window: "CommandOrControl+Alt+W".into(),
        record: "CommandOrControl+Alt+R".into(),
        record_full: "CommandOrControl+Alt+Shift+R".into(),
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// "ru" or "en"
    pub language: String,
    /// Language last applied from the installer, so that a reinstall does not override
    /// a choice made later in the settings window
    #[serde(skip_serializing_if = "String::is_empty")]
    pub installer_language: String,
    pub hotkeys: Hotkeys,
    pub save_dir: String,
    /// Copy the screenshot to the clipboard and at the same time save a file to saveDir
    pub auto_save: bool,
    pub auto_launch: bool,
    /// The welcome notification has already been shown
    pub welcomed: bool,
    pub rec_format: String,
    pub rec_fps: u32,
    pub rec_mic: bool,
    pub rec_system_audio: bool,
    /// WASAPI device identifiers. Empty string: the Windows default device
    pub rec_mic_device: String,
    pub rec_sys_device: String,
    pub rec_countdown: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_save_off_migrated: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: "en".into(),
            installer_language: String::new(),
            hotkeys: default_hotkeys(),
            save_dir: String::new(),
            auto_save: false,
            auto_launch: false,
            welcomed: false,
            rec_format: "mp4".into(),
            rec_fps: 30,
            rec_mic: false,
            rec_system_audio: true,
            rec_mic_device: String::new(),
            rec_sys_device: String::new(),
            rec_countdown: true,
            auto_save_off_migrated: false,
        }
    }
}

struct Store {
    data: AppSettings,
    path: PathBuf,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn read(path: &PathBuf) -> Option<AppSettings> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Call once at startup.
pub fn load(app: &AppHandle) {
    let dir = app.path().app_config_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = dir.join("settings.json");
    let mut data = read(&path)
        .or_else(|| {
            // Pick up settings from previous versions: Tauri builds with the old identifier,
            // Tauri versions of Screenshotter, then Electron versions (new and old)
            let config = app.path().config_dir().ok()?;
            ["com.example.stoppic", "com.example.screenshotter", "stoppic", "screenshotter"]
                .iter()
                .find_map(|name| read(&config.join(name).join("settings.json")))
        })
        .unwrap_or_default();

    if data.save_dir.is_empty() {
        let pictures = app.path().picture_dir().or_else(|_| app.path().home_dir()).unwrap_or_default();
        data.save_dir = pictures.join("Screenshots").to_string_lossy().into_owned();
    }
    // Create the folder right away: the user sees it in settings and expects it to exist
    if let Err(e) = fs::create_dir_all(&data.save_dir) {
        eprintln!("[settings] папка снимков: {e}");
    }
    // Migration: "auto" (follow Windows) is gone; keep what the user actually saw
    if !matches!(data.language.as_str(), "ru" | "en") {
        data.language = if crate::i18n::system_is_russian() { "ru" } else { "en" }.into();
    }
    // The installer writes the language chosen in its dialog to a marker file (see installer-hooks.nsh).
    // Apply it once; a later change in the settings window survives an upgrade
    let marker = dir.join("install-language");
    if let Ok(text) = fs::read_to_string(&marker) {
        let lang = text.trim();
        if matches!(lang, "ru" | "en") && lang != data.installer_language {
            data.language = lang.to_string();
            data.installer_language = lang.to_string();
        }
        let _ = fs::remove_file(&marker);
    }
    // Migration: replace the old defaults with safe ones
    if data.hotkeys.screenshot == OLD_DEFAULTS.0 && data.hotkeys.record == OLD_DEFAULTS.1 {
        data.hotkeys = default_hotkeys();
    }
    for id in [&mut data.rec_mic_device, &mut data.rec_sys_device] {
        if id.len() > 512 {
            id.clear();
        }
    }
    // Migration: copying used to also save a file by default. Reset once
    if !data.auto_save_off_migrated {
        data.auto_save = false;
        data.auto_save_off_migrated = true;
    }
    if ![15, 30, 60].contains(&data.rec_fps) {
        data.rec_fps = 30;
    }
    if data.rec_format != "mp4" && data.rec_format != "gif" {
        data.rec_format = "mp4".into();
    }

    let store = Store { data, path };
    save(&store);
    *STORE.lock().unwrap() = Some(store);
}

fn save(store: &Store) {
    let result = (|| -> std::io::Result<()> {
        if let Some(dir) = store.path.parent() {
            fs::create_dir_all(dir)?;
        }
        // Via a temporary file: if we crash mid-write, the old settings are not lost
        let tmp = store.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&store.data)?)?;
        fs::rename(&tmp, &store.path)
    })();
    if let Err(e) = result {
        eprintln!("[settings] не удалось сохранить: {e}");
    }
}

pub fn get() -> AppSettings {
    STORE.lock().unwrap().as_ref().map(|s| s.data.clone()).unwrap_or_default()
}

pub fn update(f: impl FnOnce(&mut AppSettings)) {
    let mut guard = STORE.lock().unwrap();
    if let Some(store) = guard.as_mut() {
        f(&mut store.data);
        save(store);
    }
}
