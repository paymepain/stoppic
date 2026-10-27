# StopPic

[Русский](README.md) | **English**

A free app for screenshots and screen recording. Select an area, add a label or an arrow, and the picture is already on your clipboard. You can also record your screen to MP4 or GIF, with sound.

It's lightweight: the installer is just a few megabytes, and the app sits quietly in the tray using very little memory. It never sends anything to the internet.

## Installation

1. Go to the [Releases](https://github.com/paymepain/stoppic/releases/latest) page and download `StopPic_…_x64-setup.exe`.
2. Run it and follow the installer steps.

Requires Windows 10 or 11. If the Microsoft Edge WebView2 component is missing (it's usually already there), the installer downloads it automatically.

Windows may show a "Windows protected your PC" warning. This happens with new apps that don't have a paid digital signature. Click "More info" → "Run anyway".

## Screenshots

Press `Ctrl+Alt+A` and the screen freezes. Select the area you need with the mouse. The selection can be moved and resized by its edges.

- `Enter`, `Ctrl+C` or double-click: copy to clipboard
- `Ctrl+S`: save to a file
- `Esc` or right mouse button: cancel
- `Ctrl+A`: select the whole monitor; arrow keys move the selection by 1 px, with `Shift` by 10 px

Files are saved to `Pictures\Screenshots`. Clicking the notification opens the folder with the file.

### Annotations

A toolbar appears below the selection:

| Tool | Key |
|---|---|
| Select and move | `V` |
| Pen | `P` |
| Arrow | `A` |
| Rectangle | `R` |
| Highlighter | `M` |
| Text | `T` |
| Blur (to hide a password, a face, etc.) | `B` |
| Numbering 1, 2, 3… | `N` |

- Color and size are remembered separately for each tool
- Hold `Shift` to snap arrows to multiples of 45° and draw rectangles as squares
- With the Select tool you can move, recolor or delete (`Delete`) any annotation. Double-click text to edit it
- `Ctrl+V` pastes an image from the clipboard, the toolbar button inserts one from a file
- `Ctrl+Z` to undo, `Ctrl+Y` to redo

**Pin:** keeps the screenshot as a small window on top of all others. Handy when you need something in front of you. Close it with the cross or `Esc`.

## Screen recording

1. Press `Ctrl+Alt+R` and select an area (`Ctrl+A` for the whole screen).
2. Turn on the microphone and computer sound if you need them.
3. Press the red button or `Enter`.

After a 3-2-1 countdown a small panel appears with a timer, pause and stop. You can drag the panel around, and it won't appear in the recording. You can also stop with the same shortcut. The tray icon turns red while recording.

Format (MP4 or GIF) and frame rate (15, 30 or 60) are chosen in settings.

## Settings

Open them from the tray menu ("Settings…"). Changes apply immediately.

- **Hotkeys:** click a field and press a new shortcut. If another app already uses it, StopPic will tell you. `↺` restores the default
- **Saving:** screenshots folder and "Save a file when copying"
- **Screen recording:** format, frame rate, default microphone and system sound, countdown before start
- **General:** launch at login, fast capture start, language

"Fast capture start" keeps the app ready so screenshots appear instantly, at the cost of a bit more memory. If memory matters more, turn it off.

The light or dark theme follows your Windows setting.

## Privacy

StopPic sends nothing over the network. Screenshots, recordings and settings stay on your computer only.

## License

StopPic is free, and its source code is open under the [MIT](LICENSE.txt) license.
## Support the author

If you find the app useful, give the repository a ⭐ on GitHub: it helps others find it. And if you'd like to support development financially, you can use any of the options below.

### DonationAlerts

<a href="https://www.donationalerts.com/r/paymepain"><img src="docs/qr/donationalerts.svg" width="200" alt="DonationAlerts QR code"></a>

https://www.donationalerts.com/r/paymepain

---

### Telegram (TON)

<img src="docs/qr/ton.svg" width="200" alt="TON QR code">

`UQBRPrNCafN_d41-4xq9FmCvPx0V94t6la3dY7G0l5_jOvRD`

---

### TronLink (TRON)

<img src="docs/qr/tron.svg" width="200" alt="TRON QR code">

`TG3nnaoCSwct8YCZa8ciYHK3QUe7QETJiE`
