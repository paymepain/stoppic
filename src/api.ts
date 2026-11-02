// Bridge to the Rust side. Mirrors the former window.api from the Electron preload,
// so the window code barely changed.
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
/** Latency measurements in dev mode (printed to the Rust console) */
export const devLog = (msg: string): void => {
  if (import.meta.env.DEV) void invoke('dev_log', { msg })
}
const fire = (cmd: string, args?: Record<string, unknown>): void =>
  void invoke(cmd, args).catch((e) => void invoke('dev_log', { msg: `${cmd}: ${String(e)}` }))

/** An empty response means "no data" (not an image, dialog cancelled, etc.) */
const bytesOrNull = (buf: ArrayBuffer): Uint8Array | null => (buf.byteLength > 0 ? new Uint8Array(buf) : null)

// Subscribe to the frame right at load time so ready() cannot overtake the subscription
let initCb: ((data: OverlayInit) => void) | null = null
let revealCb: (() => void) | null = null
/** Frame: 8 bytes (width, height: u32 LE), then RGBA */
async function toBitmap(buf: ArrayBuffer): Promise<ImageBitmap> {
  const head = new DataView(buf, 0, 8)
  const w = head.getUint32(0, true)
  const h = head.getUint32(4, true)
  return createImageBitmap(new ImageData(new Uint8ClampedArray(buf, 8, w * h * 4), w, h))
}

// Windows fast path: the frame arrives in WebView2 shared memory, with no copy through IPC
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
    // Raw uncompressed pixels: over Tauri IPC this is faster than encoding JPEG.
    // First 8 bytes: width and height (u32 LE), then RGBA
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
  // ───── Screenshot overlay / recording area selection ─────
  /** Page is loaded and ready to receive the frame (the overlay is created ahead of time and waits) */
  ready: (): void => void listening.then(() => invoke('overlay_ready')),
  onInit: (cb: (data: OverlayInit) => void): void => {
    initCb = cb
  },
  shown: (): void => fire('overlay_shown'),
  /** Window is shown transparent: wait until the frame has been drawn in it */
  onReveal: (cb: () => void): void => {
    revealCb = cb
  },
  revealed: (): void => fire('overlay_revealed'),
  cancel: (): void => fire('overlay_cancel'),
  /** Fast path: screenshot without annotations, Rust does the crop */
  action: (type: OverlayAction, rect: Rect): void => fire('overlay_action', { kind: type, rect }),
  /** Annotation path: fetch a lossless crop, compose the result and send it back */
  getCrop: async (rect: Rect): Promise<CropData | null> => {
    const buf = await invoke<ArrayBuffer>('overlay_get_crop', { rect })
    if (buf.byteLength <= 32) return null
    // First 32 bytes: left, top, kx, ky (f64, little-endian), then PNG
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
  /** While text is being edited, Esc must not close the whole overlay */
  setEscape: (enabled: boolean): void => fire('overlay_escape', { enabled }),
  /** Image from the clipboard (PNG), or null if the clipboard holds no image */
  readClipboardImage: async (): Promise<Uint8Array | null> => bytesOrNull(await invoke('overlay_clipboard_image')),
  /** File picker dialog for an image */
  pickImageFile: async (): Promise<Uint8Array | null> => bytesOrNull(await invoke('overlay_pick_image')),
  /** Recording mode: area selected, start recording */
  recordStart: (rect: Rect, opts: { mic: boolean; systemAudio: boolean }): void =>
    fire('overlay_record', { rect, mic: opts.mic, systemAudio: opts.systemAudio }),

  // ───── Pinned screenshot ─────
  pinGetInit: async (): Promise<Uint8Array | null> => bytesOrNull(await invoke('pin_get_init')),
  pinClose: (): void => fire('pin_close'),
  /** We drag the frameless window ourselves: CSS app-region does not work in WebView2 */
  startDrag: (): void => void win.startDragging().catch(console.error),

  // ───── Recording panel ─────
  recGetConfig: (): Promise<RecConfig | null> => invoke('rec_get_config'),
  /** Start capture after the countdown. Rust shows any error; the panel just closes. */
  recStart: (): Promise<boolean> => invoke('rec_start'),
  recCommand: (cmd: RecCommand): void => fire('rec_command', { cmd }),
  /** External commands (hotkey, tray) and GIF assembly progress */
  onRecCommand: (cb: (cmd: RecCommand) => void): void => void win.listen<RecCommand>('rec:cmd', (e) => cb(e.payload)),
  onRecProgress: (cb: (percent: number) => void): void => void win.listen<number>('rec:progress', (e) => cb(e.payload)),

  // ───── Settings window ─────
  settingsGet: (): Promise<SettingsState | null> => invoke('settings_get'),
  settingsSet: (patch: SettingsPatch): Promise<SettingsState> => invoke('settings_set', { patch }),
  /** Content height in CSS px: the window shrinks to it and cannot be made taller */
  settingsFit: (height: number): void => fire('settings_fit', { height }),
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

// Windows have no browser context menu and no reload on F5/Ctrl+R
document.addEventListener('contextmenu', (e) => e.preventDefault())
document.addEventListener('keydown', (e) => {
  if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && (e.key === 'r' || e.key === 'R'))) e.preventDefault()
})
