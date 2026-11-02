// Region screenshot: capture all monitors, an overlay on each, result goes to clipboard/file/pin.

use crate::i18n::t;
use crate::{files, notify, pin, record, settings, shortcuts};
use image::{ImageEncoder, RgbaImage};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::Duration;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{Emitter, EventTarget, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Screenshot,
    Record,
}

/// Monitor in physical pixels
#[derive(Clone, Copy, PartialEq)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub scale: f64,
}

impl Monitor {
    /// Overlay window size in CSS pixels
    pub fn css_w(&self) -> f64 {
        self.w as f64 / self.scale
    }
    pub fn css_h(&self) -> f64 {
        self.h as f64 / self.scale
    }
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x as f64 && py >= self.y as f64 && px < (self.x + self.w as i32) as f64 && py < (self.y + self.h as i32) as f64
    }
}

#[derive(Clone, Copy, Deserialize, Serialize, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

struct Session {
    mon: Monitor,
    image: Arc<RgbaImage>,
    win: WebviewWindow,
}

#[derive(Default)]
struct State {
    active: bool, // overlay is open or being prepared
    token: u64,   // race guard: every close/start changes the token
    mode: Option<Mode>,
    sessions: HashMap<String, Session>, // key: window label
    shown: usize,
    ready: HashSet<String>, // windows whose page has loaded
    /// Preloaded hidden overlay windows, one per monitor
    pool: Vec<(Monitor, WebviewWindow)>,
    seq: u64,
    warm_gen: u64,
    quitting: bool,
    /// For latency measurement in dev builds
    started: Option<std::time::Instant>,
}

static ST: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));

// Important: Tauri window methods wait for the main thread, so never call them while holding ST.

pub fn init() {
    schedule_warm(1500); // don't get in the way of app startup
}

pub fn is_active() -> bool {
    ST.lock().unwrap().active
}

// ───────────────────────── Capture ─────────────────────────

fn monitors() -> Vec<(xcap::Monitor, Monitor)> {
    xcap::Monitor::all()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| {
            let info = Monitor {
                x: m.x().ok()?,
                y: m.y().ok()?,
                w: m.width().ok()?,
                h: m.height().ok()?,
                scale: m.scale_factor().ok()? as f64,
            };
            (info.w > 0 && info.h > 0 && info.scale > 0.0).then_some((m, info))
        })
        .collect()
}

/// All monitors in parallel, frames at native resolution.
fn grab_all() -> Option<Vec<(Monitor, RgbaImage)>> {
    let list: Vec<Monitor> = monitors().into_iter().map(|(_, info)| info).collect();
    if list.is_empty() {
        return None;
    }
    thread::scope(|s| {
        let jobs: Vec<_> = list
            .into_iter()
            .map(|info| {
                // An xcap monitor can't be sent across threads, so look it up again
                s.spawn(move || {
                    let m = xcap::Monitor::from_point(info.x + info.w as i32 / 2, info.y + info.h as i32 / 2).ok()?;
                    let img = m.capture_image().ok()?;
                    Some((info, img))
                })
            })
            .collect();
        jobs.into_iter().map(|j| j.join().ok().flatten()).collect()
    })
}

/// The monitor the cursor is currently on (otherwise the first one).
pub fn monitor_under_cursor() -> Option<Monitor> {
    let list: Vec<Monitor> = monitors().into_iter().map(|(_, m)| m).collect();
    let cursor = crate::app().cursor_position().ok();
    cursor.and_then(|c| list.iter().copied().find(|m| m.contains(c.x, c.y))).or_else(|| list.first().copied())
}

/// The monitor containing most of the rectangle (physical pixels).
fn monitor_for_rect(l: i32, t: i32, r: i32, b: i32) -> Option<Monitor> {
    let area = |m: &Monitor| -> i64 {
        let w = (r.min(m.x + m.w as i32) - l.max(m.x)).max(0) as i64;
        let h = (b.min(m.y + m.h as i32) - t.max(m.y)).max(0) as i64;
        w * h
    };
    let list: Vec<Monitor> = monitors().into_iter().map(|(_, m)| m).collect();
    list.into_iter().filter(|m| area(m) > 0).max_by_key(area)
}

/// Frame of a single monitor at native resolution.
fn grab_one(mon: &Monitor) -> Option<RgbaImage> {
    xcap::Monitor::from_point(mon.x + mon.w as i32 / 2, mon.y + mon.h as i32 / 2).ok()?.capture_image().ok()
}

