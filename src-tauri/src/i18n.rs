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

pub fn set_language(lang: &str) {
    let ru = match lang {
        "ru" => true,
        "en" => false,
        _ => sys_locale::get_locale().is_some_and(|l| l.to_lowercase().starts_with("ru")),
    };
    RUSSIAN.store(ru, Ordering::Relaxed);
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

/// Все строки интерфейса на текущем языке (для окон).
pub fn strings() -> HashMap<String, String> {
    table().iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
}
