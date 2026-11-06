use crate::i18n::t;
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::sync::OnceLock;
#[cfg(windows)]
use tauri::Manager;

pub type OnClick = Box<dyn Fn() + Send + Sync + 'static>;

/// System notification. On Windows clicking it runs onClick (e.g. opens the folder with the file).
#[cfg(windows)]
pub fn notify(body: &str, on_click: Option<OnClick>) {
    use tauri_winrt_notification::Toast;

    // Without the installer's shortcut (running from target/) Windows doesn't know our AppUserModelID,
    // so during development notifications come from PowerShell, as in the Tauri plugin
    let installed = std::env::current_exe()
        .map(|p| !p.components().any(|c| c.as_os_str().eq_ignore_ascii_case("target")))
        .unwrap_or(false);
    let app_id = if installed { crate::app().config().identifier.clone() } else { Toast::POWERSHELL_APP_ID.to_string() };

    let mut toast = Toast::new(&app_id).title(&t("tooltip")).text1(body).sound(None);
    // Large icon to the left of the text: without it Windows shows only a small icon in the header
    if let Some(icon) = toast_icon() {
        toast = toast.icon(icon, tauri_winrt_notification::IconCrop::Square, "StopPic");
    }
    if let Some(cb) = on_click {
        toast = toast.on_activated(move |_| {
            cb();
            Ok(())
        });
    }
    if let Err(e) = toast.show() {
        eprintln!("[notify] {e}");
    }
}

#[cfg(not(windows))]
pub fn notify(body: &str, _on_click: Option<OnClick>) {
    use tauri_plugin_notification::NotificationExt;
    let _ = crate::app().notification().builder().title(t("tooltip")).body(body).show();
}

/// The notification needs a file on disk: put the icon next to the settings once.
#[cfg(windows)]
fn toast_icon() -> Option<&'static PathBuf> {
    static ICON: OnceLock<Option<PathBuf>> = OnceLock::new();
    ICON.get_or_init(|| {
        const PNG: &[u8] = include_bytes!("../icons/128x128.png");
        let dir = crate::app().path().app_config_dir().ok()?;
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("toast-icon.png");
        let fresh = std::fs::metadata(&path).is_ok_and(|m| m.len() == PNG.len() as u64);
        if !fresh {
            std::fs::write(&path, PNG).map_err(|e| eprintln!("[notify] иконка: {e}")).ok()?;
        }
        Some(path)
    })
    .as_ref()
}
