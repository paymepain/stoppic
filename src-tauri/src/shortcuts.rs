// Global hotkeys. Combinations are stored in Electron format ("CommandOrControl+Alt+A"),
// which is how the settings window records them and how they sit in the settings file.

use crate::i18n::t_with;
use crate::{capture, notify, record, settings, settings_window};
use serde::Serialize;
use std::sync::Mutex;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Name {
    Screenshot,
    ScreenshotFull,
    ScreenshotWindow,
    Record,
    RecordFull,
}

pub const NAMES: [Name; 5] = [Name::Screenshot, Name::ScreenshotFull, Name::ScreenshotWindow, Name::Record, Name::RecordFull];

impl Name {
    /// Key in the settings file and in the settings window
    pub fn key(self) -> &'static str {
        match self {
            Name::Screenshot => "screenshot",
            Name::ScreenshotFull => "screenshotFull",
            Name::ScreenshotWindow => "screenshotWindow",
            Name::Record => "record",
            Name::RecordFull => "recordFull",
        }
    }

    pub fn from_key(key: &str) -> Option<Name> {
        NAMES.into_iter().find(|n| n.key() == key)
    }

    pub fn get(self, hk: &settings::Hotkeys) -> &str {
        match self {
            Name::Screenshot => &hk.screenshot,
            Name::ScreenshotFull => &hk.screenshot_full,
            Name::ScreenshotWindow => &hk.screenshot_window,
            Name::Record => &hk.record,
            Name::RecordFull => &hk.record_full,
        }
    }

    fn set(self, hk: &mut settings::Hotkeys, accel: &str) {
        let slot = match self {
            Name::Screenshot => &mut hk.screenshot,
            Name::ScreenshotFull => &mut hk.screenshot_full,
            Name::ScreenshotWindow => &mut hk.screenshot_window,
            Name::Record => &mut hk.record,
            Name::RecordFull => &mut hk.record_full,
        };
        *slot = accel.to_string();
    }
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

/// Only what we registered ourselves: the overlay's Esc is tracked separately
static REGISTERED: Mutex<Vec<(Shortcut, Name)>> = Mutex::new(Vec::new());

fn escape() -> Shortcut {
    Shortcut::new(None, Code::Escape)
}

pub fn on_shortcut(_app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    dlog!("хоткей {:?} {:?}", shortcut, event.state());
    if event.state() != ShortcutState::Pressed {
        return;
    }
    // The plugin invokes the handler while holding its own lock. Our actions register
    // and unregister combinations (Esc), so run them on a separate thread, otherwise deadlock
    let shortcut = *shortcut;
    std::thread::spawn(move || {
        if shortcut == escape() {
            capture::close_overlay();
            return;
        }
        let name = REGISTERED.lock().unwrap().iter().find(|(s, _)| *s == shortcut).map(|(_, n)| *n);
        match name {
            Some(Name::Screenshot) => capture::start_capture(capture::Mode::Screenshot),
            Some(Name::ScreenshotFull) => capture::capture_full(),
            Some(Name::ScreenshotWindow) => capture::capture_window(),
            Some(Name::Record) => record::toggle(),
            Some(Name::RecordFull) => record::toggle_full(),
            None => {}
        }
    });
}

// ───────────────────────── Parsing combinations ─────────────────────────

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

/// Strict validation of a combination string (it comes from the settings window) and conversion to Shortcut.
pub fn parse(accel: &str) -> Option<Shortcut> {
    if accel.is_empty() || accel.len() > 60 {
        return None;
    }
    let mut parts: Vec<&str> = accel.split('+').collect();
    // "Ctrl++" is not supported, same as in the Electron version
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
    // Without a modifier only F-keys and PrintScreen are allowed
    let free = key.starts_with('F') && key.len() > 1 || key == "PrintScreen";
    if mods.is_empty() && !free {
        return None;
    }
    Some(Shortcut::new((!mods.is_empty()).then_some(mods), code))
}

/// For display in notifications: CommandOrControl → Ctrl / Cmd.
pub fn pretty(a: &str) -> String {
    let mac = cfg!(target_os = "macos");
    a.replace("CommandOrControl", if mac { "Cmd" } else { "Ctrl" }).replace("Alt", if mac { "Option" } else { "Alt" })
}

/// For the tray menu label (muda format).
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

// ───────────────────────── Registration ─────────────────────────

fn unregister_ours() {
    let gs = crate::app().global_shortcut();
    for (s, _) in REGISTERED.lock().unwrap().drain(..) {
        let _ = gs.unregister(s);
    }
}

fn try_register(hk: &settings::Hotkeys) -> [Reg; NAMES.len()] {
    unregister_ours();
    let gs = crate::app().global_shortcut();
    let mut out = [Reg::Invalid; NAMES.len()];
    for (i, name) in NAMES.into_iter().enumerate() {
        let accel = name.get(hk);
        if accel.is_empty() {
            out[i] = Reg::Ok; // combination not assigned: not an error
            continue;
        }
        let Some(sc) = parse(accel) else { continue };
        // Error: the combination is taken by the system or another program
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

/// Registration at startup. If something is taken, the notification leads to the settings window.
pub fn register_at_start() {
    let hk = settings::get().hotkeys;
    let res = try_register(&hk);
    for (r, name) in res.iter().zip(NAMES) {
        if *r != Reg::Ok {
            notify::notify(&t_with("hotkeyBusy", &[("key", &pretty(name.get(&hk)))]), Some(Box::new(settings_window::open)));
        }
    }
}

/// While a new combination is being recorded in the settings window, the old ones must not fire.
pub fn pause() {
    unregister_ours();
}

pub fn resume() {
    try_register(&settings::get().hotkeys);
}

/// Assigns a combination. On failure restores the previous state.
pub fn set_hotkey(name: Name, accel: &str) -> HotkeyResult {
    if parse(accel).is_none() {
        return HotkeyResult::err("invalid");
    }
    let prev = settings::get().hotkeys;
    if NAMES.into_iter().any(|n| n != name && n.get(&prev) == accel) {
        return HotkeyResult::err("duplicate");
    }
    let mut next = prev.clone();
    name.set(&mut next, accel);
    let res = try_register(&next);
    let mine = res[NAMES.iter().position(|n| *n == name).unwrap_or(0)];
    if mine != Reg::Ok {
        try_register(&prev);
        return HotkeyResult::err(if mine == Reg::Invalid { "invalid" } else { "busy" });
    }
    settings::update(|s| s.hotkeys = next);
    HotkeyResult::ok()
}

/// Esc closes the overlay even if the focus is on another monitor.
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
