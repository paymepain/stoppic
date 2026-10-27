import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

// Рамка записываемой области: на паузе становится жёлтой, как точка на панели
const frame = document.getElementById('f') as HTMLElement
void getCurrentWebviewWindow().listen<boolean>('rec:paused', (e) => frame.classList.toggle('paused', e.payload))
