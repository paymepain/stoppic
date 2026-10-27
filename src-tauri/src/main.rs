// В релизе без консольного окна
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Отладочный лог: только в dev-сборке
macro_rules! dlog {
    ($($t:tt)*) => {
        if cfg!(debug_assertions) {
            eprintln!("[dev] {}", format!($($t)*));
        }
    };
}

mod capture;
mod donate;
mod files;
mod i18n;
mod notify;
mod pin;
mod record;
mod settings;
mod settings_window;
mod shortcuts;
mod strings;
mod tray;
#[cfg(windows)]
mod winapi;

use std::sync::OnceLock;
use tauri::{AppHandle, RunEvent};

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Глобальный доступ к приложению: им пользуются хоткеи, таймеры и потоки записи.
pub fn app() -> &'static AppHandle {
    APP.get().expect("приложение ещё не запущено")
}

fn main() {
    let app = tauri::Builder::default()
        // Только один экземпляр. Повторный запуск открывает настройки: так понятно, что программа уже работает
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {
            notify::notify(&i18n::t("alreadyRunning"), None);
            settings_window::open();
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(shortcuts::on_shortcut).build())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            capture::overlay_ready,
            capture::overlay_frame,
            capture::overlay_shown,
            capture::overlay_revealed,
            capture::overlay_cancel,
            capture::overlay_action,
            capture::overlay_get_crop,
            capture::overlay_submit,
            capture::overlay_escape,
            capture::overlay_clipboard_image,
            capture::overlay_pick_image,
            capture::overlay_record,
            capture::dev_log,
            pin::pin_get_init,
            pin::pin_close,
            record::rec_get_config,
            record::rec_start,
            record::rec_command,
            settings_window::settings_get,
            settings_window::settings_set,
            settings_window::settings_set_hotkey,
            settings_window::settings_hotkeys_pause,
            settings_window::settings_choose_dir,
            settings_window::settings_open_dir,
            settings_window::settings_donate,
        ])
        .setup(|app| {
            let _ = APP.set(app.handle().clone());
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory); // без иконки в Dock

            #[cfg(not(windows))]
            app.handle().plugin(tauri_plugin_notification::init())?;

            settings::load(app.handle());
            i18n::set_language(&settings::get().language);
            tray::create(app.handle())?;
            shortcuts::register_at_start();
            capture::init();
            settings_window::sync_autostart();

            // Первый запуск: у программы нет окна, поэтому говорим, где её искать
            if !settings::get().welcomed {
                settings::update(|s| s.welcomed = true);
                let key = shortcuts::pretty(&settings::get().hotkeys.screenshot);
                notify::notify(&i18n::t_with("welcome", &[("key", &key)]), Some(Box::new(settings_window::open)));
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("не удалось запустить приложение");

    app.run(|_app, event| match event {
        // Программа живёт в трее: закрытие всех окон её не завершает
        RunEvent::ExitRequested { api, code: None, .. } => api.prevent_exit(),
        RunEvent::Exit => {
            capture::shutdown();
            record::shutdown();
        }
        _ => {}
    });
}
