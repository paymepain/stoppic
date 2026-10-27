import { devLog } from './api'
import './overlay.css'
import type { OverlayAction, OverlayInit, OverlayMode, Rect } from './shared/ipc'
import { icon, type IconName } from './shared/icons'

// ───────────────────────── Модель аннотаций ─────────────────────────
// Координаты аннотаций хранятся в CSS-пикселях окна, поэтому не зависят от рамки выделения.

interface Base { id: number; color: string; s: number } // s: индекс размера 0..2
interface ArrowA extends Base { type: 'arrow'; x1: number; y1: number; x2: number; y2: number }
interface LineA extends Base { type: 'line'; x1: number; y1: number; x2: number; y2: number }
interface RectA extends Base { type: 'rect'; x: number; y: number; w: number; h: number }
interface MarkerA extends Base { type: 'marker'; pts: Array<[number, number]> }
interface PenA extends Base { type: 'pen'; pts: Array<[number, number]> }
/** Вставленная картинка. Сами пиксели лежат в реестре images, а не в истории: иначе каждый шаг отмены копировал бы мегабайты. */
interface ImageA extends Base { type: 'image'; x: number; y: number; w: number; h: number; img: number }
interface TextA extends Base { type: 'text'; x: number; y: number; text: string }
interface NumberA extends Base { type: 'number'; x: number; y: number; n: number }
interface BlurA extends Base { type: 'blur'; x: number; y: number; w: number; h: number }
type Ann = ArrowA | LineA | RectA | MarkerA | PenA | TextA | NumberA | BlurA | ImageA
type Tool = 'select' | 'arrow' | 'line' | 'pen' | 'rect' | 'marker' | 'text' | 'blur' | 'number'
type DrawTool = Exclude<Tool, 'select'>

const LINE_W = [2, 4, 7]
const MARK_W = [12, 20, 30]
const images = new Map<number, ImageBitmap>() // id → пиксели вставленных картинок
let imgSeq = 0
const TEXT_PX = [16, 22, 32]
const NUM_R = [11, 14, 19]
const CELL = [6, 10, 16]
const COLORS = ['#ef4444', '#facc15', '#22c55e', '#3b82f6', '#111827']
const FONT = 'system-ui, -apple-system, "Segoe UI", Roboto, sans-serif'
const MIN = 4

/** Перевод CSS-пикселей окна в пиксели холста: px = x * kx + tx */
interface Xf { kx: number; ky: number; tx: number; ty: number }

const clamp = (v: number, lo: number, hi: number): number => Math.min(Math.max(v, lo), hi)
const fontOf = (a: TextA): string => `600 ${TEXT_PX[a.s]}px ${FONT}`
const lineH = (a: TextA): number => TEXT_PX[a.s] * 1.25

const mctx = document.createElement('canvas').getContext('2d') as CanvasRenderingContext2D

function textSize(a: TextA): { w: number; h: number } {
  mctx.font = fontOf(a)
  const lines = a.text.split('\n')
  return { w: Math.max(...lines.map((l) => mctx.measureText(l).width)), h: lines.length * lineH(a) }
}

function bounds(a: Ann): Rect {
  switch (a.type) {
    case 'arrow':
    case 'line': {
      const x = Math.min(a.x1, a.x2)
      const y = Math.min(a.y1, a.y2)
      return { x, y, width: Math.abs(a.x2 - a.x1), height: Math.abs(a.y2 - a.y1) }
    }
    case 'rect':
    case 'blur':
    case 'image':
      return { x: a.x, y: a.y, width: a.w, height: a.h }
    case 'marker':
    case 'pen': {
      const xs = a.pts.map((p) => p[0])
      const ys = a.pts.map((p) => p[1])
      const r = (a.type === 'pen' ? LINE_W[a.s] : MARK_W[a.s]) / 2
      const x = Math.min(...xs) - r
      const y = Math.min(...ys) - r
      return { x, y, width: Math.max(...xs) + r - x, height: Math.max(...ys) + r - y }
    }
    case 'text': {
      const m = textSize(a)
      return { x: a.x, y: a.y, width: m.w, height: m.h }
    }
    case 'number': {
      const r = NUM_R[a.s]
      return { x: a.x - r, y: a.y - r, width: r * 2, height: r * 2 }
    }
  }
}

function distSeg(px: number, py: number, x1: number, y1: number, x2: number, y2: number): number {
  const dx = x2 - x1
  const dy = y2 - y1
  const l2 = dx * dx + dy * dy
  const t = l2 ? clamp(((px - x1) * dx + (py - y1) * dy) / l2, 0, 1) : 0
  return Math.hypot(px - (x1 + t * dx), py - (y1 + t * dy))
}

function hit(a: Ann, x: number, y: number): boolean {
  const tol = 6
  switch (a.type) {
    case 'arrow':
    case 'line':
      return distSeg(x, y, a.x1, a.y1, a.x2, a.y2) <= LINE_W[a.s] / 2 + tol
    case 'rect': {
      const outer = x >= a.x - tol && x <= a.x + a.w + tol && y >= a.y - tol && y <= a.y + a.h + tol
      const inner = x > a.x + tol && x < a.x + a.w - tol && y > a.y + tol && y < a.y + a.h - tol
      return outer && !inner // попадание только по контуру, чтобы внутри рамки можно было двигать выделение
    }
    case 'marker':
    case 'pen': {
      const r = (a.type === 'pen' ? LINE_W[a.s] / 2 + 4 : MARK_W[a.s] / 2) // у тонкого пера запас, чтобы в него можно было попасть
      if (a.pts.length === 1) return Math.hypot(x - a.pts[0][0], y - a.pts[0][1]) <= r
      for (let i = 1; i < a.pts.length; i++) {
        const p = a.pts[i - 1]
        const q = a.pts[i]
        if (distSeg(x, y, p[0], p[1], q[0], q[1]) <= r) return true
      }
      return false
    }
    case 'text':
    case 'blur':
    case 'image': {
      const b = bounds(a)
      return x >= b.x && x <= b.x + b.width && y >= b.y && y <= b.y + b.height
    }
    case 'number':
      return Math.hypot(x - a.x, y - a.y) <= NUM_R[a.s] + 2
  }
}