/// Screenshot without the overlay: straight to the clipboard (and to a file if "Save on copy" is enabled).
/// Do nothing while the overlay is open: it would end up in the frame.
fn capture_instant(area: impl FnOnce() -> Option<(Monitor, Option<(i32, i32, i32, i32)>)>) {
    if is_active() {
        return;
    }
    let Some((mon, rect)) = area() else {
        notify::notify(&t("captureFailed"), None);
        return;
    };
    let Some(img) = grab_one(&mon) else {
        notify::notify(&t("captureFailed"), None);
        return;
    };
    // Region inside the monitor (physical pixels), otherwise the whole monitor
    let (l, t_, r, b) = match rect {
        Some((l, t_, r, b)) => (l.max(mon.x), t_.max(mon.y), r.min(mon.x + mon.w as i32), b.min(mon.y + mon.h as i32)),
        None => (mon.x, mon.y, mon.x + mon.w as i32, mon.y + mon.h as i32),
    };
    if r - l < 1 || b - t_ < 1 {
        notify::notify(&t("captureFailed"), None);
        return;
    }
    // The xcap frame may differ from the monitor size at fractional scaling: rescale
    let kx = img.width() as f64 / mon.w as f64;
    let ky = img.height() as f64 / mon.h as f64;
    let x = (((l - mon.x) as f64 * kx).round() as u32).min(img.width() - 1);
    let y = (((t_ - mon.y) as f64 * ky).round() as u32).min(img.height() - 1);
    let w = (((r - l) as f64 * kx).round() as u32).clamp(1, img.width() - x);
    let h = (((b - t_) as f64 * ky).round() as u32).clamp(1, img.height() - y);
    let out = if x == 0 && y == 0 && w == img.width() && h == img.height() {
        img
    } else {
        image::imageops::crop_imm(&img, x, y, w, h).to_image()
    };
    finish("copy", out, None, (l, t_, (r - l) as u32, (b - t_) as u32));
}

/// Hotkey: whole screen (the one with the cursor) straight to the clipboard.
pub fn capture_full() {
    capture_instant(|| monitor_under_cursor().map(|m| (m, None)));
}

/// Hotkey: active window straight to the clipboard. If there is no window (desktop), take the whole screen.
pub fn capture_window() {
    capture_instant(|| {
        #[cfg(windows)]
        if let Some((l, t_, r, b)) = crate::winapi::foreground_window_rect() {
            if let Some(mon) = monitor_for_rect(l, t_, r, b) {
                return Some((mon, Some((l, t_, r, b))));
            }
        }
        monitor_under_cursor().map(|m| (m, None))
    });
}

/// Hotkey or tray item. Pressing again while the overlay is open closes it.
pub fn start_capture(mode: Mode) {
    let my = {
        let mut s = ST.lock().unwrap();
        if s.active {
            drop(s);
            close_overlay();
            return;
        }
        s.active = true;
        s.started = Some(std::time::Instant::now());
        s.mode = Some(mode);
        s.token += 1;
        s.token
    };
    thread::spawn(move || {
        let t0 = std::time::Instant::now();
        let frames = grab_all();
        dlog!("захват: {} мониторов за {:?}", frames.as_ref().map_or(0, |f| f.len()), t0.elapsed());
        if ST.lock().unwrap().token != my {
            return; // cancelled while capturing
        }
        match frames {
            Some(list) => open_overlay(list, my),
            None => {
                ST.lock().unwrap().active = false;
                notify::notify(&t("captureFailed"), None);
            }
        }
    });
}

fn create_overlay_window(mon: &Monitor) -> Option<WebviewWindow> {
    let app = crate::app();
    let label = {
        let mut s = ST.lock().unwrap();
        s.seq += 1;
        format!("overlay-{}", s.seq)
    };
    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("overlay.html".into()))
        .title("StopPic")
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .shadow(false)
        .visible(false)
        .focused(false)
        .background_color(tauri::window::Color(0, 0, 0, 255))
        .disable_drag_drop_handler()
        .build()
        .map_err(|e| eprintln!("[capture] окно оверлея: {e}"))
        .ok()?;
    place(&win, mon);
    #[cfg(windows)]
    crate::winapi::disable_transitions(&win);

    let lbl = label.clone();
    win.on_window_event(move |e| {
        if let tauri::WindowEvent::Destroyed = e {
            ST.lock().unwrap().ready.remove(&lbl);
        }
    });
    Some(win)
}

