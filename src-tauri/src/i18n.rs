use crate::strings::{EN, RU};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

static RUSSIAN: AtomicBool = AtomicBool::new(false);

fn table() -> &'static [(&'static str, &'static str)] {
    if RUSSIAN.load(Ordering::Relaxed) {
        RU
    } else {
        EN
    }
}

/// "ru" or "en"; anything else is treated as English.
pub fn set_language(lang: &str) {
    RUSSIAN.store(lang == "ru", Ordering::Relaxed);
}

/// Windows display language is Russian. Used once, to migrate the old "auto" setting.
pub fn system_is_russian() -> bool {
    sys_locale::get_locale().is_some_and(|l| l.to_lowercase().starts_with("ru"))
}

pub fn t(key: &str) -> String {
    table().iter().find(|(k, _)| *k == key).map(|(_, v)| (*v).to_string()).unwrap_or_else(|| key.to_string())
}

pub fn t_with(key: &str, vars: &[(&str, &str)]) -> String {
    let mut s = t(key);
    for (k, v) in vars {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// All UI strings in the current language (for windows).
pub fn strings() -> HashMap<String, String> {
    table().iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
}