function moved(a: Ann, dx: number, dy: number): Ann {
  switch (a.type) {
    case 'arrow':
    case 'line':
      return { ...a, x1: a.x1 + dx, y1: a.y1 + dy, x2: a.x2 + dx, y2: a.y2 + dy }
    case 'marker':
    case 'pen':
      return { ...a, pts: a.pts.map((p): [number, number] => [p[0] + dx, p[1] + dy]) }
    default:
      return { ...a, x: a.x + dx, y: a.y + dy }
  }
}

// ───────────────────────── Рисование ─────────────────────────

const tmp = document.createElement('canvas')

function textOn(color: string): string {
  const n = parseInt(color.slice(1), 16)
  const lum = 0.299 * (n >> 16) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255)
  return lum > 150 ? '#111827' : '#ffffff'
}

/** Пикселизация области холста: необратима, в отличие от лёгкого гауссова размытия. */
function pixelate(ctx: CanvasRenderingContext2D, a: BlurA, xf: Xf): void {
  const cw = ctx.canvas.width
  const ch = ctx.canvas.height
  const x0 = Math.max(0, Math.round(a.x * xf.kx + xf.tx))
  const y0 = Math.max(0, Math.round(a.y * xf.ky + xf.ty))
  const x1 = Math.min(cw, Math.round((a.x + a.w) * xf.kx + xf.tx))
  const y1 = Math.min(ch, Math.round((a.y + a.h) * xf.ky + xf.ty))
  if (x1 <= x0 || y1 <= y0) return

  const cell = Math.max(2, Math.round(CELL[a.s] * xf.kx))
  const sw = x1 - x0
  const sh = y1 - y0
  const tw = Math.max(1, Math.round(sw / cell))
  const th = Math.max(1, Math.round(sh / cell))

  tmp.width = tw
  tmp.height = th
  const t = tmp.getContext('2d') as CanvasRenderingContext2D
  t.imageSmoothingEnabled = true
  t.imageSmoothingQuality = 'high'
  t.drawImage(ctx.canvas, x0, y0, sw, sh, 0, 0, tw, th)

  ctx.save()
  ctx.setTransform(1, 0, 0, 1, 0, 0)
  ctx.imageSmoothingEnabled = false
  ctx.drawImage(tmp, 0, 0, tw, th, x0, y0, sw, sh)
  ctx.restore()
}

function drawAnn(ctx: CanvasRenderingContext2D, a: Ann, xf: Xf): void {
  if (a.type === 'blur') {
    pixelate(ctx, a, xf)
    return
  }
  ctx.save()
  ctx.setTransform(xf.kx, 0, 0, xf.ky, xf.tx, xf.ty)
  ctx.strokeStyle = a.color
  ctx.fillStyle = a.color
  ctx.lineCap = 'round'
  ctx.lineJoin = 'round'

  switch (a.type) {
    case 'arrow': {
      const w = LINE_W[a.s]
      const head = 9 + w * 2.2
      const len = Math.hypot(a.x2 - a.x1, a.y2 - a.y1)
      if (len < 1) break
      const ang = Math.atan2(a.y2 - a.y1, a.x2 - a.x1)
      if (len > head * 0.8) {
        ctx.lineWidth = w
        ctx.beginPath()
        ctx.moveTo(a.x1, a.y1)
        ctx.lineTo(a.x2 - Math.cos(ang) * head * 0.8, a.y2 - Math.sin(ang) * head * 0.8)
        ctx.stroke()
      }
      ctx.beginPath()
      ctx.moveTo(a.x2, a.y2)
      ctx.lineTo(a.x2 - head * Math.cos(ang - 0.42), a.y2 - head * Math.sin(ang - 0.42))
      ctx.lineTo(a.x2 - head * Math.cos(ang + 0.42), a.y2 - head * Math.sin(ang + 0.42))
      ctx.closePath()
      ctx.fill()
      break
    }
    case 'line':
      ctx.lineWidth = LINE_W[a.s]
      ctx.beginPath()
      ctx.moveTo(a.x1, a.y1)
      ctx.lineTo(a.x2, a.y2)
      ctx.stroke()
      break
    case 'rect':
      ctx.lineWidth = LINE_W[a.s]
      ctx.strokeRect(a.x, a.y, a.w, a.h)
      break
    case 'pen': {
      // Свободная линия, сглаженная квадратичными кривыми через середины отрезков
      const p = a.pts
      ctx.lineWidth = LINE_W[a.s]
      ctx.beginPath()
      ctx.moveTo(p[0][0], p[0][1])
      if (p.length < 3) {
        ctx.lineTo(p[p.length - 1][0], p[p.length - 1][1])
      } else {
        for (let i = 1; i < p.length - 1; i++) {
          ctx.quadraticCurveTo(p[i][0], p[i][1], (p[i][0] + p[i + 1][0]) / 2, (p[i][1] + p[i + 1][1]) / 2)
        }
        ctx.lineTo(p[p.length - 1][0], p[p.length - 1][1])
      }
      ctx.stroke()
      break
    }
    case 'image': {
      const bmp = images.get(a.img)
      if (bmp) {
        ctx.imageSmoothingQuality = 'high'
        ctx.drawImage(bmp, a.x, a.y, a.w, a.h)
      }
      break
    }
    case 'marker':
      ctx.globalAlpha = 0.4
      ctx.lineWidth = MARK_W[a.s]
      ctx.beginPath()
      ctx.moveTo(a.pts[0][0], a.pts[0][1])
      if (a.pts.length === 1) ctx.lineTo(a.pts[0][0], a.pts[0][1])
      for (const p of a.pts.slice(1)) ctx.lineTo(p[0], p[1])
      ctx.stroke()
      break
    case 'text': {
      ctx.font = fontOf(a)
      ctx.textBaseline = 'top'
      const lh = lineH(a)
      const lead = (lh - TEXT_PX[a.s]) / 2 // половина интерлиньяжа, как в textarea
      a.text.split('\n').forEach((line, i) => ctx.fillText(line, a.x, a.y + i * lh + lead))
      break
    }
    case 'number': {
      const r = NUM_R[a.s]
      ctx.beginPath()
      ctx.arc(a.x, a.y, r, 0, Math.PI * 2)
      ctx.fill()
      ctx.fillStyle = textOn(a.color)
      ctx.font = `700 ${Math.round(r * 1.15)}px ${FONT}`
      ctx.textAlign = 'center'
      ctx.textBaseline = 'middle'
      ctx.fillText(String(a.n), a.x, a.y + 0.5)
      break
    }
  }
  ctx.restore()
}