/// Setting the bounds again fixes the wrong size on monitors with different DPI.
fn place(win: &WebviewWindow, mon: &Monitor) {
    for _ in 0..2 {
        let _ = win.set_position(PhysicalPosition::new(mon.x, mon.y));
        let _ = win.set_size(PhysicalSize::new(mon.w, mon.h));
    }
}

fn take_window(mon: &Monitor) -> Option<WebviewWindow> {
    let pooled = {
        let mut s = ST.lock().unwrap();
        s.pool.iter().position(|(m, _)| m == mon).map(|i| s.pool.remove(i).1)
    };
    pooled.or_else(|| create_overlay_window(mon))
}

#[derive(Serialize, Clone)]
struct InitPayload {
    labels: HashMap<&'static str, String>,
    mode: Mode,
    rec: RecInit,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RecInit {
    mic: bool,
    system_audio: bool,
    format: String,
}

fn init_payload(mode: Mode) -> InitPayload {
    let pairs = [
        ("copy", "labelCopy"),
        ("save", "labelSave"),
        ("close", "labelClose"),
        ("pin", "labelPin"),
        ("undo", "labelUndo"),
        ("redo", "labelRedo"),
        ("select", "labelSelect"),
        ("arrow", "labelArrow"),
        ("line", "labelLine"),
        ("rect", "labelRect"),
        ("marker", "labelMarker"),
        ("text", "labelText"),
        ("blur", "labelBlur"),
        ("number", "labelNumber"),
        ("pen", "labelPen"),
        ("customColor", "labelCustomColor"),
        ("pickColor", "labelPickColor"),
        ("image", "labelImage"),
        ("mic", "labelMic"),
        ("sys", "labelSys"),
        ("record", "labelRecord"),
    ];
    let st = settings::get();
    InitPayload {
        labels: pairs.into_iter().map(|(k, v)| (k, t(v))).collect(),
        mode,
        rec: RecInit { mic: st.rec_mic, system_audio: st.rec_system_audio, format: st.rec_format },
    }
}

/// Log from the window (dev only): latency measurements
#[tauri::command]
pub async fn dev_log(window: WebviewWindow, msg: String) {
    let since = ST.lock().unwrap().started.map(|t| t.elapsed().as_secs_f64() * 1000.0).unwrap_or(0.0);
    dlog!("{} @{:.0} мс: {}", window.label(), since, msg);
    let _ = (window, msg, since);
}

fn send_init(label: &str, mode: Mode) {
    dlog!("send_init {} @{:?}", label, ST.lock().unwrap().started.map(|t| t.elapsed()));
    #[cfg(windows)]
    {
        let found = {
            let s = ST.lock().unwrap();
            s.sessions.get(label).map(|x| (x.win.clone(), x.image.clone()))
        };
        if let Some((win, image)) = found {
            post_frame(win, image, init_payload(mode));
            return;
        }
    }
    emit_init(label, init_payload(mode));
}

/// Normal path: an event, then the window fetches the frame itself via the overlay_frame command (~140 ms for 14 MB).
fn emit_init(label: &str, payload: InitPayload) {
    let _ = crate::app().emit_to(EventTarget::webview_window(label), "overlay:init", payload);
}

/// Windows fast path: the frame goes into WebView2 shared memory, the window reads it without copying through IPC.
/// First 8 bytes: width and height (u32 LE), then RGBA. If WebView2 is too old, fall back to emit_init.
#[cfg(windows)]
fn post_frame(win: WebviewWindow, image: Arc<RgbaImage>, payload: InitPayload) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment12, ICoreWebView2_17, COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
    };
    use windows::core::{Interface, HSTRING, PCWSTR};

    let label = win.label().to_string();
    let json = serde_json::to_string(&payload).unwrap_or_default();
    let res = win.with_webview(move |wv| {
        let post = || -> windows::core::Result<()> {
            unsafe {
                let raw = image.as_raw();
                let env: ICoreWebView2Environment12 = wv.environment().cast()?;
                let buf = env.CreateSharedBuffer(8 + raw.len() as u64)?;
                let mut ptr: *mut u8 = std::ptr::null_mut();
                buf.Buffer(&mut ptr)?;
                let dst = std::slice::from_raw_parts_mut(ptr, 8 + raw.len());
                dst[0..4].copy_from_slice(&image.width().to_le_bytes());
                dst[4..8].copy_from_slice(&image.height().to_le_bytes());
                dst[8..].copy_from_slice(raw);
                let core: ICoreWebView2_17 = wv.controller().CoreWebView2()?.cast()?;
                let data = HSTRING::from(json.as_str());
                core.PostSharedBufferToScript(&buf, COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY, PCWSTR(data.as_ptr()))?;
                // Our reference is no longer needed: the page has its own and will release it itself
                buf.Close()?;
            }
            Ok(())
        };
        if let Err(e) = post() {
            eprintln!("[capture] общая память WebView2 недоступна, обычный путь: {e}");
            if let Ok(p) = serde_json::from_str::<serde_json::Value>(&json) {
                let _ = crate::app().emit_to(EventTarget::webview_window(&label), "overlay:init", p);
            }
        }
    });
    if let Err(e) = res {
        eprintln!("[capture] with_webview: {e}");
    }
}

