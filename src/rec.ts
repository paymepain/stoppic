import './api'
import './rec.css'
import type { RecCommand, RecConfig, RecUiState } from './shared/ipc'
import { icon } from './shared/icons'

// Панель записи. Захват экрана, звук и кодирование делает Rust;
// здесь только отсчёт, таймер и кнопки.

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
let capturing = false // Rust уже пишет
let stopping = false
let paused = false
let accumulated = 0 // накопленное время записи без пауз, мс
let runStart = 0 // начало текущего отрезка записи
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

/** Rust сохранит файл и сам закроет панель */
function stop(): void {
  if (stopping || !capturing) return
  stopping = true
  window.clearInterval(timerId)
  setState('processing')
  label.textContent = s('recProcessing')
  window.api.recCommand('stop')
}

/** Без сохранения: Rust остановит запись, удалит файл и закроет панель */
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
  // Панель без рамки: тянем за любое место, кроме кнопок
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
  if (!(await window.api.recStart())) return // Rust уже показал ошибку и закрывает панель
  if (cancelled) return // отменили, пока запись запускалась

  capturing = true
  runStart = performance.now()
  setState('recording')
  timerId = window.setInterval(() => (timeEl.textContent = formatTime(elapsed())), 250)
}

main().catch((e) => {
  console.error(e)
  window.api.recCommand('cancel')
})
