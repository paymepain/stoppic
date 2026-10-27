// Мост к Rust-части. Повторяет прежний window.api из Electron-preload,
// поэтому код окон почти не изменился.
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import type {
  CropData,
  DirResult,
  HotkeyName,
  HotkeyResult,
  OverlayAction,
  OverlayInit,
  RecCommand,
  RecConfig,
  Rect,
  SettingsPatch,
  SettingsState
} from './shared/ipc'

const win = getCurrentWebviewWindow()
/** Замеры задержки в dev-режиме (в консоль Rust) */
export const devLog = (msg: string): void => {
  if (import.meta.env.DEV) void invoke('dev_log', { msg })
}
const fire = (cmd: string, args?: Record<string, unknown>): void => void invoke(cmd, args).catch(console.error)

/** Пустой ответ означает «нет данных» (не картинка, отмена диалога и т.п.) */
const bytesOrNull = (buf: ArrayBuffer): Uint8Array | null => (buf.byteLength > 0 ? new Uint8Array(buf) : null)

// Подписку на кадр ставим сразу при загрузке, чтобы ready() не обогнал её
let initCb: ((data: OverlayInit) => void) | null = null
let revealCb: (() => void) | null = null
/** Кадр: 8 байт (ширина, высота: u32 LE), дальше RGBA */
async function toBitmap(buf: ArrayBuffer): Promise<ImageBitmap> {
  const head = new DataView(buf, 0, 8)
  const w = head.getUint32(0, true)
  const h = head.getUint32(4, true)
  return createImageBitmap(new ImageData(new Uint8ClampedArray(buf, 8, w * h * 4), w, h))
}

// Быстрый путь Windows: кадр приходит в общей памяти WebView2, без копирования через IPC
interface SharedBufferEvent extends Event {
  getBuffer(): ArrayBuffer
  additionalData: Omit<OverlayInit, 'frame'>
}
interface WebView2 {
  addEventListener(type: 'sharedbufferreceived', cb: (e: SharedBufferEvent) => void): void
  releaseBuffer(buf: ArrayBuffer): void
}
const webview2 = (window as unknown as { chrome?: { webview?: WebView2 } }).chrome?.webview
webview2?.addEventListener('sharedbufferreceived', (e) => {
  const buf = e.getBuffer()
  const cb = initCb
  initCb = null
  devLog('кадр в общей памяти')
  void toBitmap(buf)
    .then((frame) => {
      devLog('ImageBitmap готов')
      cb?.({ ...e.additionalData, frame })
    })
    .finally(() => webview2.releaseBuffer(buf))
})

const listening = Promise.all([
  win.listen<Omit<OverlayInit, 'frame'>>('overlay:init', async (e) => {
    const cb = initCb
    initCb = null
    if (!cb) return
    devLog('init получен')
    // Сырые пиксели без сжатия: по IPC Tauri это быстрее, чем кодировать JPEG.
    // Первые 8 байт: ширина и высота (u32 LE), дальше RGBA
    const buf = await invoke<ArrayBuffer>('overlay_frame')
    devLog(`кадр получен по IPC ${(buf.byteLength / 1048576).toFixed(1)} МБ`)
    const frame = await toBitmap(buf)
    devLog('ImageBitmap готов')
    cb({ ...e.payload, frame })
  }),
  win.listen('overlay:reveal', () => {
    const cb = revealCb
    revealCb = null
    cb?.()
  })
])