fn open_overlay(list: Vec<(Monitor, RgbaImage)>, my: u64) {
    dlog!("открываю оверлей");
    let mut opened = Vec::new();
    for (mon, image) in list {
        match take_window(&mon) {
            Some(win) => opened.push(Session { mon, image: Arc::new(image), win }),
            None => {
                for s in opened {
                    let _ = s.win.destroy();
                }
                ST.lock().unwrap().active = false;
                notify::notify(&t("captureFailed"), None);
                return;
            }
        }
    }

    let mut to_init = Vec::new();
    let stale = {
        let mut s = ST.lock().unwrap();
        if s.token != my {
            Some(opened)
        } else {
            s.shown = 0;
            for sess in opened {
                let label = sess.win.label().to_string();
                // Page already loaded (window was prepared in advance): hand over the frame right away
                if s.ready.contains(&label) {
                    to_init.push(label.clone());
                }
                s.sessions.insert(label, sess);
            }
            None
        }
    };
    if let Some(list) = stale {
        for s in list {
            let _ = s.win.destroy();
        }
        return;
    }
    let mode = ST.lock().unwrap().mode.unwrap_or(Mode::Screenshot);
    for label in to_init {
        send_init(&label, mode);
    }
    shortcuts::escape_on();

    // Safety net: if the overlay never showed up, don't leave the app "hung"
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(5));
        let stuck = {
            let s = ST.lock().unwrap();
            s.token == my && s.active && s.shown < s.sessions.len()
        };
        if stuck {
            close_overlay();
            notify::notify(&t("captureFailed"), None);
        }
    });
}

pub fn close_overlay() {
    let wins: Vec<WebviewWindow> = {
        let mut s = ST.lock().unwrap();
        s.token += 1;
        s.active = false;
        s.sessions.drain().map(|(_, sess)| sess.win).collect()
    };
    shortcuts::escape_off();
    for w in wins {
        let _ = w.destroy();
    }
    schedule_warm(600);
}

// ───────────────────────── Overlay preloading ─────────────────────────
// Creating the window and loading the page take most of the delay after the hotkey.
// So we keep a hidden window with an already loaded page for each monitor in advance.

fn destroy_pool() {
    let pool: Vec<_> = ST.lock().unwrap().pool.drain(..).collect();
    for (_, w) in pool {
        let _ = w.destroy();
    }
}

fn warm() {
    {
        let s = ST.lock().unwrap();
        if s.quitting || s.active {
            return;
        }
    }
    let current: Vec<Monitor> = monitors().into_iter().map(|(_, m)| m).collect();
    // Monitor was disconnected or changed: such windows are unusable
    let stale: Vec<WebviewWindow> = {
        let mut s = ST.lock().unwrap();
        let (keep, drop): (Vec<_>, Vec<_>) = s.pool.drain(..).partition(|(m, _)| current.contains(m));
        s.pool = keep;
        drop.into_iter().map(|(_, w)| w).collect()
    };
    for w in stale {
        let _ = w.destroy();
    }
    for mon in current {
        let has = ST.lock().unwrap().pool.iter().any(|(m, _)| *m == mon);
        if !has {
            if let Some(w) = create_overlay_window(&mon) {
                let mut s = ST.lock().unwrap();
                if s.active || s.quitting {
                    drop(s);
                    let _ = w.destroy();
                    return;
                }
                s.pool.push((mon, w));
            }
        }
    }
}

