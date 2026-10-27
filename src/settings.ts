import './api'
import './settings.css'
import type { HotkeyName, HotkeyResult, SettingsState } from './shared/ipc'
import { LOGO_SVG, LOGO_VIEWBOX } from './shared/brand'
import { icon } from './shared/icons'

let st: SettingsState
let recording: HotkeyName | null = null
let msgTimer: number | undefined

const app = document.getElementById('app') as HTMLElement
const s = (key: string): string => st.strings[key] ?? key

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = '', text = ''): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag)
  if (cls) e.className = cls
  if (text) e.textContent = text
  return e
}

// ───────────────────────── Сочетания клавиш ─────────────────────────

const NAMED_KEYS: Record<string, string> = {
  Space: 'Space', Tab: 'Tab', Enter: 'Return', Backspace: 'Backspace', Delete: 'Delete', Insert: 'Insert',
  Home: 'Home', End: 'End', PageUp: 'PageUp', PageDown: 'PageDown',
  ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right', PrintScreen: 'PrintScreen',
  Minus: '-', Equal: '=', BracketLeft: '[', BracketRight: ']', Backslash: '\\', Semicolon: ';',
  Quote: "'", Comma: ',', Period: '.', Slash: '/', Backquote: '`'
}

/** Физическая клавиша → имя для Electron. Работает в любой раскладке. */
function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3)
  if (/^Digit\d$/.test(code)) return code.slice(5)
  if (/^F([1-9]|1\d|2[0-4])$/.test(code)) return code
  return NAMED_KEYS[code] ?? null
}

const isModifierCode = (code: string): boolean => /^(Control|Shift|Alt|Meta|AltGraph|OS)/.test(code)

function toAccelerator(e: KeyboardEvent): { accel?: string; needMod?: boolean } {
  const key = keyName(e.code)
  if (!key) return {}
  const mac = st.platform === 'darwin'
  const mods: string[] = []
  if (mac ? e.metaKey : e.ctrlKey) mods.push('CommandOrControl')
  if (mac && e.ctrlKey) mods.push('Control')
  if (!mac && e.metaKey) mods.push('Super')
  if (e.altKey) mods.push('Alt')
  if (e.shiftKey) mods.push('Shift')
  const free = /^F\d+$/.test(key) || key === 'PrintScreen'
  if (mods.length === 0 && !free) return { needMod: true }
  return { accel: [...mods, key].join('+') }
}

function keycaps(accel: string): HTMLElement[] {
  const mac = st.platform === 'darwin'
  const names: Record<string, string> = mac
    ? { CommandOrControl: '⌘', Control: '⌃', Alt: '⌥', Shift: '⇧', Super: '⌘' }
    : { CommandOrControl: 'Ctrl', Control: 'Ctrl', Alt: 'Alt', Shift: 'Shift', Super: 'Win' }
  return accel.split('+').map((part) => el('kbd', '', names[part] ?? (part === 'Return' ? 'Enter' : part)))
}

function showMsg(text: string): void {
  const m = document.getElementById('msg')
  if (!m) return
  m.textContent = text
  window.clearTimeout(msgTimer)
  if (text) msgTimer = window.setTimeout(() => (m.textContent = ''), 4500)
}

function errorText(res: Extract<HotkeyResult, { ok: false }>): string {
  return s(res.error === 'busy' ? 'hkErrBusy' : res.error === 'duplicate' ? 'hkErrDup' : 'hkErrInvalid')
}

async function applyHotkey(name: HotkeyName, accel: string): Promise<void> {
  const res = await window.api.setHotkey(name, accel)
  window.api.hotkeysPause(false)
  const next = await window.api.settingsGet()
  if (next) st = next
  render()
  if (!res.ok) showMsg(errorText(res))
}

function stopRecording(): void {
  if (!recording) return
  recording = null
  window.removeEventListener('keydown', onRecordKey, true)
  window.removeEventListener('blur', cancelRecording)
}

function cancelRecording(): void {
  if (!recording) return
  stopRecording()
  window.api.hotkeysPause(false)
  render()
}

function onRecordKey(e: KeyboardEvent): void {
  e.preventDefault()
  e.stopPropagation()
  if (!recording) return
  if (e.code === 'Escape') {
    cancelRecording()
    return
  }
  if (isModifierCode(e.code)) return // ждём основную клавишу
  const { accel, needMod } = toAccelerator(e)
  if (needMod) {
    showMsg(s('hkErrMod'))
    return
  }
  if (!accel) {
    showMsg(s('hkErrInvalid'))
    return
  }
  const name = recording
  stopRecording()
  void applyHotkey(name, accel)
}

function startRecording(name: HotkeyName): void {
  if (recording) cancelRecording()
  recording = name
  window.api.hotkeysPause(true) // иначе текущее сочетание сработает прямо во время записи
  window.addEventListener('keydown', onRecordKey, true)
  window.addEventListener('blur', cancelRecording)
  render()
}

// ───────────────────────── Разметка ─────────────────────────