const api = {
  // ───── Оверлей снимка / выбор области записи ─────
  /** Страница загружена и готова принять кадр (оверлей создаётся заранее и ждёт) */
  ready: (): void => void listening.then(() => invoke('overlay_ready')),
  onInit: (cb: (data: OverlayInit) => void): void => {
    initCb = cb
  },
  shown: (): void => fire('overlay_shown'),
  /** Окно показано прозрачным: ждём, когда в нём будет отрисован кадр */
  onReveal: (cb: () => void): void => {
    revealCb = cb
  },
  revealed: (): void => fire('overlay_revealed'),
  cancel: (): void => fire('overlay_cancel'),
  /** Быстрый путь: снимок без аннотаций, кроп делает Rust */
  action: (type: OverlayAction, rect: Rect): void => fire('overlay_action', { kind: type, rect }),
  /** Путь с аннотациями: забрать lossless-кроп, собрать итог и отправить обратно */
  getCrop: async (rect: Rect): Promise<CropData | null> => {
    const buf = await invoke<ArrayBuffer>('overlay_get_crop', { rect })
    if (buf.byteLength <= 32) return null
    // Первые 32 байта: left, top, kx, ky (f64, little-endian), дальше PNG
    const head = new DataView(buf, 0, 32)
    return {
      left: head.getFloat64(0, true),
      top: head.getFloat64(8, true),
      kx: head.getFloat64(16, true),
      ky: head.getFloat64(24, true),
      png: new Uint8Array(buf, 32)
    }
  },
  submit: (type: OverlayAction, rect: Rect, png: Uint8Array): void =>
    void invoke('overlay_submit', png, { headers: { 'x-kind': type, 'x-rect': JSON.stringify(rect) } }).catch(console.error),
  /** Пока редактируется текст, Esc не должен закрывать весь оверлей */
  setEscape: (enabled: boolean): void => fire('overlay_escape', { enabled }),
  /** Картинка из буфера обмена (PNG) или null, если там не картинка */
  readClipboardImage: async (): Promise<Uint8Array | null> => bytesOrNull(await invoke('overlay_clipboard_image')),
  /** Диалог выбора файла с картинкой */
  pickImageFile: async (): Promise<Uint8Array | null> => bytesOrNull(await invoke('overlay_pick_image')),
  /** Режим записи: область выбрана, начинаем */
  recordStart: (rect: Rect, opts: { mic: boolean; systemAudio: boolean }): void =>
    fire('overlay_record', { rect, mic: opts.mic, systemAudio: opts.systemAudio }),

  // ───── Закреплённый снимок ─────
  pinGetInit: async (): Promise<Uint8Array | null> => bytesOrNull(await invoke('pin_get_init')),
  pinClose: (): void => fire('pin_close'),
  /** Окно без рамки двигаем сами: CSS app-region в WebView2 не работает */
  startDrag: (): void => void win.startDragging().catch(console.error),

  // ───── Панель записи ─────
  recGetConfig: (): Promise<RecConfig | null> => invoke('rec_get_config'),
  /** Запустить захват после отсчёта. Ошибку показывает Rust, панель просто закроется. */
  recStart: (): Promise<boolean> => invoke('rec_start'),
  recCommand: (cmd: RecCommand): void => fire('rec_command', { cmd }),
  /** Команды извне (хоткей, трей) и прогресс сборки GIF */
  onRecCommand: (cb: (cmd: RecCommand) => void): void => void win.listen<RecCommand>('rec:cmd', (e) => cb(e.payload)),
  onRecProgress: (cb: (percent: number) => void): void => void win.listen<number>('rec:progress', (e) => cb(e.payload)),

  // ───── Окно настроек ─────
  settingsGet: (): Promise<SettingsState | null> => invoke('settings_get'),
  settingsSet: (patch: SettingsPatch): Promise<SettingsState> => invoke('settings_set', { patch }),
  setHotkey: (name: HotkeyName, accel: string): Promise<HotkeyResult> => invoke('settings_set_hotkey', { name, accel }),
  hotkeysPause: (paused: boolean): void => fire('settings_hotkeys_pause', { paused }),
  chooseDir: (): Promise<DirResult> => invoke('settings_choose_dir'),
  openDir: (): void => fire('settings_open_dir'),
  donate: (): void => fire('settings_donate')
}

export type Api = typeof api

declare global {
  interface Window {
    api: Api
  }
}

window.api = api

// Окна без контекстного меню браузера и без перезагрузки по F5/Ctrl+R
document.addEventListener('contextmenu', (e) => e.preventDefault())
document.addEventListener('keydown', (e) => {
  if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && (e.key === 'r' || e.key === 'R'))) e.preventDefault()
})
