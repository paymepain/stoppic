import './api'
import './rec.css'
import type { RecCommand, RecConfig, RecUiState } from './shared/ipc'
import { icon } from './shared/icons'

// Recording panel. Screen capture, audio and encoding are done by Rust;
// here there is only the countdown, the timer and the buttons.

const $ = <T extends HTMLElement = HTMLElement>(id: string): T => document.getElementById(id) as T
const pill = $('pill')
const count = $('count')
const timeEl = $('time')
const label = $('label')
const btnPause = $('pause')
const btnStop = $('stop')
const btnCancel = $('cancel')
btnPause.innerHTML = icon('pause', 18, { cls: 'i-pause' }) + icon('play', 18, { cls: 'i-play' })
btnStop.innerHTML = icon('stop', 18)
btnCancel.innerHTML = icon('close')

let cfg: RecConfig
let cancelled = false
let capturing = false // Rust is already recording
let stopping = false
let paused = false
let accumulated = 0 // accumulated recording time excluding pauses, ms
let runStart = 0 // start of the current recording segment
let timerId: number | undefined

const s = (key: string): string => cfg.strings[key] ?? key
const setState = (st: RecUiState): void => {
  pill.dataset.state = st
}
const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms))
const elapsed = (): number => accumulated + (runStart ? performance.now() - runStart : 0)

function formatTime(ms: number): string {
  const total = Math.floor(ms / 1000)
  const p = (n: number): string => String(n).padStart(2, '0')
  const h = Math.floor(total / 3600)
  return `${h > 0 ? `${p(h)}:` : ''}${p(Math.floor((total % 3600) / 60))}:${p(total % 60)}`
}

function pause(): void {
  if (!capturing || stopping || paused) return
  accumulated += performance.now() - runStart
  runStart = 0
  paused = true
  setState('paused')
  btnPause.title = s('recResume')
  window.api.recCommand('pause')
}

function resume(): void {
  if (!capturing || stopping || !paused) return
  runStart = performance.now()
  paused = false
  setState('recording')
  btnPause.title = s('recPause')
  window.api.recCommand('resume')
}

/** Rust saves the file and closes the panel itself */
function stop(): void {
  if (stopping || !capturing) return
  stopping = true
  window.clearInterval(timerId)
  setState('processing')
  label.textContent = s('recProcessing')
  window.api.recCommand('stop')
}

/** Without saving: Rust stops the recording, deletes the file and closes the panel */
function cancel(): void {
  if (stopping || cancelled) return
  cancelled = true
  window.clearInterval(timerId)
  window.api.recCommand('cancel')
}

async function main(): Promise<void> {
  const c = await window.api.recGetConfig()
  if (!c) return
  cfg = c
  btnStop.title = s('recStop')
  btnCancel.title = s('recCancel')
  btnPause.title = s('recPause')

  window.api.onRecCommand((cmd: RecCommand) => {
    if (cmd === 'cancel') cancel()
    else if (cmd === 'stop') stop()
    else if (cmd === 'pause') pause()
    else if (cmd === 'resume') resume()
  })
  window.api.onRecProgress((p) => (label.textContent = `${s('recProcessing')} ${p}%`))

  btnPause.addEventListener('click', () => (paused ? resume() : pause()))
  btnStop.addEventListener('click', stop)
  btnCancel.addEventListener('click', cancel)
  // Frameless panel: drag by any spot except the buttons
  pill.addEventListener('mousedown', (e) => {
    if (e.button === 0 && !(e.target as HTMLElement).closest('button')) window.api.startDrag()
  })

  if (cfg.countdown) {
    setState('countdown')
    for (let n = 3; n > 0; n--) {
      count.textContent = String(n)
      await sleep(1000)
      if (cancelled) return
    }
  }
  if (cancelled) return
  if (!(await window.api.recStart())) return // Rust has already shown the error and is closing the panel
  if (cancelled) return // cancelled while the recording was starting

  capturing = true
  runStart = performance.now()
  setState('recording')
  timerId = window.setInterval(() => (timeEl.textContent = formatTime(elapsed())), 250)
}

main().catch((e) => {
  console.error(e)
  window.api.recCommand('cancel')
})
