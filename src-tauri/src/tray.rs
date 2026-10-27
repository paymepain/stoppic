use crate::i18n::t;
use crate::{capture, files, record, settings, settings_window, shortcuts};
use std::path::PathBuf;
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Wry};

const TRAY_ID: &str = "main";

fn icon(recording: bool) -> Image<'static> {
    let bytes: &[u8] = if recording {
        include_bytes!("../icons/trayRec.png")
    } else if cfg!(target_os = "macos") {
        include_bytes!("../icons/trayTemplate.png")
    } else {
        include_bytes!("../icons/tray.png")
    };
    Image::from_bytes(bytes).expect("иконка трея")
}

fn item(app: &AppHandle, id: &str, text: &str, accel: Option<String>) -> tauri::Result<MenuItem<Wry>> {
    MenuItem::with_id(app, id, text, true, accel.as_deref()).or_else(|_| MenuItem::with_id(app, id, text, true, None::<&str>))
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let hk = settings::get().hotkeys;
    let rec_label = if record::is_recording() { t("stopRecord") } else { t("record") };
    Menu::with_items(
        app,
        &[
            &item(app, "screenshot", &t("screenshot"), shortcuts::menu_accel(&hk.screenshot))?,
            &item(app, "record", &rec_label, shortcuts::menu_accel(&hk.record))?,
            &PredefinedMenuItem::separator(app)?,
            &item(app, "folder", &t("openFolder"), None)?,
            &item(app, "settings", &t("settings"), None)?,
            &PredefinedMenuItem::separator(app)?,
            &item(app, "quit", &t("quit"), None)?,
        ],
    )
}

fn on_menu(app: &AppHandle, e: MenuEvent) {
    match e.id().as_ref() {
        "screenshot" => {
            // Небольшая пауза, чтобы закрывающееся меню трея не попало в кадр
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(250));
                capture::start_capture(capture::Mode::Screenshot);
            });
        }
        "record" => record::toggle(), // меню обновится по событию смены состояния записи
        "folder" => files::open_dir(&PathBuf::from(settings::get().save_dir)),
        "settings" => settings_window::open(),
        "quit" => app.exit(0),
        _ => {}
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(false))
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip(t("tooltip"))
        .menu(&build_menu(app)?)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu)
        .build(app)?;
    Ok(())
}

pub fn rebuild() {
    let app = crate::app();
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), build_menu(app)) {
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_tooltip(Some(t("tooltip")));
    }
}

/// Красная иконка в трее, пока идёт запись.
pub fn set_recording(on: bool) {
    if let Some(tray) = crate::app().tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(icon(on)));
        let _ = tray.set_icon_as_template(!on && cfg!(target_os = "macos"));
    }
}