fn schedule_warm(delay_ms: u64) {
    let gen = {
        let mut s = ST.lock().unwrap();
        if s.quitting {
            return;
        }
        s.warm_gen += 1;
        s.warm_gen
    };
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(delay_ms));
        if ST.lock().unwrap().warm_gen == gen {
            warm();
        }
    });
}

pub fn shutdown() {
    ST.lock().unwrap().quitting = true;
    close_overlay();
    destroy_pool();
}

// ───────────────────────── Helpers ─────────────────────────

fn session_mon(label: &str) -> Option<(Monitor, Mode)> {
    let s = ST.lock().unwrap();
    s.sessions.get(label).map(|x| (x.mon, s.mode.unwrap_or(Mode::Screenshot)))
}

/// Validates a rectangle from the window and clamps it to the monitor bounds (CSS pixels).
fn clamp_rect(r: Rect, mon: &Monitor) -> Option<Rect> {
    if ![r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite()) {
        return None;
    }
    let (w, h) = (mon.css_w(), mon.css_h());
    let left = r.x.clamp(0.0, w);
    let top = r.y.clamp(0.0, h);
    let right = (r.x + r.width).clamp(0.0, w);
    let bottom = (r.y + r.height).clamp(0.0, h);
    if right - left < 1.0 || bottom - top < 1.0 {
        return None;
    }
    Some(Rect { x: left, y: top, width: right - left, height: bottom - top })
}

struct Crop {
    image: RgbaImage,
    left: u32,
    top: u32,
    kx: f64,
    ky: f64,
}

/// Converts a rectangle from window CSS pixels to physical frame pixels.
fn crop(img: &RgbaImage, mon: &Monitor, r: Rect) -> Crop {
    let kx = img.width() as f64 / mon.css_w();
    let ky = img.height() as f64 / mon.css_h();
    let left = ((r.x * kx).round() as u32).min(img.width() - 1);
    let top = ((r.y * ky).round() as u32).min(img.height() - 1);
    let right = (((r.x + r.width) * kx).round() as u32).min(img.width());
    let bottom = (((r.y + r.height) * ky).round() as u32).min(img.height());
    let image = image::imageops::crop_imm(img, left, top, right.saturating_sub(left).max(1), bottom.saturating_sub(top).max(1)).to_image();
    Crop { image, left, top, kx, ky }
}

/// Screen coordinates of the selection in physical pixels (for the "Pin" window).
fn placement(mon: &Monitor, r: Rect) -> (i32, i32, u32, u32) {
    (
        mon.x + (r.x * mon.scale).round() as i32,
        mon.y + (r.y * mon.scale).round() as i32,
        (r.width * mon.scale).round() as u32,
        (r.height * mon.scale).round() as u32,
    )
}

pub fn encode_png(img: &RgbaImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Adaptive)
        .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(out)
}

fn copy_to_clipboard(img: &RgbaImage) -> Result<(), arboard::Error> {
    arboard::Clipboard::new()?.set_image(arboard::ImageData {
        width: img.width() as usize,
        height: img.height() as usize,
        bytes: std::borrow::Cow::Borrowed(img.as_raw()),
    })
}

/// Copy / save / pin. png: an already encoded PNG, if there is one.
fn finish(kind: &str, img: RgbaImage, png: Option<Vec<u8>>, place: (i32, i32, u32, u32)) {
    let st = settings::get();
    let dir = PathBuf::from(&st.save_dir);
    let save = |png: Option<Vec<u8>>| -> Option<PathBuf> {
        let data = png.or_else(|| encode_png(&img))?;
        files::write_unique(&dir, &format!("Screenshot {}", files::timestamp()), "png", &data)
            .map_err(|e| eprintln!("[capture] сохранение не удалось: {e}"))
            .ok()
    };
    match kind {
        "pin" => match png.or_else(|| encode_png(&img)) {
            Some(data) => pin::create(data, place),
            None => notify::notify(&t("saveFailed"), None),
        },
        "copy" => {
            if let Err(e) = copy_to_clipboard(&img) {
                eprintln!("[capture] буфер обмена: {e}");
                notify::notify(&t("saveFailed"), None);
                return;
            }
            if st.auto_save {
                match save(png) {
                    Some(file) => notify::notify(&t("copiedAndSaved"), Some(files::reveal(file))),
                    None => notify::notify(&t("saveFailed"), None),
                }
            } else {
                notify::notify(&t("copied"), None);
            }
        }
        _ => match save(png) {
            Some(file) => notify::notify(&t("saved"), Some(files::reveal(file))),
            None => notify::notify(&t("saveFailed"), None),
        },
    }
}

