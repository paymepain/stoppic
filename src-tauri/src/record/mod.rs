// Screen recording: state, the HUD panel and saving. The capture itself lives in engine_*.

use crate::capture::{self, Monitor, Rect};
use crate::i18n::{self, t, t_with};
use crate::{files, notify, settings, tray};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{Emitter, EventTarget, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[cfg(windows)]
mod engine_windows;
#[cfg(windows)]
use engine_windows as engine;

#[cfg(not(windows))]
mod engine {
    pub struct Engine;
    impl Engine {
        pub fn prepare(_: &super::Spec) -> Result<(Self, Vec<&'static str>), String> {
            Err("unsupported".into())
        }
        pub fn go(&mut self) -> Result<(), String> {
            Err("unsupported".into())
        }
        pub fn pause(&self) {}
        pub fn resume(&self) {}
        pub fn finish(self, _: impl Fn(u32)) -> Result<(), String> {
            Ok(())
        }
        pub fn cancel(self) {}
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Countdown,
    Recording,
    Paused,
    Processing,
}

/// What to record and how
#[derive(Clone)]
pub struct Spec {
    pub mon: Monitor,
    /// Region in physical monitor pixels; dimensions are even (H.264 requires it)
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub fps: u32,
    pub gif: bool,
    pub mic: bool,
    pub system_audio: bool,
    /// WASAPI device ids; an empty string means the default device
    pub mic_device: String,
    pub sys_device: String,
    pub file: PathBuf,
}

/// Audio device for the list in settings
#[derive(Serialize, Clone)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
}

/// Active devices: microphones and output devices (for system audio).
pub fn audio_devices() -> (Vec<AudioDevice>, Vec<AudioDevice>) {
    #[cfg(windows)]
    {
        engine::audio_devices()
    }
    #[cfg(not(windows))]
    {
        (Vec::new(), Vec::new())
    }
}

type Prepared = Result<(engine::Engine, Vec<&'static str>), String>;

struct Job {
    spec: Spec,
    engine: Option<engine::Engine>,
    /// Engine preparation runs during the countdown; the result arrives here
    prep: Option<Receiver<Prepared>>,
}

struct State {
    phase: Phase,
    job: Option<Job>,
}

static ST: Mutex<State> = Mutex::new(State { phase: Phase::Idle, job: None });

const HUD: &str = "rec";
/// Dashed frame around the recorded region
const FRAME: &str = "rec-frame";
/// Same height as the overlay bar: 34 for buttons + 4 padding on each side
const HUD_W: f64 = 260.0;
const HUD_H: f64 = 42.0;

fn phase() -> Phase {
    ST.lock().unwrap().phase
}

fn set_phase(next: Phase) {
    {
        let mut s = ST.lock().unwrap();
        if s.phase == next {
            return;
        }
        s.phase = next;
    }
    tray::rebuild();
    tray::set_recording(is_recording());
}

/// Countdown, recording or pause is in progress (post-stop processing is not included).
pub fn is_recording() -> bool {
    matches!(phase(), Phase::Countdown | Phase::Recording | Phase::Paused)
}

fn hud() -> Option<WebviewWindow> {
    crate::app().get_webview_window(HUD)
}

fn send_cmd(cmd: &str) {
    let _ = crate::app().emit_to(EventTarget::webview_window(HUD), "rec:cmd", cmd);
}

/// Hotkey and tray item: start region selection, or stop while recording.
pub fn toggle() {
    toggle_with(|| capture::start_capture(capture::Mode::Record));
}

/// "Record full screen" hotkey: no region selection, the screen under the cursor. Stops while recording.
pub fn toggle_full() {
    toggle_with(|| {
        if capture::is_active() {
            return; // overlay is open: it decides what to record
        }
        let Some(mon) = capture::monitor_under_cursor() else {
            notify::notify(&t("recFailed"), None);
            return;
        };
        let st = settings::get();
        let rect = Rect { x: 0.0, y: 0.0, width: mon.css_w(), height: mon.css_h() };
        begin(mon, rect, st.rec_mic, st.rec_system_audio);
    });
}

fn toggle_with(start: impl FnOnce()) {
    match phase() {
        Phase::Processing => notify::notify(&t("recBusy"), None),
        Phase::Countdown => {
            if hud().is_some() {
                send_cmd("cancel");
            } else {
                discard();
            }
        }
        Phase::Recording | Phase::Paused => {
            if hud().is_some() {
                send_cmd("stop"); // the panel shows "Processing..." and calls rec_command('stop')
            } else {
                stop();
            }
        }
        Phase::Idle => start(),
    }
}

// ───────────────────────── Recording panel (HUD) ─────────────────────────

/// Same place as the overlay bar (placeBar in overlay.ts): below the region at the right edge,
/// if it does not fit, above the region, and only then inside it. On Windows the panel is excluded from the frame anyway.
fn hud_position(spec: &Spec) -> (i32, i32, u32, u32) {
    let m = &spec.mon;
    let k = m.scale;
    let (hw, hh) = ((HUD_W * k).round(), (HUD_H * k).round());
    let (gap, edge) = (8.0 * k, 4.0 * k);
    let (bx, by, bw, bh) = (m.x as f64, m.y as f64, m.w as f64, m.h as f64);
    let (rx, ry, rw, rh) = (bx + spec.x as f64, by + spec.y as f64, spec.w as f64, spec.h as f64);
    let mut y = ry + rh + gap;
    if y + hh > by + bh - edge {
        y = ry - hh - gap;
    }
    if y < by + edge {
        y = ry + rh - hh - gap; // region covers the whole screen
    }
    let y = y.max(by + edge).min(by + bh - hh - edge);
    let x = (rx + rw - hw).max(bx + edge).min(bx + bw - hw - edge);
    (x.round() as i32, y.round() as i32, hw as u32, hh as u32)
}

fn create_hud(spec: &Spec) -> Option<WebviewWindow> {
    let win = WebviewWindowBuilder::new(crate::app(), HUD, WebviewUrl::App("rec.html".into()))
        .title("StopPic")
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .shadow(false)
        .visible(false)
        .focused(false)
        .disable_drag_drop_handler()
        .build()
        .map_err(|e| eprintln!("[record] панель: {e}"))
        .ok()?;
    let (x, y, w, h) = hud_position(spec);
    for _ in 0..2 {
        let _ = win.set_position(PhysicalPosition::new(x, y));
        let _ = win.set_size(PhysicalSize::new(w, h));
    }
    #[cfg(windows)]
    crate::winapi::exclude_from_capture(&win); // the panel does not appear in the recording
    let _ = win.show();
    Some(win)
}

/// Frame just outside the region (or along its edge if the region is full screen).
/// Clicks pass through it, and it does not appear in the recording.
fn create_frame(spec: &Spec) {
    let m = &spec.mon;
    let pad = (4.0 * m.scale).round() as i32;
    let left = (m.x + spec.x as i32 - pad).max(m.x);
    let top = (m.y + spec.y as i32 - pad).max(m.y);
    let right = (m.x + (spec.x + spec.w) as i32 + pad).min(m.x + m.w as i32);
    let bottom = (m.y + (spec.y + spec.h) as i32 + pad).min(m.y + m.h as i32);
    let built = WebviewWindowBuilder::new(crate::app(), FRAME, WebviewUrl::App("frame.html".into()))
        .title("StopPic")
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .shadow(false)
        .visible(false)
        .focused(false)
        .disable_drag_drop_handler()
        .build();
    let win = match built {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[record] рамка: {e}"); // recording works without the frame anyway
            return;
        }
    };
    for _ in 0..2 {
        let _ = win.set_position(PhysicalPosition::new(left, top));
        let _ = win.set_size(PhysicalSize::new((right - left).max(1) as u32, (bottom - top).max(1) as u32));
    }
    let _ = win.set_ignore_cursor_events(true);
    #[cfg(windows)]
    crate::winapi::exclude_from_capture(&win);
    let _ = win.show();
}

fn set_frame_paused(paused: bool) {
    let _ = crate::app().emit_to(EventTarget::webview_window(FRAME), "rec:paused", paused);
}

fn close_hud() {
    let app = crate::app();
    for label in [HUD, FRAME] {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.destroy();
        }
    }
}

