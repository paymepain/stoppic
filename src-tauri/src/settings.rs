// Настройки в JSON-файле. Формат совпадает с electron-store, поэтому при первом запуске
// подхватываются настройки Electron-версии.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Hotkeys {
    pub screenshot: String,
    pub record: String,
}

// Ctrl+Shift+S («Сохранить как») и Ctrl+Shift+R (жёсткое обновление) глобально перехватывать нельзя
const OLD_DEFAULTS: (&str, &str) = ("CommandOrControl+Shift+S", "CommandOrControl+Shift+R");

pub fn default_hotkeys() -> Hotkeys {
    Hotkeys { screenshot: "CommandOrControl+Alt+A".into(), record: "CommandOrControl+Alt+R".into() }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub language: String,
    pub hotkeys: Hotkeys,
    pub save_dir: String,
    /// Копировать снимок в буфер и одновременно сохранять файл в saveDir
    pub auto_save: bool,
    pub auto_launch: bool,
    /// Держать окно оверлея заранее загруженным: снимок появляется мгновенно, но это занимает память
    pub fast_start: bool,
    /// Приветственное уведомление уже показывалось
    pub welcomed: bool,
    pub rec_format: String,
    pub rec_fps: u32,
    pub rec_mic: bool,
    pub rec_system_audio: bool,
    pub rec_countdown: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_save_off_migrated: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: "auto".into(),
            hotkeys: default_hotkeys(),
            save_dir: String::new(),
            auto_save: false,
            auto_launch: false,
            fast_start: true,
            welcomed: false,
            rec_format: "mp4".into(),
            rec_fps: 30,
            rec_mic: false,
            rec_system_audio: true,
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

/// Вызывать один раз при запуске.
pub fn load(app: &AppHandle) {
    let dir = app.path().app_config_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = dir.join("settings.json");
    let mut data = read(&path)
        .or_else(|| {
            // Подхватываем настройки прежних версий: Tauri-сборки со старым identifier,
            // Tauri-версии Screenshotter, затем Electron-версии (новой и старой)
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
    // Миграция: старые умолчания заменяем на безопасные
    if data.hotkeys.screenshot == OLD_DEFAULTS.0 && data.hotkeys.record == OLD_DEFAULTS.1 {
        data.hotkeys = default_hotkeys();
    }
    // Миграция: раньше копирование по умолчанию ещё и сохраняло файл. Сбрасываем один раз
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
        // Через временный файл: при сбое посреди записи старые настройки не пропадут
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
