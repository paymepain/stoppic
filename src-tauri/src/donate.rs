use tauri::Url;
use tauri_plugin_opener::OpenerExt;

const DONATE_URL: &str = "https://www.donationalerts.com/r/paymepain";
const ALLOWED_HOSTS: &[&str] = &["www.donationalerts.com"];

pub fn open() {
    let ok = Url::parse(DONATE_URL)
        .map(|u| u.scheme() == "https" && u.host_str().is_some_and(|h| ALLOWED_HOSTS.contains(&h)))
        .unwrap_or(false);
    if !ok {
        eprintln!("[donate] ссылка не прошла проверку: {DONATE_URL}");
        return;
    }
    let _ = crate::app().opener().open_url(DONATE_URL, None::<&str>);
}