function hotkeyRow(name: HotkeyName): HTMLElement {
  const row = el('div', 'row')
  row.appendChild(el('div', 'lbl', s(name)))

  const field = el('div', 'keyfield')
  const reset = el('button', 'reset')
  reset.innerHTML = icon('reset', 14)
  reset.title = s('hkReset')
  reset.setAttribute('aria-label', s('hkReset'))
  reset.hidden = st.settings.hotkeys[name] === st.defaults[name] || recording === name
  reset.addEventListener('click', () => {
    window.api.hotkeysPause(true)
    void applyHotkey(name, st.defaults[name])
  })

  const key = el('button', recording === name ? 'key rec' : 'key')
  if (recording === name) key.textContent = s('hkPress')
  else key.append(...keycaps(st.settings.hotkeys[name]))
  key.addEventListener('click', () => startRecording(name))

  field.append(reset, key)
  row.appendChild(field)
  return row
}

function toggleRow(label: string, checked: boolean, onChange: (v: boolean) => void, hint = ''): HTMLElement {
  const row = el('label', 'row')
  const text = el('div', 'lbl', label)
  if (hint) text.appendChild(el('small', 'hint', hint))
  const input = el('input', 'sw')
  input.type = 'checkbox'
  input.checked = checked
  input.addEventListener('change', () => onChange(input.checked))
  row.append(text, input)
  return row
}

async function patch(p: Parameters<typeof window.api.settingsSet>[0]): Promise<void> {
  st = await window.api.settingsSet(p)
  render()
}

function folderRow(): HTMLElement {
  const row = el('div', 'row')
  const path = el('div', 'path', st.settings.saveDir)
  path.title = st.settings.saveDir

  const change = el('button', '', s('folderChange'))
  change.addEventListener('click', async () => {
    const res = await window.api.chooseDir()
    if (res.ok) {
      st = res.state
      render()
    } else if (res.error === 'dir') {
      showMsg(s('folderErr'))
    }
  })
  const open = el('button', '', s('folderOpen'))
  open.addEventListener('click', () => window.api.openDir())

  const btns = el('div', 'btns')
  btns.append(change, open)
  row.append(path, btns)
  return row
}

function selectRow(label: string, options: Array<[string, string]>, value: string, onChange: (v: string) => void): HTMLElement {
  const row = el('div', 'row')
  row.appendChild(el('div', 'lbl', label))
  const sel = el('select')
  for (const [v, text] of options) {
    const o = el('option', '', text)
    o.value = v
    o.selected = v === value
    sel.appendChild(o)
  }
  sel.addEventListener('change', () => onChange(sel.value))
  row.appendChild(sel)
  return row
}

function languageRow(): HTMLElement {
  const row = el('div', 'row')
  row.appendChild(el('div', 'lbl', s('language')))
  const sel = el('select')
  for (const [value, label] of [['auto', s('langAuto')], ['ru', 'Русский'], ['en', 'English']] as const) {
    const o = el('option', '', label)
    o.value = value
    o.selected = st.settings.language === value
    sel.appendChild(o)
  }
  sel.addEventListener('change', () => void patch({ language: sel.value as 'auto' | 'ru' | 'en' }))
  row.appendChild(sel)
  return row
}

function render(): void {
  document.documentElement.lang = st.settings.language === 'auto' ? '' : st.settings.language

  const hotkeys = el('div', 'card')
  hotkeys.append(hotkeyRow('screenshot'), hotkeyRow('record'))

  const saving = el('div', 'card')
  saving.append(
    folderRow(),
    toggleRow(s('autoSave'), st.settings.autoSave, (v) => void patch({ autoSave: v }))
  )

  const general = el('div', 'card')
  general.append(
    toggleRow(s('autoLaunch'), st.settings.autoLaunch, (v) => void patch({ autoLaunch: v })),
    toggleRow(s('fastStart'), st.settings.fastStart, (v) => void patch({ fastStart: v }), s('fastStartHint')),
    languageRow()
  )

  const record = el('div', 'card')
  record.append(
    selectRow(s('recFormat'), [['mp4', 'MP4'], ['gif', 'GIF']], st.settings.recFormat, (v) => void patch({ recFormat: v as 'mp4' | 'gif' })),
    selectRow(s('recFps'), [['15', '15'], ['30', '30'], ['60', '60']], String(st.settings.recFps), (v) => void patch({ recFps: Number(v) })),
    toggleRow(s('recCountdown'), st.settings.recCountdown, (v) => void patch({ recCountdown: v })),
    toggleRow(s('recSystemAudio'), st.settings.recSystemAudio, (v) => void patch({ recSystemAudio: v })),
    toggleRow(s('recMic'), st.settings.recMic, (v) => void patch({ recMic: v }))
  )

  const msg = el('div', 'msg')
  msg.id = 'msg'

  const foot = el('div', 'foot')
  const brand = el('div', 'brand')
  brand.innerHTML = `<svg viewBox="${LOGO_VIEWBOX}" height="20" role="img" aria-label="StopPic">${LOGO_SVG}</svg>`
  const ver = el('span', 'ver', st.version)
  ver.title = `${s('version')} ${st.version}`
  brand.appendChild(ver)
  foot.appendChild(brand)
  const support = el('button', 'support', s('support'))
  support.addEventListener('click', () => window.api.donate())
  foot.appendChild(support)

  app.replaceChildren(
    el('div', 'cap', s('secHotkeys')), hotkeys,
    msg,
    el('div', 'cap', s('secSaving')), saving,
    el('div', 'cap', s('secRecord')), record,
    el('div', 'cap', s('secGeneral')), general,
    foot
  )
}

async function init(): Promise<void> {
  const state = await window.api.settingsGet()
  if (!state) return
  st = state
  render()
}

void init()