// ───────────────────────── Start ─────────────────────────

/// Region selected in the overlay (monitor CSS pixels).
pub fn begin(mon: Monitor, rect: Rect, mic: bool, system_audio: bool) {
    if phase() != Phase::Idle || rect.width < 16.0 || rect.height < 16.0 {
        return;
    }
    // The overlay choice becomes the default for next time
    settings::update(|s| {
        s.rec_mic = mic;
        s.rec_system_audio = system_audio;
    });
    if cfg!(not(windows)) {
        notify::notify(&t("recUnsupported"), None);
        return;
    }
    let st = settings::get();
    let gif = st.rec_format == "gif";
    let k = mon.scale;
    let x = ((rect.x * k).round() as u32).min(mon.w.saturating_sub(2));
    let y = ((rect.y * k).round() as u32).min(mon.h.saturating_sub(2));
    let w = (((rect.width * k).round() as u32).min(mon.w - x).max(2)) & !1;
    let h = (((rect.height * k).round() as u32).min(mon.h - y).max(2)) & !1;
    let file = std::env::temp_dir().join(format!("stoppic-{}.part", chrono::Local::now().timestamp_millis()));
    let spec = Spec {
        mon,
        x,
        y,
        w,
        h,
        fps: st.rec_fps,
        gif,
        mic: mic && !gif,
        system_audio: system_audio && !gif,
        mic_device: st.rec_mic_device,
        sys_device: st.rec_sys_device,
        file,
    };

    // Frame before the panel: the panel must end up on top of it
    create_frame(&spec);
    let hud_ok = create_hud(&spec).is_some();

    // Audio and the encoder are prepared during the countdown: afterwards only the frames need starting
    let (tx, rx) = mpsc::channel::<Prepared>();
    let prep_spec = spec.clone();
    thread::spawn(move || {
        let res = engine::Engine::prepare(&prep_spec);
        // Recording was cancelled while preparing: nobody needs the engine, stop it
        if let Err(mpsc::SendError(Ok((e, _)))) = tx.send(res) {
            e.cancel();
        }
    });
    ST.lock().unwrap().job = Some(Job { spec, engine: None, prep: Some(rx) });
    if !hud_ok {
        abort_with("recFailed");
        return;
    }
    set_phase(Phase::Countdown);
}