fn is_action(kind: &str) -> bool {
    matches!(kind, "copy" | "save" | "pin")
}

fn empty() -> Response {
    Response::new(Vec::new())
}

// ───────────────────────── Overlay window commands ─────────────────────────
// All commands are async: that way they run off the main thread and can wait for windows.

#[tauri::command]
pub async fn overlay_ready(window: WebviewWindow) {
    dlog!("overlay_ready {}", window.label());
    let label = window.label().to_string();
    let mode = {
        let mut s = ST.lock().unwrap();
        s.ready.insert(label.clone());
        s.sessions.contains_key(&label).then(|| s.mode.unwrap_or(Mode::Screenshot))
    };
    if let Some(mode) = mode {
        send_init(&label, mode);
    }
}

#[tauri::command]
/// Frame for display: 8 bytes (width, height: u32 LE), then uncompressed RGBA.
/// Encoding JPEG takes longer than sending raw pixels.
pub async fn overlay_frame(window: WebviewWindow) -> Response {
    let s = ST.lock().unwrap();
    let Some(sess) = s.sessions.get(window.label()) else { return empty() };
    let img = &sess.image;
    let mut out = Vec::with_capacity(8 + img.as_raw().len());
    out.extend_from_slice(&img.width().to_le_bytes());
    out.extend_from_slice(&img.height().to_le_bytes());
    out.extend_from_slice(img.as_raw());
    Response::new(out)
}

#[tauri::command]
pub async fn overlay_shown(window: WebviewWindow) {
    dlog!("overlay_shown {} (через {:?} после хоткея)", window.label(), ST.lock().unwrap().started.map(|t| t.elapsed()));
    let Some((mon, _)) = session_mon(window.label()) else { return };
    #[cfg(windows)]
    crate::winapi::set_alpha(&window, Some(0));
    let _ = window.show();
    let cursor = crate::app().cursor_position().ok();
    if cursor.is_none_or(|c| mon.contains(c.x, c.y)) {
        let _ = window.set_focus();
    }
    let _ = window.emit_to(EventTarget::webview_window(window.label()), "overlay:reveal", ());
    // Safety net: an invisible window on top of everything must not intercept the mouse
    let w = window.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        #[cfg(windows)]
        crate::winapi::set_alpha(&w, None);
        #[cfg(not(windows))]
        let _ = w;
    });
}

#[tauri::command]
pub async fn overlay_revealed(window: WebviewWindow) {
    dlog!("overlay_revealed {}", window.label());
    if session_mon(window.label()).is_none() {
        return;
    }
    #[cfg(windows)]
    crate::winapi::set_alpha(&window, None);
    ST.lock().unwrap().shown += 1;
}

#[tauri::command]
pub async fn overlay_cancel(window: WebviewWindow) {
    if session_mon(window.label()).is_some() {
        close_overlay();
    }
}

/// Screenshot without annotations: crop here, from the lossless original
#[tauri::command]
pub async fn overlay_action(window: WebviewWindow, kind: String, rect: Rect) {
    if !is_action(&kind) {
        return;
    }
    let cropped = {
        let s = ST.lock().unwrap();
        if s.mode != Some(Mode::Screenshot) {
            return;
        }
        let Some(sess) = s.sessions.get(window.label()) else { return };
        let Some(r) = clamp_rect(rect, &sess.mon) else { return };
        (crop(&sess.image, &sess.mon, r).image, placement(&sess.mon, r))
    };
    close_overlay();
    let (img, place) = cropped;
    thread::spawn(move || finish(&kind, img, None, place));
}

/// Screenshot with annotations: hand the window a lossless crop, it draws on top and sends back the result.
/// First 32 bytes of the response: left, top, kx, ky (f64 LE), then PNG.
#[tauri::command]
pub async fn overlay_get_crop(window: WebviewWindow, rect: Rect) -> Response {
    let c = {
        let s = ST.lock().unwrap();
        let Some(sess) = s.sessions.get(window.label()) else { return empty() };
        let Some(r) = clamp_rect(rect, &sess.mon) else { return empty() };
        crop(&sess.image, &sess.mon, r)
    };
    let Some(png) = encode_png(&c.image) else { return empty() };
    let mut out = Vec::with_capacity(32 + png.len());
    for v in [c.left as f64, c.top as f64, c.kx, c.ky] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&png);
    Response::new(out)
}

