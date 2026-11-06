import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

// Frame around the recorded area: turns yellow on pause, like the dot on the panel
const frame = document.getElementById('f') as HTMLElement
void getCurrentWebviewWindow().listen<boolean>('rec:paused', (e) => frame.classList.toggle('paused', e.payload))