function drawAll(ctx: CanvasRenderingContext2D, list: Ann[], xf: Xf, selectedId: number | null): void {
  for (const a of list) drawAnn(ctx, a, xf)
  const s = selectedId === null ? undefined : list.find((a) => a.id === selectedId)
  if (s) {
    const b = bounds(s)
    ctx.save()
    ctx.setTransform(xf.kx, 0, 0, xf.ky, xf.tx, xf.ty)
    ctx.strokeStyle = '#f03a2e'
    ctx.lineWidth = 1.2
    ctx.setLineDash([4, 3])
    ctx.strokeRect(b.x - 4, b.y - 4, b.width + 8, b.height + 8)
    if (s.type === 'image') {
      // Ручка в правом нижнем углу: тянем, чтобы менять размер с сохранением пропорций
      ctx.setLineDash([])
      ctx.fillStyle = '#ffffff'
      ctx.fillRect(b.x + b.width - 5, b.y + b.height - 5, 10, 10)
      ctx.strokeRect(b.x + b.width - 5, b.y + b.height - 5, 10, 10)
    }
    ctx.restore()
  }
}

// ───────────────────────── Состояние ─────────────────────────

const $ = <T extends HTMLElement = HTMLElement>(id: string): T => document.getElementById(id) as T
const cv = $<HTMLCanvasElement>('cv')
const cctx = cv.getContext('2d') as CanvasRenderingContext2D
const dim = $('dim')
const sel = $('sel')
const sizeLabel = $('size')
const bar = $('bar')
const rowMain = $('row-main')
const rowOpts = $('row-opts')

let bgImg: ImageBitmap
let live: Xf = { kx: 1, ky: 1, tx: 0, ty: 0 }

let rect: Rect | null = null // рамка выделения
let items: Ann[] = []
let draft: Ann | null = null
let selectedId: number | null = null
let seq = 0
let tool: Tool = 'select'
let busy = false
// Один оверлей служит и для снимка, и для выбора области записи
let appMode: OverlayMode = 'screenshot'
const rec = { mic: false, sys: false, gif: false }

const toolColor: Record<DrawTool, string> = {
  arrow: COLORS[0], line: COLORS[0], pen: COLORS[0], rect: COLORS[0], marker: COLORS[1], text: COLORS[0], blur: COLORS[0], number: COLORS[0]
}
const toolSize: Record<DrawTool, number> = { arrow: 1, line: 1, pen: 1, rect: 1, marker: 1, text: 1, blur: 1, number: 1 }

// История: снимки массива аннотаций
let history: string[] = ['[]']
let hIndex = 0

function commit(): void {
  history = history.slice(0, hIndex + 1)
  history.push(JSON.stringify(items))
  if (history.length > 100) history.shift()
  hIndex = history.length - 1
  syncUI()
}

function restore(i: number): void {
  hIndex = i
  items = JSON.parse(history[i]) as Ann[]
  if (selectedId !== null && !items.some((a) => a.id === selectedId)) selectedId = null
  syncUI()
  schedulePaint()
}

const undo = (): void => {
  if (hIndex > 0) restore(hIndex - 1)
}
const redo = (): void => {
  if (hIndex < history.length - 1) restore(hIndex + 1)
}

let paintQueued = false
function schedulePaint(): void {
  if (paintQueued) return
  paintQueued = true
  requestAnimationFrame(() => {
    paintQueued = false
    cctx.setTransform(1, 0, 0, 1, 0, 0)
    cctx.drawImage(bgImg, 0, 0)
    drawAll(cctx, draft ? [...items, draft] : items, live, selectedId)
  })
}

// ───────────────────────── Панель ─────────────────────────

const buttons = new Map<IconName, HTMLButtonElement>()
let labels: Record<string, string> = {}

function makeBtn(id: IconName, onClick: () => void, cls = ''): HTMLButtonElement {
  const b = document.createElement('button')
  b.type = 'button'
  if (cls) b.className = cls
  b.innerHTML = icon(id)
  b.addEventListener('click', onClick)
  buttons.set(id, b)
  return b
}

const sep = (): HTMLElement => Object.assign(document.createElement('div'), { className: 'sep' })

const TOOLS: Tool[] = ['select', 'pen', 'line', 'arrow', 'rect', 'marker', 'text', 'blur', 'number']
const dotEls: HTMLElement[] = []
const szEls: HTMLElement[] = []
let colorGroup: HTMLElement[] = []

function buildRecordBar(): void {
  if (!rec.gif) {
    // GIF без звука: переключатели не нужны
    rowMain.appendChild(makeBtn('mic', () => { rec.mic = !rec.mic; syncUI() }))
    rowMain.appendChild(makeBtn('sys', () => { rec.sys = !rec.sys; syncUI() }))
    rowMain.appendChild(sep())
  }
  rowMain.appendChild(makeBtn('record', startRecord, 'primary rec'))
  rowMain.appendChild(makeBtn('close', () => window.api.cancel()))
}

