use crate::i18n::t;

pub type OnClick = Box<dyn Fn() + Send + Sync + 'static>;

/// Системное уведомление. На Windows клик по нему выполняет onClick (например, открывает папку с файлом).
#[cfg(windows)]
pub fn notify(body: &str, on_click: Option<OnClick>) {
    use tauri_winrt_notification::Toast;

    // Без ярлыка установщика (запуск из target/) Windows не знает наш AppUserModelID,
    // поэтому при разработке уведомления идут от имени PowerShell, как в плагине Tauri
    let installed = std::env::current_exe()
        .map(|p| !p.components().any(|c| c.as_os_str().eq_ignore_ascii_case("target")))
        .unwrap_or(false);
    let app_id = if installed { crate::app().config().identifier.clone() } else { Toast::POWERSHELL_APP_ID.to_string() };

    let mut toast = Toast::new(&app_id).title(&t("tooltip")).text1(body).sound(None);
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
