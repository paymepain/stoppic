// Запись на Windows без внешних программ:
// картинка: Windows Graphics Capture, видео: H.264 + AAC встроенным кодировщиком Media Foundation,
// звук: WASAPI (системный звук через loopback и микрофон), GIF: крейт image.

use super::Spec;
use image::{Delay, Frame as GifFrame, RgbaImage};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufWriter;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{
    AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder, VideoSettingsSubType,
};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor as WgcMonitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings,
    SecondaryWindowSettings, Settings,
};

const GIF_MAX_FPS: u32 = 15;
const GIF_MAX_WIDTH: u32 = 960;
const GIF_MAX_SECONDS: u32 = 120;
const SAMPLE_RATE: u32 = 48_000;

type Error = Box<dyn std::error::Error + Send + Sync>;

/// Общее для потока захвата, звука и управления
#[derive(Default)]
struct Shared {
    paused: AtomicBool,
    /// Стоп видео
    stopping: AtomicBool,
    /// Первый кадр видео ушёл в кодировщик: от этого момента идут часы звука
    video_started: AtomicBool,
    /// Стоп устройств звука (после дочитывания последних пакетов)
    audio_stop: AtomicBool,
    /// Стоп смесителя (после того как устройства остановлены)
    mixer_stop: AtomicBool,
}

// ───────────────────────── Звук ─────────────────────────

/// Очередь сэмплов одного источника: f32, стерео, 48 кГц
type Queue = Arc<Mutex<VecDeque<f32>>>;

/// Сколько звука может ждать в очереди сверх нужного. Больше: срезаем старое, иначе звук начнёт отставать
const MAX_BACKLOG: usize = (SAMPLE_RATE as usize / 10) * 2; // 100 мс, стерео

/// Поток захвата WASAPI. loopback: звук системы, иначе микрофон.
/// Возвращается, когда устройство открыто (или не открылось).
fn start_audio_source(loopback: bool, shared: Arc<Shared>) -> Option<(Queue, JoinHandle<()>)> {
    let queue: Queue = Arc::new(Mutex::new(VecDeque::new()));
    let (ready_tx, ready_rx) = mpsc::channel::<bool>();
    let q = queue.clone();
    let handle = thread::spawn(move || {
        let run = || -> Result<(), Error> {
            let _ = wasapi::initialize_mta();
            let enumerator = wasapi::DeviceEnumerator::new()?;
            let dir = if loopback { wasapi::Direction::Render } else { wasapi::Direction::Capture };
            let device = enumerator.get_default_device(&dir)?;
            let mut client = device.get_iaudioclient()?;
            let format = wasapi::WaveFormat::new(32, 32, &wasapi::SampleType::Float, SAMPLE_RATE as usize, 2, None);
            // Опрос, а не события: loopback без звука события не присылает
            let mode = wasapi::StreamMode::PollingShared { autoconvert: true, buffer_duration_hns: 2_000_000 };
            client.initialize_client(&format, &wasapi::Direction::Capture, &mode)?;
            let capture = client.get_audiocaptureclient()?;
            client.start_stream()?;
            let _ = ready_tx.send(true);

            let mut bytes: VecDeque<u8> = VecDeque::new();
            let read = |bytes: &mut VecDeque<u8>| -> Result<(), Error> {
                while capture.get_next_packet_size()?.unwrap_or(0) > 0 {
                    capture.read_from_device_to_deque(bytes)?;
                }
                let n = bytes.len() / 4;
                let mut q = q.lock().unwrap();
                for _ in 0..n {
                    let b = [bytes.pop_front().unwrap(), bytes.pop_front().unwrap(), bytes.pop_front().unwrap(), bytes.pop_front().unwrap()];
                    q.push_back(f32::from_le_bytes(b));
                }
                // До старта видео звук не нужен: иначе он накопится и весь сдвинется
                if !shared.video_started.load(Ordering::Relaxed) {
                    q.clear();
                }
                Ok(())
            };
            while !shared.audio_stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(10));
                read(&mut bytes)?;
            }
            read(&mut bytes)?; // последние пакеты, иначе обрежется конец фразы
            client.stop_stream()?;
            Ok(())
        };
        if let Err(e) = run() {
            eprintln!("[record] звук ({}): {e}", if loopback { "система" } else { "микрофон" });
        }
        let _ = ready_tx.send(false);
    });
    match ready_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(true) => Some((queue, handle)),
        _ => None,
    }
}