function buildBar(): void {
  if (appMode === 'record') {
    buildRecordBar()
    return
  }
  for (const tl of TOOLS) rowMain.appendChild(makeBtn(tl, () => setTool(tl)))
  rowMain.appendChild(makeBtn('image', () => void insertImage(true)))
  rowMain.appendChild(sep())
  rowMain.appendChild(makeBtn('undo', undo))
  rowMain.appendChild(makeBtn('redo', redo))
  rowMain.appendChild(sep())
  rowMain.appendChild(makeBtn('copy', () => void finishWith('copy'), 'primary'))
  rowMain.appendChild(makeBtn('save', () => void finishWith('save')))
  rowMain.appendChild(makeBtn('pin', () => void finishWith('pin')))
  rowMain.appendChild(makeBtn('close', () => window.api.cancel()))

  COLORS.forEach((c) => {
    const d = document.createElement('div')
    d.className = 'dot'
    d.style.background = c
    d.addEventListener('click', () => setColor(c))
    dotEls.push(d)
    rowOpts.appendChild(d)
  })
  buildCustomColor()
  colorGroup = [...dotEls, customDot, pickerBtn]
  const gap = document.createElement('div')
  gap.className = 'gap'
  rowOpts.appendChild(gap)
  colorGroup.push(gap)
  ;[6, 10, 14].forEach((px, i) => {
    const el = document.createElement('div')
    el.className = 'sz'
    el.innerHTML = `<i style="width:${px}px;height:${px}px"></i>`
    el.addEventListener('click', () => setSize(i))
    szEls.push(el)
    rowOpts.appendChild(el)
  })
}

// ───────────────────────── Свой цвет ─────────────────────────
// Палитра рисуется прямо в оверлее: системный диалог цвета оказался бы под окном «поверх всего»

const CUSTOM_KEY = 'customColor'
const HEX_RE = /^#[0-9a-f]{6}$/
let customColor: string | null = loadCustomColor()
const customDot = document.createElement('div') // выбранный свой цвет, виден когда он есть
const pickerBtn = document.createElement('div') // «+»: всегда открывает палитру
const palette = document.createElement('div')
palette.hidden = true
let hexInput: HTMLInputElement

function loadCustomColor(): string | null {
  try {
    const v = localStorage.getItem(CUSTOM_KEY)
    return v && HEX_RE.test(v) ? v : null
  } catch {
    return null
  }
}

function saveCustomColor(c: string): void {
  try {
    localStorage.setItem(CUSTOM_KEY, c)
  } catch {
    // не сохранилось — цвет останется только до закрытия оверлея
  }
}

function hslHex(h: number, s: number, l: number): string {
  const a = (s / 100) * Math.min(l / 100, 1 - l / 100)
  const f = (n: number): string => {
    const k = (n + h / 30) % 12
    const v = l / 100 - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))
    return Math.round(v * 255).toString(16).padStart(2, '0')
  }
  return `#${f(0)}${f(8)}${f(4)}`
}

/** Оттенки по кругу × несколько светлот, плюс ряд серых. */
function paletteColors(): string[][] {
  const hues = [0, 20, 40, 55, 80, 130, 165, 190, 210, 240, 270, 300, 330]
  const rows = [85, 70, 55, 42, 30].map((l) => hues.map((h) => hslHex(h, 85, l)))
  rows.push(hues.map((_, i) => hslHex(0, 0, 100 - Math.round((i * 100) / (hues.length - 1)))))
  return rows
}

function paintCustomDot(): void {
  customDot.hidden = customColor === null
  customDot.style.background = customColor ?? ''
}

function buildCustomColor(): void {
  customDot.className = 'dot'
  customDot.title = labels['customColor'] ?? ''
  paintCustomDot()
  customDot.addEventListener('click', () => {
    closePalette()
    if (customColor) setColor(customColor)
  })
  rowOpts.appendChild(customDot)

  pickerBtn.className = 'dot picker'
  pickerBtn.title = labels['pickColor'] ?? ''
  pickerBtn.innerHTML = icon('plus', 10, { stroke: 3.2 })
  pickerBtn.addEventListener('click', togglePalette)
  rowOpts.appendChild(pickerBtn)

  palette.className = 'palette'
  palette.hidden = true
  const grid = document.createElement('div')
  grid.className = 'pal-grid'
  for (const row of paletteColors()) {
    for (const c of row) {
      const cell = document.createElement('div')
      cell.className = 'pal-cell'
      cell.style.background = c
      cell.title = c
      cell.addEventListener('click', () => pickCustom(c, true))
      grid.appendChild(cell)
    }
  }
  palette.appendChild(grid)

  hexInput = document.createElement('input')
  hexInput.className = 'pal-hex'
  hexInput.maxLength = 7
  hexInput.spellcheck = false
  hexInput.placeholder = '#rrggbb'
  hexInput.addEventListener('keydown', (ev) => {
    ev.stopPropagation() // буквы не должны переключать инструменты
    if (ev.key === 'Enter') {
      ev.preventDefault()
      const c = normalizeHex(hexInput.value)
      if (c) pickCustom(c, true)
    } else if (ev.key === 'Escape') {
      ev.preventDefault()
      closePalette()
    }
  })
  hexInput.addEventListener('input', () => {
    // На лету применяем только полный #rrggbb, короткий #rgb — по Enter
    const c = normalizeHex(hexInput.value)
    const full = c !== null && hexInput.value.replace('#', '').trim().length === 6
    hexInput.classList.toggle('bad', c === null && hexInput.value.trim() !== '')
    if (full) pickCustom(c, false)
  })
  palette.appendChild(hexInput)
  bar.appendChild(palette)
}

