import './api'
import './pin.css'
import { icon } from './shared/icons'

const img = document.getElementById('img') as HTMLImageElement
const btnClose = document.getElementById('x') as HTMLButtonElement
btnClose.innerHTML = icon('close', 14, { stroke: 2.4 })

btnClose.addEventListener('click', () => window.api.pinClose())
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') window.api.pinClose()
})
// Окно без рамки: тащим за само изображение
document.getElementById('drag')?.addEventListener('mousedown', (e) => {
  if (e.button === 0) window.api.startDrag()
})

async function init(): Promise<void> {
  const png = await window.api.pinGetInit()
  if (!png) {
    window.api.pinClose()
    return
  }
  img.src = URL.createObjectURL(new Blob([new Uint8Array(png)], { type: 'image/png' }))
}

init().catch(() => window.api.pinClose())
