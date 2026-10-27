// Глобальные горячие клавиши. Сочетания хранятся в формате Electron («CommandOrControl+Alt+A»),
// так их записывает окно настроек и так они лежат в файле настроек.

use crate::i18n::t_with;
use crate::{capture, notify, record, settings, settings_window};
use serde::Serialize;
use std::sync::Mutex;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Name {
    Screenshot,
    Record,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reg {
    Ok,
    Busy,
    Invalid,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum HotkeyResult {
    Ok { ok: bool },
    Err { ok: bool, error: &'static str },
}

impl HotkeyResult {
    fn ok() -> Self {
        Self::Ok { ok: true }
    }
    fn err(error: &'static str) -> Self {
        Self::Err { ok: false, error }
    }
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }
}

/// Только то, что зарегистрировали мы: Esc оверлея учитывается отдельно
static REGISTERED: Mutex<Vec<(Shortcut, Name)>> = Mutex::new(Vec::new());

fn escape() -> Shortcut {
    Shortcut::new(None, Code::Escape)
}

pub fn on_shortcut(_app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    dlog!("хоткей {:?} {:?}", shortcut, event.state());
    if event.state() != ShortcutState::Pressed {
        return;
    }
    // Плагин вызывает обработчик, удерживая свою блокировку. Наши действия регистрируют
    // и снимают сочетания (Esc), поэтому выполняем их в отдельном потоке, иначе взаимная блокировка
    let shortcut = *shortcut;
    std::thread::spawn(move || {
        if shortcut == escape() {
            capture::close_overlay();
            return;
        }
        let name = REGISTERED.lock().unwrap().iter().find(|(s, _)| *s == shortcut).map(|(_, n)| *n);
        match name {
            Some(Name::Screenshot) => capture::start_capture(capture::Mode::Screenshot),
            Some(Name::Record) => record::toggle(),
            None => {}
        }
    });
}

// ───────────────────────── Разбор сочетаний ─────────────────────────

fn key_code(key: &str) -> Option<Code> {
    use Code::*;
    let letters = [KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ];
    let digits = [Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9];
    let fkeys = [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24];
    let b = key.as_bytes();
    if b.len() == 1 && b[0].is_ascii_uppercase() {
        return Some(letters[(b[0] - b'A') as usize]);
    }
    if b.len() == 1 && b[0].is_ascii_digit() {
        return Some(digits[(b[0] - b'0') as usize]);
    }
    if let Some(n) = key.strip_prefix('F').and_then(|n| n.parse::<usize>().ok()) {
        return (1..=24).contains(&n).then(|| fkeys[n - 1]);
    }
    Some(match key {
        "Space" => Space,
        "Tab" => Tab,
        "Return" => Enter,
        "Backspace" => Backspace,
        "Delete" => Delete,
        "Insert" => Insert,
        "Home" => Home,
        "End" => End,
        "PageUp" => PageUp,
        "PageDown" => PageDown,
        "Up" => ArrowUp,
        "Down" => ArrowDown,
        "Left" => ArrowLeft,
        "Right" => ArrowRight,
        "PrintScreen" => PrintScreen,
        "-" => Minus,
        "=" => Equal,
        "[" => BracketLeft,
        "]" => BracketRight,
        "\\" => Backslash,
        ";" => Semicolon,
        "'" => Quote,
        "," => Comma,
        "." => Period,
        "/" => Slash,
        "`" => Backquote,
        _ => return None,
    })
}

fn modifier(m: &str) -> Option<Modifiers> {
    Some(match m {
        "CommandOrControl" => {
            if cfg!(target_os = "macos") {
                Modifiers::SUPER
            } else {
                Modifiers::CONTROL
            }
        }
        "Control" => Modifiers::CONTROL,
        "Alt" => Modifiers::ALT,
        "Shift" => Modifiers::SHIFT,
        "Super" => Modifiers::SUPER,
        _ => return None,
    })
}

/// Строгая проверка строки сочетания (она приходит из окна настроек) и перевод в Shortcut.
pub fn parse(accel: &str) -> Option<Shortcut> {
    if accel.is_empty() || accel.len() > 60 {
        return None;
    }
    let mut parts: Vec<&str> = accel.split('+').collect();
    // «Ctrl++» не поддерживаем, как и в Electron-версии
    let key = parts.pop()?;
    let code = key_code(key)?;
    let mut mods = Modifiers::empty();
    let mut seen = Vec::new();
    for p in &parts {
        if seen.contains(p) {
            return None;
        }
        seen.push(*p);
        mods |= modifier(p)?;
    }
    // Без модификатора допустимы только F-клавиши и PrintScreen
    let free = key.starts_with('F') && key.len() > 1 || key == "PrintScreen";
    if mods.is_empty() && !free {
        return None;
    }
    Some(Shortcut::new((!mods.is_empty()).then_some(mods), code))
}

/// Для показа в уведомлениях: CommandOrControl → Ctrl / Cmd.
pub fn pretty(a: &str) -> String {
    let mac = cfg!(target_os = "macos");
    a.replace("CommandOrControl", if mac { "Cmd" } else { "Ctrl" }).replace("Alt", if mac { "Option" } else { "Alt" })
}

/// Для подписи в меню трея (формат muda).
pub fn menu_accel(a: &str) -> Option<String> {
    parse(a)?;
    let mut parts: Vec<String> = a.split('+').map(String::from).collect();
    let key = parts.pop()?;
    let key = match key.as_str() {
        "Return" => "Enter".to_string(),
        "Up" | "Down" | "Left" | "Right" => format!("Arrow{key}"),
        "-" => "Minus".into(),
        "=" => "Equal".into(),
        "[" => "BracketLeft".into(),
        "]" => "BracketRight".into(),
        "\\" => "Backslash".into(),
        ";" => "Semicolon".into(),
        "'" => "Quote".into(),
        "," => "Comma".into(),
        "." => "Period".into(),
        "/" => "Slash".into(),
        "`" => "Backquote".into(),
        _ => key,
    };
    let mods: Vec<String> = parts.into_iter().map(|m| if m == "CommandOrControl" { "CmdOrCtrl".into() } else { m }).collect();
    Some([mods, vec![key]].concat().join("+"))
}

// ───────────────────────── Регистрация ─────────────────────────

fn unregister_ours() {
    let gs = crate::app().global_shortcut();
    for (s, _) in REGISTERED.lock().unwrap().drain(..) {
        let _ = gs.unregister(s);
    }
}

fn try_register(hk: &settings::Hotkeys) -> [Reg; 2] {
    unregister_ours();
    let gs = crate::app().global_shortcut();
    let mut out = [Reg::Invalid; 2];
    for (i, (name, accel)) in [(Name::Screenshot, &hk.screenshot), (Name::Record, &hk.record)].into_iter().enumerate() {
        let Some(sc) = parse(accel) else { continue };
        // Ошибка: сочетание занято системой или другой программой
        let res = gs.register(sc);
        dlog!("регистрация {accel}: {res:?}");
        out[i] = if res.is_ok() {
            REGISTERED.lock().unwrap().push((sc, name));
            Reg::Ok
        } else {
            Reg::Busy
        };
    }
    out
}

/// Регистрация при запуске. Если что-то занято, уведомление ведёт в окно настроек.
pub fn register_at_start() {
    let hk = settings::get().hotkeys;
    let res = try_register(&hk);
    for (r, accel) in res.iter().zip([&hk.screenshot, &hk.record]) {
        if *r != Reg::Ok {
            notify::notify(&t_with("hotkeyBusy", &[("key", &pretty(accel))]), Some(Box::new(settings_window::open)));
        }
    }
}

/// Пока в окне настроек записывается новое сочетание, старые не должны срабатывать.
pub fn pause() {
    unregister_ours();
}

pub fn resume() {
    try_register(&settings::get().hotkeys);
}

/// Назначает сочетание. При неудаче возвращает прежнее состояние.
pub fn set_hotkey(name: Name, accel: &str) -> HotkeyResult {
    if parse(accel).is_none() {
        return HotkeyResult::err("invalid");
    }
    let prev = settings::get().hotkeys;
    let other = if name == Name::Screenshot { &prev.record } else { &prev.screenshot };
    if accel == other {
        return HotkeyResult::err("duplicate");
    }
    let mut next = prev.clone();
    match name {
        Name::Screenshot => next.screenshot = accel.to_string(),
        Name::Record => next.record = accel.to_string(),
    }
    let res = try_register(&next);
    let mine = res[if name == Name::Screenshot { 0 } else { 1 }];
    if mine != Reg::Ok {
        try_register(&prev);
        return HotkeyResult::err(if mine == Reg::Invalid { "invalid" } else { "busy" });
    }
    settings::update(|s| s.hotkeys = next);
    HotkeyResult::ok()
}

/// Esc закрывает оверлей, даже если фокус на другом мониторе.
pub fn escape_on() {
    let gs = crate::app().global_shortcut();
    if !gs.is_registered(escape()) {
        let _ = gs.register(escape());
    }
}

pub fn escape_off() {
    let gs = crate::app().global_shortcut();
    if gs.is_registered(escape()) {
        let _ = gs.unregister(escape());
    }
}