function normalizeHex(raw: string): string | null {
  let v = raw.trim().toLowerCase()
  if (!v.startsWith('#')) v = `#${v}`
  if (/^#[0-9a-f]{3}$/.test(v)) v = `#${v[1]}${v[1]}${v[2]}${v[2]}${v[3]}${v[3]}`
  return HEX_RE.test(v) ? v : null
}

function pickCustom(c: string, close: boolean): void {
  customColor = c
  saveCustomColor(c)
  paintCustomDot()
  setColor(c)
  if (close) closePalette()
  else syncUI()
}

function togglePalette(): void {
  if (palette.hidden) openPalette()
  else closePalette()
}

function openPalette(): void {
  commitText()
  palette.hidden = false
  pickerBtn.classList.add('open')
  hexInput.value = customColor ?? ''
  hexInput.classList.remove('bad')
  // Под панелью, а если не помещается — над ней
  palette.classList.remove('up')
  const br = bar.getBoundingClientRect()
  if (br.bottom + palette.offsetHeight + 8 > window.innerHeight) palette.classList.add('up')
  window.api.setEscape(false) // Esc закрывает палитру, а не весь оверлей
}

function closePalette(): void {
  if (palette.hidden) return
  palette.hidden = true
  pickerBtn.classList.remove('open')
  if (document.activeElement === hexInput) hexInput.blur()
  window.api.setEscape(true)
}

function selectedItem(): Ann | undefined {
  return selectedId === null ? undefined : items.find((a) => a.id === selectedId)
}

function syncUI(): void {
  if (appMode === 'record') {
    buttons.get('mic')?.classList.toggle('active', rec.mic)
    buttons.get('sys')?.classList.toggle('active', rec.sys)
    rowOpts.hidden = true
    render()
    return
  }
  for (const tl of TOOLS) buttons.get(tl)?.classList.toggle('active', tl === tool)
  ;(buttons.get('undo') as HTMLButtonElement).disabled = hIndex === 0
  ;(buttons.get('redo') as HTMLButtonElement).disabled = hIndex >= history.length - 1

  const s = selectedItem()
  const target: DrawTool | null = tool !== 'select' ? tool : s && s.type !== 'image' ? s.type : null
  rowOpts.hidden = target === null
  if (target === null) closePalette()
  if (target !== null) {
    const color = s && tool === 'select' ? s.color : toolColor[target]
    const size = s && tool === 'select' ? s.s : toolSize[target]
    dotEls.forEach((d, i) => d.classList.toggle('on', COLORS[i] === color))
    customDot.classList.toggle('on', customColor === color && !COLORS.includes(color))
    pickerBtn.classList.toggle('open', !palette.hidden)
    if (target === 'blur') closePalette()
    szEls.forEach((el, i) => el.classList.toggle('on', i === size))
    for (const el of colorGroup) el.hidden = target === 'blur' // у пикселизации нет цвета
    if (customColor === null) customDot.hidden = true
  }
  document.body.classList.toggle('tool-draw', tool !== 'select' && tool !== 'text')
  document.body.classList.toggle('tool-text', tool === 'text')
  render()
}

function setTool(next: Tool): void {
  commitText()
  tool = next
  if (next !== 'select') selectedId = null
  syncUI()
  schedulePaint()
}

function setColor(c: string): void {
  const s = selectedItem()
  if (s && tool === 'select') {
    items = items.map((a) => (a.id === s.id ? { ...a, color: c } : a))
    commit()
    schedulePaint()
  } else if (tool !== 'select') {
    toolColor[tool] = c
    syncUI()
  }
}

function setSize(i: number): void {
  const s = selectedItem()
  if (s && tool === 'select') {
    items = items.map((a) => (a.id === s.id ? { ...a, s: i } : a))
    commit()
    schedulePaint()
  } else if (tool !== 'select') {
    toolSize[tool] = i
    syncUI()
  }
}

// ───────────────────────── Выделение области ─────────────────────────

for (const h of ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']) {
  const el = document.createElement('div')
  el.className = `h h-${h}`
  el.dataset.h = h
  sel.appendChild(el)
}

type Mode =
  | { kind: 'none' }
  | { kind: 'create'; sx: number; sy: number }
  | { kind: 'move'; sx: number; sy: number; start: Rect }
  | { kind: 'resize'; handle: string; sx: number; sy: number; start: Rect }
  | { kind: 'draw'; sx: number; sy: number }
  | { kind: 'annMove'; sx: number; sy: number; orig: Ann; editOnClick?: boolean }
  | { kind: 'imgResize'; sx: number; sy: number; orig: ImageA }

let mode: Mode = { kind: 'none' }
let dragging = false

const contains = (r: Rect, x: number, y: number): boolean =>
  x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height

function render(): void {
  if (!rect) {
    sel.hidden = true
    bar.hidden = true
    dim.hidden = false
    return
  }
  dim.hidden = true
  sel.hidden = false
  sel.style.left = `${rect.x}px`
  sel.style.top = `${rect.y}px`
  sel.style.width = `${rect.width}px`
  sel.style.height = `${rect.height}px`

  // Размер показываем в физических пикселях: именно такой будет файл
  const k = live.kx || 1
  sizeLabel.textContent = `${Math.round(rect.width * k)} × ${Math.round(rect.height * k)}`
  sizeLabel.classList.toggle('inside', rect.y < 30)

  bar.hidden = dragging
  if (!dragging) placeBar(rect)
}

function placeBar(r: Rect): void {
  const bw = bar.offsetWidth
  const bh = bar.offsetHeight
  const gap = 8
  let top = r.y + r.height + gap
  if (top + bh > window.innerHeight - 4) top = r.y - bh - gap // не влезло снизу: над выделением
  if (top < 4) top = r.y + r.height - bh - gap // не влезло и сверху: внутрь
  bar.style.top = `${clamp(top, 4, window.innerHeight - bh - 4)}px`
  bar.style.left = `${clamp(r.x + r.width - bw, 4, window.innerWidth - bw - 4)}px`
}

// ───────────────────────── Итоговый снимок ─────────────────────────

/**
 * Вставляет картинку поверх снимка. Ctrl+V берёт её из буфера обмена, кнопка открывает выбор файла.
 * Картинка ставится по центру выделения в натуральном размере (но не больше 70% выделения).
 */
async function insertImage(fromFile: boolean): Promise<void> {
  commitText()
  if (appMode !== 'screenshot' || !rect) return
  const bytes = fromFile ? await window.api.pickImageFile() : await window.api.readClipboardImage()
  if (!bytes) return
  let bmp: ImageBitmap
  try {
    bmp = await createImageBitmap(new Blob([new Uint8Array(bytes)]))
  } catch {
    return // файл не картинка
  }
  const r = rect
  const id = ++imgSeq
  images.set(id, bmp)
  let w = bmp.width / live.kx // 1:1 в физических пикселях
  let h = bmp.height / live.ky
  const fit = Math.min(1, (r.width * 0.7) / w, (r.height * 0.7) / h)
  w = Math.max(16, w * fit)
  h = Math.max(16, h * fit)
  const item: ImageA = { id: ++seq, type: 'image', color: '#000000', s: 1, img: id, x: r.x + (r.width - w) / 2, y: r.y + (r.height - h) / 2, w, h }
  items.push(item)
  tool = 'select' // сразу можно перетаскивать и менять размер
  selectedId = item.id
  commit()
  schedulePaint()
}

function startRecord(): void {
  if (rect) window.api.recordStart({ ...rect }, { mic: rec.mic, systemAudio: rec.sys })
}

async function finishWith(type: OverlayAction): Promise<void> {
  if (appMode === 'record') {
    startRecord() // Enter и двойной клик в режиме записи начинают запись
    return
  }
  commitText()
  if (!rect || busy) return
  busy = true
  try {
    const r = { ...rect }
    if (items.length === 0) {
      window.api.action(type, r) // быстрый путь: аннотаций нет
      return
    }
    const crop = await window.api.getCrop(r)
    if (!crop) {
      window.api.cancel()
      return
    }
    const bmp = await createImageBitmap(new Blob([new Uint8Array(crop.png)], { type: 'image/png' }))
    const out = document.createElement('canvas')
    out.width = bmp.width
    out.height = bmp.height
    const octx = out.getContext('2d') as CanvasRenderingContext2D
    octx.drawImage(bmp, 0, 0)
    drawAll(octx, items, { kx: crop.kx, ky: crop.ky, tx: -crop.left, ty: -crop.top }, null)
    const blob = await new Promise<Blob | null>((res) => out.toBlob(res, 'image/png'))
    if (!blob) {
      window.api.cancel()
      return
    }
    window.api.submit(type, r, new Uint8Array(await blob.arrayBuffer()))
  } catch (e) {
    console.error(e)
    window.api.cancel()
  } finally {
    busy = false
  }
}

// ───────────────────────── Текст ─────────────────────────

interface Editing { x: number; y: number; color: string; s: number; orig?: TextA; el: HTMLTextAreaElement }
let editing: Editing | null = null

function startTextEdit(x: number, y: number, orig?: TextA): void {
  const color = orig?.color ?? toolColor.text
  const s = orig?.s ?? toolSize.text
  if (orig) {
    items = items.filter((a) => a.id !== orig.id)
    selectedId = null
    schedulePaint()
  }
  const el = document.createElement('textarea')
  el.className = 'txt'
  el.rows = 1
  el.spellcheck = false
  el.value = orig?.text ?? ''
  el.style.left = `${x}px`
  el.style.top = `${y}px`
  el.style.color = color
  el.style.font = `600 ${TEXT_PX[s]}px/1.25 ${FONT}`
  el.style.setProperty('field-sizing', 'content')
  el.addEventListener('keydown', (ev) => {
    ev.stopPropagation()
    if (ev.key === 'Enter' && !ev.shiftKey) {
      ev.preventDefault()
      commitText()
    } else if (ev.key === 'Escape') {
      ev.preventDefault()
      cancelText()
    }
  })
  el.addEventListener('blur', () => commitText())
  document.body.appendChild(el)
  editing = { x, y, color, s, orig, el }
  window.api.setEscape(false) // иначе глобальный Esc закроет весь оверлей
  el.focus()
}

function commitText(): void {
  if (!editing) return
  const e = editing
  editing = null
  const text = e.el.value.replace(/\s+$/, '')
  e.el.remove()
  window.api.setEscape(true)
  if (text.trim()) {
    items.push({ id: e.orig?.id ?? ++seq, type: 'text', x: e.x, y: e.y, text, color: e.color, s: e.s })
    commit()
  } else if (e.orig) {
    commit() // пустой текст у существующей подписи = удаление
  }
  schedulePaint()
}

function cancelText(): void {
  if (!editing) return
  const e = editing
  editing = null
  e.el.remove()
  window.api.setEscape(true)
  if (e.orig) items.push(e.orig)
  schedulePaint()
}

// ───────────────────────── Мышь ─────────────────────────

function pick(x: number, y: number): Ann | undefined {
  for (let i = items.length - 1; i >= 0; i--) if (hit(items[i], x, y)) return items[i]
  return undefined
}

/** Начинает рисование выбранным инструментом. Возвращает true, если нужен drag. */
function beginDraw(x: number, y: number): boolean {
  const base = { id: ++seq, color: toolColor[tool as DrawTool], s: toolSize[tool as DrawTool] }
  switch (tool as DrawTool) {
    case 'text':
      startTextEdit(x, y)
      return false
    case 'number': {
      const n = items.reduce((m, a) => (a.type === 'number' ? Math.max(m, a.n) : m), 0) + 1
      items.push({ ...base, type: 'number', x, y, n })
      commit()
      schedulePaint()
      return false
    }
    case 'arrow':
      draft = { ...base, type: 'arrow', x1: x, y1: y, x2: x, y2: y }
      break
    case 'line':
      draft = { ...base, type: 'line', x1: x, y1: y, x2: x, y2: y }
      break
    case 'rect':
      draft = { ...base, type: 'rect', x, y, w: 0, h: 0 }
      break
    case 'blur':
      draft = { ...base, type: 'blur', x, y, w: 0, h: 0 }
      break
    case 'marker':
      draft = { ...base, type: 'marker', pts: [[x, y]] }
      break
    case 'pen':
      draft = { ...base, type: 'pen', pts: [[x, y]] }
      break
  }
  return true
}

function updateDraft(d: Ann, sx: number, sy: number, cx: number, cy: number, shift: boolean): void {
  switch (d.type) {
    case 'arrow':
    case 'line': {
      let x2 = cx
      let y2 = cy
      if (shift) {
        // Shift: угол кратен 45°
        const len = Math.hypot(cx - sx, cy - sy)
        const ang = Math.round(Math.atan2(cy - sy, cx - sx) / (Math.PI / 4)) * (Math.PI / 4)
        x2 = sx + Math.cos(ang) * len
        y2 = sy + Math.sin(ang) * len
      }
      d.x2 = x2
      d.y2 = y2
      break
    }
    case 'rect':
    case 'blur': {
      let w = cx - sx
      let h = cy - sy
      if (shift && d.type === 'rect') {
        const m = Math.max(Math.abs(w), Math.abs(h))
        w = Math.sign(w || 1) * m
        h = Math.sign(h || 1) * m
      }
      d.x = Math.min(sx, sx + w)
      d.y = Math.min(sy, sy + h)
      d.w = Math.abs(w)
      d.h = Math.abs(h)
      break
    }
    case 'marker':
    case 'pen': {
      const last = d.pts[d.pts.length - 1]
      if (Math.hypot(cx - last[0], cy - last[1]) > 1.5) d.pts.push([cx, cy])
      break
    }
  }
}

function draftValid(d: Ann): boolean {
  switch (d.type) {
    case 'arrow':
    case 'line':
      return Math.hypot(d.x2 - d.x1, d.y2 - d.y1) >= 6
    case 'rect':
    case 'blur':
      return d.w >= MIN && d.h >= MIN
    default:
      return true
  }
}

document.addEventListener('mousedown', (e) => {
  if (e.button !== 0) return
  const target = e.target as HTMLElement
  if (!target.closest('.palette') && !pickerBtn.contains(target)) closePalette()
  if (target.closest('#bar')) return
  if (editing) {
    if (target !== editing.el) commitText() // первый клик вне редактора только завершает текст
    return
  }

  const x = e.clientX
  const y = e.clientY
  const handle = target.dataset.h

  if (handle && rect) {
    mode = { kind: 'resize', handle, sx: x, sy: y, start: { ...rect } }
  } else if (rect && contains(rect, x, y)) {
    if (tool === 'select') {
      const cur = selectedItem()
      const onHandle = !!cur && cur.type === 'image' && Math.hypot(x - (cur.x + cur.w), y - (cur.y + cur.h)) <= 12
      const a = onHandle ? undefined : pick(x, y)
      if (onHandle && cur?.type === 'image') {
        mode = { kind: 'imgResize', sx: x, sy: y, orig: JSON.parse(JSON.stringify(cur)) as ImageA }
      } else if (a) {
        selectedId = a.id
        mode = { kind: 'annMove', sx: x, sy: y, orig: JSON.parse(JSON.stringify(a)) as Ann }
      } else {
        selectedId = null
        mode = { kind: 'move', sx: x, sy: y, start: { ...rect } }
      }
      syncUI()
      schedulePaint()
    } else if (tool === 'text' && pick(x, y)?.type === 'text') {
      // Инструмент «Текст» поверх готовой надписи: тянем — двигаем, просто клик — редактируем
      const a = pick(x, y) as TextA
      selectedId = a.id
      mode = { kind: 'annMove', sx: x, sy: y, orig: { ...a }, editOnClick: true }
      schedulePaint()
    } else {
      if (!beginDraw(x, y)) {
        e.preventDefault() // иначе браузер переведёт фокус на клик и поле текста сразу закроется пустым
        return
      }
      mode = { kind: 'draw', sx: x, sy: y }
    }
  } else {
    mode = { kind: 'create', sx: x, sy: y }
    rect = null
    selectedId = null
    syncUI()
    schedulePaint()
  }
  dragging = true
  render()
})

document.addEventListener('mousemove', (e) => {
  const m = mode
  if (m.kind === 'none') {
    const over = tool === 'text' && !editing && pick(e.clientX, e.clientY)?.type === 'text'
    document.body.classList.toggle('over-text', over)
    return
  }
  const W = window.innerWidth
  const H = window.innerHeight

  switch (m.kind) {
    case 'create': {
      const cx = clamp(e.clientX, 0, W)
      const cy = clamp(e.clientY, 0, H)
      const sx = clamp(m.sx, 0, W)
      const sy = clamp(m.sy, 0, H)
      rect = { x: Math.min(sx, cx), y: Math.min(sy, cy), width: Math.abs(cx - sx), height: Math.abs(cy - sy) }
      break
    }
    case 'move': {
      const s = m.start
      rect = {
        ...s,
        x: clamp(s.x + e.clientX - m.sx, 0, W - s.width),
        y: clamp(s.y + e.clientY - m.sy, 0, H - s.height)
      }
      break
    }
    case 'resize': {
      const s = m.start
      const dx = e.clientX - m.sx
      const dy = e.clientY - m.sy
      let l = s.x
      let r = s.x + s.width
      let t = s.y
      let b = s.y + s.height
      if (m.handle.includes('w')) l = clamp(s.x + dx, 0, r - MIN)
      if (m.handle.includes('e')) r = clamp(s.x + s.width + dx, l + MIN, W)
      if (m.handle.includes('n')) t = clamp(s.y + dy, 0, b - MIN)
      if (m.handle.includes('s')) b = clamp(s.y + s.height + dy, t + MIN, H)
      rect = { x: l, y: t, width: r - l, height: b - t }
      break
    }
    case 'draw':
      if (draft) {
        updateDraft(draft, m.sx, m.sy, clamp(e.clientX, 0, W), clamp(e.clientY, 0, H), e.shiftKey)
        schedulePaint()
      }
      break
    case 'imgResize': {
      const o = m.orig
      const w = Math.max(16, o.w + (e.clientX - m.sx))
      items = items.map((a) => (a.id === o.id ? { ...o, w, h: (o.h * w) / o.w } : a))
      schedulePaint()
      break
    }
    case 'annMove': {
      const dx = e.clientX - m.sx
      const dy = e.clientY - m.sy
      items = items.map((a) => (a.id === m.orig.id ? moved(m.orig, dx, dy) : a))
      schedulePaint()
      break
    }
  }
  render()
})

document.addEventListener('mouseup', () => {
  const m = mode
  if (m.kind === 'none') return

  if (m.kind === 'draw' && draft) {
    if (draftValid(draft)) {
      items.push(draft)
      commit()
    }
    draft = null
    schedulePaint()
  } else if (m.kind === 'annMove' || m.kind === 'imgResize') {
    const cur = items.find((a) => a.id === m.orig.id)
    const changed = !!cur && JSON.stringify(cur) !== JSON.stringify(m.orig)
    if (changed) commit()
    if (m.kind === 'annMove' && m.editOnClick) {
      selectedId = null
      if (!changed && cur?.type === 'text') startTextEdit(cur.x, cur.y, cur)
      schedulePaint()
    }
  } else if (rect && (rect.width < MIN || rect.height < MIN)) {
    rect = null // простой клик без протяжки не создаёт выделение
  }

  mode = { kind: 'none' }
  dragging = false
  render()
})

document.addEventListener('dblclick', (e) => {
  if (editing || !rect || tool !== 'select' || !contains(rect, e.clientX, e.clientY)) return
  const a = pick(e.clientX, e.clientY)
  if (a?.type === 'text') startTextEdit(a.x, a.y, a)
  else if (!a) void finishWith('copy')
})

// Правая кнопка мыши отменяет снимок
document.addEventListener('contextmenu', (e) => {
  e.preventDefault()
  if (!editing) window.api.cancel()
})

// ───────────────────────── Клавиатура ─────────────────────────

const HOTKEYS: Record<string, Tool> = { v: 'select', p: 'pen', a: 'arrow', l: 'line', r: 'rect', m: 'marker', t: 'text', b: 'blur', n: 'number' }

document.addEventListener('keydown', (e) => {
  if (editing) return
  const mod = e.ctrlKey || e.metaKey
  // Буква совпадает и по символу (латиница, Dvorak), и по физической клавише (русская раскладка)
  const is = (letter: string): boolean => e.key.toLowerCase() === letter || e.code === `Key${letter.toUpperCase()}`
  const toolKey = Object.keys(HOTKEYS).find(is)

  if (e.key === 'Escape') {
    if (!palette.hidden) closePalette()
    else window.api.cancel()
  } else if (e.key === 'Enter' || (appMode === 'screenshot' && mod && is('c'))) {
    e.preventDefault()
    void finishWith('copy')
  } else if (appMode === 'screenshot' && mod && is('s')) {
    e.preventDefault()
    void finishWith('save')
  } else if (appMode === 'screenshot' && mod && is('z')) {
    e.preventDefault()
    if (e.shiftKey) redo()
    else undo()
  } else if (appMode === 'screenshot' && mod && is('y')) {
    e.preventDefault()
    redo()
  } else if (appMode === 'screenshot' && mod && is('v')) {
    e.preventDefault()
    void insertImage(false)
  } else if (mod && is('a')) {
    e.preventDefault()
    rect = { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight }
    render()
  } else if ((e.key === 'Delete' || e.key === 'Backspace') && selectedId !== null) {
    e.preventDefault()
    items = items.filter((a) => a.id !== selectedId)
    selectedId = null
    commit()
    schedulePaint()
  } else if (appMode === 'screenshot' && !mod && !e.altKey && toolKey) {
    setTool(HOTKEYS[toolKey])
  } else if (rect && e.key.startsWith('Arrow')) {
    // Точная подгонка: стрелки двигают выделение (или выбранную аннотацию) на 1 px, с Shift на 10
    e.preventDefault()
    const step = e.shiftKey ? 10 : 1
    const dx = e.key === 'ArrowLeft' ? -step : e.key === 'ArrowRight' ? step : 0
    const dy = e.key === 'ArrowUp' ? -step : e.key === 'ArrowDown' ? step : 0
    const s = selectedItem()
    if (s) {
      items = items.map((a) => (a.id === s.id ? moved(a, dx, dy) : a))
      commit()
      schedulePaint()
    } else {
      rect = {
        ...rect,
        x: clamp(rect.x + dx, 0, window.innerWidth - rect.width),
        y: clamp(rect.y + dy, 0, window.innerHeight - rect.height)
      }
      render()
    }
  }
})

// ───────────────────────── Запуск ─────────────────────────

async function start(data: OverlayInit): Promise<void> {
  labels = data.labels
  appMode = data.mode
  rec.mic = data.rec.mic
  rec.sys = data.rec.systemAudio
  rec.gif = data.rec.format === 'gif'
  buildBar()
  for (const [id, b] of buttons) b.title = labels[id] ?? ''

  bgImg = data.frame
  cv.width = bgImg.width
  cv.height = bgImg.height
  live = {
    kx: bgImg.width / window.innerWidth,
    ky: bgImg.height / window.innerHeight,
    tx: 0,
    ty: 0
  }
  cctx.drawImage(bgImg, 0, 0)
  syncUI()
  devLog('нарисован')
  // Два кадра: первый rAF — до отрисовки, ко второму кадр уже выведен в видимое окно
  window.api.onReveal(() => requestAnimationFrame(() => requestAnimationFrame(() => window.api.revealed())))
  window.api.shown() // main покажет окно только когда кадр уже отрисован
}

// Страница может быть загружена заранее и ждать кадра: он придёт событием
window.api.onInit((data) => void start(data).catch(() => window.api.cancel()))
window.api.ready()