/// Смеситель: по часам отдаёт кодировщику ровно столько звука, сколько прошло времени.
/// Тишину (нет данных от loopback) заполняет нулями, чтобы звук не отставал от видео.
fn start_mixer(sources: Vec<Queue>, encoder: Arc<Mutex<Option<VideoEncoder>>>, shared: Arc<Shared>) -> JoinHandle<()> {
    thread::spawn(move || {
        let send = |mix: &[f32], pcm: &mut Vec<u8>| -> bool {
            pcm.clear();
            for v in mix {
                pcm.extend_from_slice(&((v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
            }
            match encoder.lock().unwrap().as_mut() {
                Some(enc) => enc.send_audio_buffer(pcm, 0).map_err(|e| eprintln!("[record] кодирование звука: {e}")).is_ok(),
                None => false,
            }
        };
        let take = |frames: usize| -> Vec<f32> {
            let mut mix = vec![0f32; frames * 2];
            for q in &sources {
                let mut q = q.lock().unwrap();
                let n = (frames * 2).min(q.len());
                for (v, s) in mix.iter_mut().zip(q.drain(..n)) {
                    *v += s;
                }
            }
            mix
        };

        // Ждём первый кадр видео: звук и картинка начинаются в один момент
        let wait_from = Instant::now();
        while !shared.video_started.load(Ordering::Relaxed) && !shared.mixer_stop.load(Ordering::Relaxed) {
            if wait_from.elapsed() > Duration::from_secs(3) {
                shared.video_started.store(true, Ordering::Relaxed); // кадров нет, но звук писать надо
            }
            thread::sleep(Duration::from_millis(2));
        }
        for q in &sources {
            q.lock().unwrap().clear();
        }

        let mut active = Duration::ZERO; // время записи без пауз
        let mut last = Instant::now();
        let mut sent: u64 = 0; // кадров (пар сэмплов) отдано
        let mut pcm: Vec<u8> = Vec::new();
        while !shared.mixer_stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(20));
            let now = Instant::now();
            if shared.paused.load(Ordering::Relaxed) {
                last = now;
                for q in &sources {
                    q.lock().unwrap().clear(); // звук на паузе не нужен
                }
                continue;
            }
            // После остановки видео время больше не идёт: в конце отдадим только остаток очередей
            if !shared.stopping.load(Ordering::Relaxed) {
                active += now - last;
            }
            last = now;
            let target = (active.as_secs_f64() * SAMPLE_RATE as f64) as u64;
            let frames = target.saturating_sub(sent) as usize;
            if frames == 0 {
                continue;
            }
            // Очередь не должна расти: лишнее (самое старое) срезаем, чтобы звук не уезжал от видео
            for q in &sources {
                let mut q = q.lock().unwrap();
                let limit = frames * 2 + MAX_BACKLOG;
                if q.len() > limit {
                    let extra = q.len() - limit;
                    q.drain(..extra);
                }
            }
            if !send(&take(frames), &mut pcm) {
                return;
            }
            sent += frames as u64;
        }
        // Финал: всё, что устройства успели дописать до остановки
        let rest = sources.iter().map(|q| q.lock().unwrap().len() / 2).max().unwrap_or(0);
        if rest > 0 {
            send(&take(rest), &mut pcm);
        }
    })
}

// ───────────────────────── Видео ─────────────────────────

enum Sink {
    Mp4 { encoder: Arc<Mutex<Option<VideoEncoder>>> },
    Gif { tx: Option<SyncSender<(RgbaImage, i64)>>, gw: u32, gh: u32, frames: u32, max_frames: u32 },
}

struct Flags {
    spec: SpecCopy,
    shared: Arc<Shared>,
    sink: Arc<Mutex<Sink>>,
    failed: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
struct SpecCopy {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    fps: u32,
}

struct Handler {
    flags: Flags,
    /// Время кадров в единицах 100 нс
    interval: i64,
    last_raw: Option<i64>,
    last_sent: Option<i64>,
    paused_total: i64,
    was_paused: bool,
    flipped: Vec<u8>,
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Flags;
    type Error = Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let fps = ctx.flags.spec.fps.max(1) as i64;
        Ok(Self {
            interval: 10_000_000 / fps,
            flags: ctx.flags,
            last_raw: None,
            last_sent: None,
            paused_total: 0,
            was_paused: false,
            flipped: Vec::new(),
        })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, control: InternalCaptureControl) -> Result<(), Self::Error> {
        let shared = &self.flags.shared;
        if shared.stopping.load(Ordering::Relaxed) {
            control.stop();
            return Ok(());
        }
        if shared.paused.load(Ordering::Relaxed) {
            self.was_paused = true;
            return Ok(());
        }
        let raw = frame.timestamp()?.Duration;
        // Время паузы вырезаем из ролика
        if self.was_paused {
            if let Some(last) = self.last_raw {
                self.paused_total += (raw - last - self.interval).max(0);
            }
            self.was_paused = false;
        }
        self.last_raw = Some(raw);
        let ts = raw - self.paused_total;
        // Не чаще заданной частоты кадров
        if let Some(prev) = self.last_sent {
            if ts - prev < self.interval * 9 / 10 {
                return Ok(());
            }
        }
        self.last_sent = Some(ts);

        let s = self.flags.spec;
        let full = s.x == 0 && s.y == 0 && s.w == frame.width() && s.h == frame.height();
        let mut sink = self.flags.sink.lock().unwrap();
        match &mut *sink {
            Sink::Mp4 { encoder } => {
                let mut guard = encoder.lock().unwrap();
                let Some(enc) = guard.as_mut() else { return Ok(()) };
                let res = if full {
                    // Весь монитор: кадр уходит в кодировщик прямо на видеокарте
                    enc.send_frame(frame)
                } else {
                    let mut buf = frame.buffer_crop(s.x, s.y, s.x + s.w, s.y + s.h)?;
                    let pitch = buf.row_pitch() as usize;
                    let row = s.w as usize * 4;
                    let raw_buf = buf.as_raw_buffer();
                    // Для буфера кодировщик ждёт строки снизу вверх
                    self.flipped.resize(row * s.h as usize, 0);
                    for y in 0..s.h as usize {
                        let src = &raw_buf[y * pitch..y * pitch + row];
                        let dst_y = s.h as usize - 1 - y;
                        self.flipped[dst_y * row..dst_y * row + row].copy_from_slice(src);
                    }
                    enc.send_frame_buffer(&self.flipped, ts)
                };
                if res.is_ok() {
                    shared.video_started.store(true, Ordering::Relaxed); // с этого кадра идут часы звука
                }
                if let Err(e) = res {
                    eprintln!("[record] кодирование видео: {e}");
                    self.flags.failed.store(true, Ordering::Relaxed);
                    control.stop();
                }
            }
            Sink::Gif { tx, gw, gh, frames, max_frames } => {
                if *frames >= *max_frames {
                    // Достигли предела длины: панель покажет обработку и вызовет стоп (один раз)
                    if *frames == *max_frames {
                        *frames += 1;
                        super::send_cmd("stop");
                    }
                    return Ok(());
                }
                let buf = frame.buffer_crop(s.x, s.y, s.x + s.w, s.y + s.h)?;
                let mut plain = Vec::new();
                let bgra = buf.as_nopadding_buffer(&mut plain);
                let mut rgba = bgra.to_vec();
                for px in rgba.chunks_exact_mut(4) {
                    px.swap(0, 2);
                    px[3] = 255;
                }
                let Some(img) = RgbaImage::from_raw(s.w, s.h, rgba) else { return Ok(()) };
                let small = if *gw != s.w {
                    image::imageops::resize(&img, *gw, *gh, image::imageops::FilterType::Triangle)
                } else {
                    img
                };
                if let Some(tx) = tx {
                    // Если сборщик GIF не успевает, лишний кадр пропускаем, а не копим в памяти
                    if tx.try_send((small, ts / 10_000)).is_ok() {
                        *frames += 1;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Поток сборки GIF: кадры приходят по одному, задержку кадра узнаём по времени следующего.
fn start_gif_writer(rx: Receiver<(RgbaImage, i64)>, file: File, done: Arc<AtomicU32>, total: Arc<AtomicU32>) -> JoinHandle<Result<(), String>> {
    thread::spawn(move || {
        let mut enc = image::codecs::gif::GifEncoder::new_with_speed(BufWriter::new(file), 10);
        enc.set_repeat(image::codecs::gif::Repeat::Infinite).map_err(|e| e.to_string())?;
        let mut pending: Option<(RgbaImage, i64)> = None;
        let mut start: Option<i64> = None;
        let mut written_cs: i64 = 0; // уже записанная длительность в сотых секунды
        let mut write = |img: RgbaImage, t: i64, next: i64, enc: &mut image::codecs::gif::GifEncoder<BufWriter<File>>| {
            // Округляем границы кадров, а не длительности: иначе ошибка копится и ролик «плывёт»
            let t0 = *start.get_or_insert(t);
            let end_cs = ((next - t0) as f64 / 10.0).round() as i64;
            let delay_cs = (end_cs - written_cs).max(2); // меньше 2 сотых многие плееры замедляют
            written_cs += delay_cs;
            let frame = GifFrame::from_parts(img, 0, 0, Delay::from_numer_denom_ms((delay_cs * 10) as u32, 1));
            enc.encode_frame(frame).map_err(|e| e.to_string())?;
            done.fetch_add(1, Ordering::Relaxed);
            Ok::<(), String>(())
        };
        for (img, t) in rx.iter() {
            total.fetch_add(1, Ordering::Relaxed);
            if let Some((prev, pt)) = pending.replace((img, t)) {
                write(prev, pt, t, &mut enc)?;
            }
        }
        if let Some((img, t)) = pending {
            write(img, t, t + 1000 / GIF_MAX_FPS as i64, &mut enc)?;
        }
        Ok(())
    })
}

// ───────────────────────── Движок ─────────────────────────

pub struct Engine {
    shared: Arc<Shared>,
    control: Option<CaptureControl<Handler, Error>>,
    sink: Arc<Mutex<Sink>>,
    failed: Arc<AtomicBool>,
    audio: Vec<JoinHandle<()>>,
    mixer: Option<JoinHandle<()>>,
    gif: Option<(JoinHandle<Result<(), String>>, Arc<AtomicU32>, Arc<AtomicU32>)>,
    /// Что захватывать, когда придёт время go(): HMONITOR и область
    pending: Option<(usize, SpecCopy)>,
}

impl Engine {
    /// Подготовка во время отсчёта: звук, кодировщик, сборщик GIF. Кадры ещё не идут.
    /// Второе значение: предупреждения (ключи строк), запись при этом возможна.
    pub fn prepare(spec: &Spec) -> Result<(Self, Vec<&'static str>), String> {
        let t0 = Instant::now();
        let shared = Arc::new(Shared::default());
        let mut warnings = Vec::new();
        let failed = Arc::new(AtomicBool::new(false));

        let center = (spec.mon.x + spec.mon.w as i32 / 2, spec.mon.y + spec.mon.h as i32 / 2);
        let hmonitor = crate::winapi::monitor_at(center.0, center.1) as usize;

        let mut audio = Vec::new();
        let mut mixer = None;
        let mut gif = None;
        let sink = if spec.gif {
            let fps = spec.fps.min(GIF_MAX_FPS);
            let gw = spec.w.min(GIF_MAX_WIDTH);
            let gh = ((spec.h as u64 * gw as u64) / spec.w as u64).max(1) as u32;
            let file = File::create(&spec.file).map_err(|e| e.to_string())?;
            let (tx, rx) = mpsc::sync_channel(fps as usize * 4);
            let (done, total) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
            gif = Some((start_gif_writer(rx, file, done.clone(), total.clone()), done, total));
            Sink::Gif { tx: Some(tx), gw, gh, frames: 0, max_frames: fps * GIF_MAX_SECONDS }
        } else {
            // Сначала открываем звук (оба устройства параллельно): от этого зависит, будет ли в MP4 звуковая дорожка
            let wanted: Vec<(bool, &'static str)> =
                [(spec.system_audio, true, "recNoSysAudio"), (spec.mic, false, "recNoMic")].into_iter().filter(|w| w.0).map(|w| (w.1, w.2)).collect();
            let opened: Vec<_> = thread::scope(|sc| {
                let jobs: Vec<_> = wanted.iter().map(|(loopback, _)| {
                    let sh = shared.clone();
                    let lb = *loopback;
                    sc.spawn(move || start_audio_source(lb, sh))
                }).collect();
                jobs.into_iter().map(|j| j.join().ok().flatten()).collect()
            });
            let mut sources = Vec::new();
            for ((_, warn), res) in wanted.iter().zip(opened) {
                match res {
                    Some((q, h)) => {
                        sources.push(q);
                        audio.push(h);
                    }
                    None => warnings.push(*warn),
                }
            }
            dlog!("запись: звук открыт за {:?} ({} источн.)", t0.elapsed(), sources.len());
            let bitrate = (spec.w as u64 * spec.h as u64 * spec.fps as u64 * 12 / 100).clamp(3_000_000, 25_000_000) as u32;
            let video = VideoSettingsBuilder::new(spec.w, spec.h).sub_type(VideoSettingsSubType::H264).bitrate(bitrate).frame_rate(spec.fps);
            let audio_settings = AudioSettingsBuilder::new().sample_rate(SAMPLE_RATE).channel_count(2).bit_per_sample(16).bitrate(160_000).disabled(sources.is_empty());
            let encoder = match VideoEncoder::new(video, audio_settings, ContainerSettingsBuilder::new(), &spec.file) {
                Ok(e) => e,
                Err(e) => {
                    let mut engine = Self { shared, control: None, sink: Arc::new(Mutex::new(Sink::Gif { tx: None, gw: 0, gh: 0, frames: 0, max_frames: 0 })), failed, audio, mixer: None, gif: None, pending: None };
                    engine.halt();
                    return Err(format!("кодировщик: {e}"));
                }
            };
            dlog!("запись: кодировщик готов за {:?}", t0.elapsed());
            let encoder = Arc::new(Mutex::new(Some(encoder)));
            if !sources.is_empty() {
                mixer = Some(start_mixer(sources, encoder.clone(), shared.clone()));
            }
            Sink::Mp4 { encoder }
        };
        let fps = if spec.gif { spec.fps.min(GIF_MAX_FPS) } else { spec.fps };
        let pending = Some((hmonitor, SpecCopy { x: spec.x, y: spec.y, w: spec.w, h: spec.h, fps }));
        Ok((Self { shared, control: None, sink: Arc::new(Mutex::new(sink)), failed, audio, mixer, gif, pending }, warnings))
    }

    /// Отсчёт закончился: пошли кадры.
    pub fn go(&mut self) -> Result<(), String> {
        let t0 = Instant::now();
        let Some((hmonitor, spec)) = self.pending.take() else { return Err("уже запущено".into()) };
        let flags = Flags { spec, shared: self.shared.clone(), sink: self.sink.clone(), failed: self.failed.clone() };
        let settings = Settings::new(
            WgcMonitor::from_raw_hmonitor(hmonitor as *mut std::ffi::c_void),
            CursorCaptureSettings::WithCursor,
            DrawBorderSettings::WithoutBorder, // без жёлтой рамки вокруг экрана
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            flags,
        );
        let control = Handler::start_free_threaded(settings).map_err(|e| format!("захват экрана: {e}"))?;
        self.control = Some(control);
        dlog!("запись: захват запущен за {:?}", t0.elapsed());
        Ok(())
    }

    pub fn pause(&self) {
        self.shared.paused.store(true, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.shared.paused.store(false, Ordering::Relaxed);
    }

    /// Останавливает захват и дописывает файл. progress: проценты сборки GIF.
    pub fn finish(mut self, progress: impl Fn(u32)) -> Result<(), String> {
        self.halt();
        let failed = self.failed.load(Ordering::Relaxed);
        let mut sink = self.sink.lock().unwrap();
        match &mut *sink {
            Sink::Mp4 { encoder } => {
                let enc = encoder.lock().unwrap().take();
                if let Some(enc) = enc {
                    enc.finish().map_err(|e| e.to_string())?;
                }
            }
            Sink::Gif { tx, .. } => {
                drop(tx.take()); // сборщик допишет оставшиеся кадры и завершится
                drop(sink);
                if let Some((handle, done, total)) = self.gif.take() {
                    while !handle.is_finished() {
                        let t = total.load(Ordering::Relaxed).max(1);
                        progress((done.load(Ordering::Relaxed) * 100 / t).min(99));
                        thread::sleep(Duration::from_millis(150));
                    }
                    handle.join().map_err(|_| "сборка GIF упала".to_string())??;
                }
                if failed {
                    return Err("ошибка захвата".into());
                }
                return Ok(());
            }
        }
        if failed {
            return Err("ошибка кодирования".into());
        }
        Ok(())
    }

    /// Остановить всё, файл не нужен.
    pub fn cancel(mut self) {
        self.halt();
        let mut sink = self.sink.lock().unwrap();
        match &mut *sink {
            Sink::Mp4 { encoder } => {
                if let Some(enc) = encoder.lock().unwrap().take() {
                    let _ = enc.finish();
                }
            }
            Sink::Gif { tx, .. } => drop(tx.take()),
        }
        drop(sink);
        if let Some((h, _, _)) = self.gif.take() {
            let _ = h.join();
        }
    }

    /// Порядок важен: видео, затем дочитываем звук с устройств, затем смеситель отдаёт остаток
    fn halt(&mut self) {
        self.shared.stopping.store(true, Ordering::Relaxed);
        if let Some(c) = self.control.take() {
            let _ = c.stop();
        }
        if !self.audio.is_empty() {
            thread::sleep(Duration::from_millis(80)); // последние пакеты звука ещё в пути
        }
        self.shared.audio_stop.store(true, Ordering::Relaxed);
        for h in self.audio.drain(..) {
            let _ = h.join();
        }
        self.shared.mixer_stop.store(true, Ordering::Relaxed);
        if let Some(m) = self.mixer.take() {
            let _ = m.join();
        }
    }
}