#[tauri::command]
pub async fn overlay_submit(window: WebviewWindow, request: Request<'_>) -> Result<(), String> {
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(String::from);
    let kind = header("x-kind").unwrap_or_default();
    let rect: Option<Rect> = header("x-rect").and_then(|r| serde_json::from_str(&r).ok());
    let InvokeBody::Raw(png) = request.body() else { return Err("ожидались байты PNG".into()) };
    if !is_action(&kind) || png.is_empty() || png.len() > 300 * 1024 * 1024 {
        return Ok(());
    }
    let place = {
        let s = ST.lock().unwrap();
        if s.mode != Some(Mode::Screenshot) {
            return Ok(());
        }
        let Some(sess) = s.sessions.get(window.label()) else { return Ok(()) };
        let Some(r) = rect.and_then(|r| clamp_rect(r, &sess.mon)) else { return Ok(()) };
        placement(&sess.mon, r)
    };
    let png = png.clone();
    close_overlay();
    thread::spawn(move || match image::load_from_memory_with_format(&png, image::ImageFormat::Png) {
        Ok(img) => finish(&kind, img.to_rgba8(), Some(png), place),
        Err(e) => {
            eprintln!("[capture] итоговый PNG не читается: {e}");
            notify::notify(&t("saveFailed"), None);
        }
    });
    Ok(())
}

/// While text is being edited, Esc must not close the whole overlay
#[tauri::command]
pub async fn overlay_escape(window: WebviewWindow, enabled: bool) {
    if session_mon(window.label()).is_none() {
        return;
    }
    if enabled {
        shortcuts::escape_on();
    } else {
        shortcuts::escape_off();
    }
}

/// Pasting an image onto the screenshot: from the clipboard
#[tauri::command]
pub async fn overlay_clipboard_image(window: WebviewWindow) -> Response {
    if session_mon(window.label()).is_none_or(|(_, m)| m != Mode::Screenshot) {
        return empty();
    }
    let img = arboard::Clipboard::new().and_then(|mut c| c.get_image()).ok();
    let png = img.and_then(|d| {
        let rgba = RgbaImage::from_raw(d.width as u32, d.height as u32, d.bytes.into_owned())?;
        encode_png(&rgba)
    });
    Response::new(png.unwrap_or_default())
}

/// ...or from a file
#[tauri::command]
pub async fn overlay_pick_image(window: WebviewWindow) -> Response {
    use tauri_plugin_dialog::DialogExt;
    if session_mon(window.label()).is_none_or(|(_, m)| m != Mode::Screenshot) {
        return empty();
    }
    // The overlay sits on top of everything: lower it for the dialog and release Esc,
    // otherwise the dialog would end up under the overlay and Esc would close the whole screenshot
    shortcuts::escape_off();
    let _ = window.set_always_on_top(false);
    let w = window.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        crate::app()
            .dialog()
            .file()
            .set_parent(&w)
            .set_title(t("labelImage"))
            .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
            .blocking_pick_file()
    })
    .await
    .ok()
    .flatten();
    if session_mon(window.label()).is_some() {
        let _ = window.set_always_on_top(true);
        let _ = window.set_focus();
        if is_active() {
            shortcuts::escape_on();
        }
    }
    let data = picked.and_then(|p| p.into_path().ok()).and_then(|path| {
        let meta = std::fs::metadata(&path).ok()?;
        if meta.len() > 30 * 1024 * 1024 {
            return None;
        }
        std::fs::read(path).ok()
    });
    Response::new(data.unwrap_or_default())
}

/// Record mode: region selected, close the overlay and hand control over to recording
#[tauri::command(rename_all = "camelCase")]
pub async fn overlay_record(window: WebviewWindow, rect: Rect, mic: bool, system_audio: bool) {
    let Some((mon, mode)) = session_mon(window.label()) else { return };
    if mode != Mode::Record {
        return;
    }
    let Some(r) = clamp_rect(rect, &mon) else { return };
    close_overlay();
    record::begin(mon, r, mic, system_audio);
}
