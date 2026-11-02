// Types shared by main, preload and renderer

export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

export type OverlayAction = 'copy' | 'save' | 'pin'

/** The overlay is used both for screenshots and for selecting the recording area */
export type OverlayMode = 'screenshot' | 'record'

export interface OverlayInit {
  /** Screen frame at native resolution (for display; the result is still taken from the original in Rust) */
  frame: ImageBitmap
  labels: Record<string, string>
  mode: OverlayMode
  /** Initial values of the audio toggles and the recording format (GIF has no audio) */
  rec: { mic: boolean; systemAudio: boolean; format: RecFormat }
}

/** Lossless crop of the original for composing the final annotated screenshot in the renderer */
export interface CropData {
  png: Uint8Array
  /** Crop offset in frame pixels */
  left: number
  top: number
  /** Factors converting window CSS pixels to frame pixels */
  kx: number
  ky: number
}

// ───── Settings ─────

export type Language = 'ru' | 'en'
export type HotkeyName = 'screenshot' | 'screenshotFull' | 'screenshotWindow' | 'record' | 'recordFull'
export type RecFormat = 'mp4' | 'gif'

export interface AppSettings {
  language: Language
  hotkeys: Record<HotkeyName, string>
  saveDir: string
  /** Copy the screenshot to the clipboard and also save a file to saveDir */
  autoSave: boolean
  autoLaunch: boolean
  /** The welcome notification has already been shown */
  welcomed: boolean
  recFormat: RecFormat
  recFps: number
  recMic: boolean
  recSystemAudio: boolean
  /** WASAPI device IDs; an empty string means the Windows default device */
  recMicDevice: string
  recSysDevice: string
  recCountdown: boolean
}

export type SettingsPatch = Partial<
  Pick<
    AppSettings,
    'autoSave' | 'autoLaunch' | 'language' | 'recFormat' | 'recFps' | 'recMic' | 'recSystemAudio' | 'recMicDevice' | 'recSysDevice' | 'recCountdown'
  >
>

export interface AudioDevice {
  id: string
  name: string
}

export interface SettingsState {
  settings: AppSettings
  /** All UI strings in the current language */
  strings: Record<string, string>
  platform: string
  version: string
  defaults: Record<HotkeyName, string>
  /** Active microphones and output devices (for system audio) */
  mics: AudioDevice[]
  outputs: AudioDevice[]
}

export type HotkeyResult = { ok: true } | { ok: false; error: 'invalid' | 'busy' | 'duplicate' }
export type DirResult = { ok: true; state: SettingsState } | { ok: false; error?: 'dir' }

// ───── Screen recording ─────

/** What the recording panel shows and does */
export type RecUiState = 'countdown' | 'recording' | 'paused' | 'processing'
export type RecCommand = 'stop' | 'pause' | 'resume' | 'cancel'

/** Rust does the capture and encoding; the panel only needs this */
export interface RecConfig {
  format: RecFormat
  countdown: boolean
  strings: Record<string, string>
}