// ───────────────────────── Finishing ─────────────────────────

/// Finish without saving: cancel or error. The temp file is deleted.
fn discard() {
    let job = ST.lock().unwrap().job.take();
    if let Some(job) = job {
        if let Some(e) = job.engine {
            e.cancel();
        }
        // Preparation may still be running: wait for it in the background, stop the engine and delete the file
        let file = job.spec.file.clone();
        let prep = job.prep;
        thread::spawn(move || {
            if let Some(Ok((e, _))) = prep.and_then(|rx| rx.recv_timeout(Duration::from_secs(20)).ok()) {
                e.cancel();
            }
            let _ = std::fs::remove_file(&file);
        });
    }
    close_hud();
    set_phase(Phase::Idle);
}

fn abort_with(key: &str) {
    notify::notify(&t(key), None);
    discard();
}

fn stop() {
    let job = {
        let mut s = ST.lock().unwrap();
        if !matches!(s.phase, Phase::Recording | Phase::Paused) {
            return;
        }
        s.job.take()
    };
    let Some(mut job) = job else { return };
    set_phase(Phase::Processing);
    thread::spawn(move || {
        let progress = |p: u32| {
            let _ = crate::app().emit_to(EventTarget::webview_window(HUD), "rec:progress", p);
        };
        let res = match job.engine.take() {
            Some(e) => e.finish(progress),
            None => Err("нет движка".into()),
        };
        finalize(job.spec, res);
        close_hud();
        set_phase(Phase::Idle);
    });
}

/// The recording is ready in the target format; all that remains is to put it in the screenshots folder.
fn finalize(spec: Spec, res: Result<(), String>) {
    let size = std::fs::metadata(&spec.file).map(|m| m.len()).unwrap_or(0);
    if let Err(e) = &res {
        eprintln!("[record] ошибка записи: {e}");
    }
    if res.is_err() || size == 0 {
        let _ = std::fs::remove_file(&spec.file);
        notify::notify(&t("recFailed"), None);
        return;
    }
    let dir = PathBuf::from(settings::get().save_dir);
    let ext = if spec.gif { "gif" } else { "mp4" };
    let placed = files::create_unique(&dir, &format!("Recording {}", files::timestamp()), ext).and_then(|(f, out)| {
        drop(f);
        files::move_file(&spec.file, &out).map(|_| out)
    });
    match placed {
        Ok(out) => notify::notify(&t("recSaved"), Some(files::reveal(out))),
        Err(e) => {
            // Keep the temp file: the recording can be recovered from it manually
            eprintln!("[record] не удалось сохранить запись: {e}");
            let file = spec.file.to_string_lossy().into_owned();
            notify::notify(&t_with("recFailedKept", &[("file", &file)]), Some(files::reveal(spec.file.clone())));
        }
    }
}

