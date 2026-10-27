// Типы, общие для main, preload и renderer

export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

export type OverlayAction = 'copy' | 'save' | 'pin'

/** Оверлей используется и для снимка, и для выбора области записи */
export type OverlayMode = 'screenshot' | 'record'

export interface OverlayInit {
  /** Кадр экрана в родном разрешении (для показа; итог всё равно берётся из оригинала в Rust) */
  frame: ImageBitmap
  labels: Record<string, string>
  mode: OverlayMode
  /** Стартовые значения переключателей звука и формат записи (у GIF звука нет) */
  rec: { mic: boolean; systemAudio: boolean; format: RecFormat }
}

/** Lossless-кроп оригинала для сборки итогового снимка с аннотациями в renderer */
export interface CropData {
  png: Uint8Array
  /** Смещение кропа в пикселях кадра */
  left: number
  top: number
  /** Коэффициенты перевода CSS-пикселей окна в пиксели кадра */
  kx: number
  ky: number
}

// ───── Настройки ─────

export type Language = 'auto' | 'ru' | 'en'
export type HotkeyName = 'screenshot' | 'record'
export type RecFormat = 'mp4' | 'gif'

export interface AppSettings {
  language: Language
  hotkeys: Record<HotkeyName, string>
  saveDir: string
  /** Копировать снимок в буфер и одновременно сохранять файл в saveDir */
  autoSave: boolean
  autoLaunch: boolean
  /** Держать окно оверлея заранее загруженным: снимок появляется мгновенно, но это занимает память */
  fastStart: boolean
  /** Приветственное уведомление уже показывалось */
  welcomed: boolean
  recFormat: RecFormat
  recFps: number
  recMic: boolean
  recSystemAudio: boolean
  recCountdown: boolean
}

export type SettingsPatch = Partial<
  Pick<AppSettings, 'autoSave' | 'autoLaunch' | 'fastStart' | 'language' | 'recFormat' | 'recFps' | 'recMic' | 'recSystemAudio' | 'recCountdown'>
>

export interface SettingsState {
  settings: AppSettings
  /** Все строки интерфейса на текущем языке */
  strings: Record<string, string>
  platform: string
  version: string
  defaults: Record<HotkeyName, string>
}

export type HotkeyResult = { ok: true } | { ok: false; error: 'invalid' | 'busy' | 'duplicate' }
export type DirResult = { ok: true; state: SettingsState } | { ok: false; error?: 'dir' }

// ───── Запись экрана ─────

/** Что показывает и что делает панель записи */
export type RecUiState = 'countdown' | 'recording' | 'paused' | 'processing'
export type RecCommand = 'stop' | 'pause' | 'resume' | 'cancel'

/** Захват и кодирование делает Rust, панели нужно только это */
export interface RecConfig {
  format: RecFormat
  countdown: boolean
  strings: Record<string, string>
}