pub fn shutdown() {
    // An unfinished recording stays in the temp folder
    if let Some(job) = ST.lock().unwrap().job.take() {
        if let Some(e) = job.engine {
            e.cancel();
        }
        if let Some(Ok((e, _))) = job.prep.and_then(|rx| rx.try_recv().ok()) {
            e.cancel();
        }
    }
}

// ───────────────────────── Panel commands ─────────────────────────

#[derive(Serialize)]
pub struct RecConfig {
    format: &'static str,
    countdown: bool,
    strings: HashMap<String, String>,
}

fn from_hud(w: &WebviewWindow) -> bool {
    w.label() == HUD
}

#[tauri::command]
pub async fn rec_get_config(window: WebviewWindow) -> Option<RecConfig> {
    if !from_hud(&window) {
        return None;
    }
    let s = ST.lock().unwrap();
    let job = s.job.as_ref()?;
    Some(RecConfig {
        format: if job.spec.gif { "gif" } else { "mp4" },
        countdown: settings::get().rec_countdown,
        strings: i18n::strings(),
    })
}

/// Countdown finished: start the capture. false: it failed (the notification has already been shown).
#[tauri::command]
pub async fn rec_start(window: WebviewWindow) -> bool {
    if !from_hud(&window) || phase() != Phase::Countdown {
        return false;
    }
    let Some(rx) = ST.lock().unwrap().job.as_mut().and_then(|j| j.prep.take()) else { return false };
    // Wait for preparation without holding the state lock: the hotkey and tray keep working meanwhile.
    // Usually it is already done: the countdown lasts 3 seconds
    let res = tauri::async_runtime::spawn_blocking(move || {
        let t0 = std::time::Instant::now();
        let prepared = rx.recv_timeout(Duration::from_secs(15)).unwrap_or_else(|_| Err("подготовка не уложилась в 15 с".into()));
        dlog!("запись: ждали подготовку {:?}", t0.elapsed());
        prepared.and_then(|(mut e, warnings)| match e.go() {
            Ok(()) => Ok((e, warnings)),
            Err(err) => {
                e.cancel();
                Err(err)
            }
        })
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));

    match res {
        Ok((engine, warnings)) => {
            let rejected = {
                let mut s = ST.lock().unwrap();
                let counting = s.phase == Phase::Countdown;
                match s.job.as_mut() {
                    Some(job) if counting => {
                        job.engine = Some(engine);
                        None
                    }
                    _ => Some(engine),
                }
            };
            if let Some(engine) = rejected {
                engine.cancel(); // cancelled while starting
                return false;
            }
            set_phase(Phase::Recording);
            // Warnings that do not stop the recording (e.g. no microphone)
            for w in warnings {
                notify::notify(&t(w), None);
            }
            true
        }
        Err(e) => {
            eprintln!("[record] не удалось начать: {e}");
            abort_with(if e == "unsupported" { "recUnsupported" } else { "recFailed" });
            false
        }
    }
}

#[tauri::command]
pub async fn rec_command(window: WebviewWindow, cmd: String) {
    if !from_hud(&window) {
        return;
    }
    match cmd.as_str() {
        "pause" | "resume" => {
            let mut s = ST.lock().unwrap();
            let pause = cmd == "pause";
            let ok = (pause && s.phase == Phase::Recording) || (!pause && s.phase == Phase::Paused);
            if !ok {
                return;
            }
            if let Some(e) = s.job.as_ref().and_then(|j| j.engine.as_ref()) {
                if pause {
                    e.pause();
                } else {
                    e.resume();
                }
            }
            s.phase = if pause { Phase::Paused } else { Phase::Recording };
            drop(s);
            set_frame_paused(pause);
        }
        "stop" => stop(),
        "cancel" => discard(),
        _ => {}
    }
}
