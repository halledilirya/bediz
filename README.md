# Bediz

[![CI](https://github.com/halledilirya/bediz/actions/workflows/ci.yml/badge.svg)](https://github.com/halledilirya/bediz/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Bediz is a screenshot and GIF recording app for Windows 11. It waits in the tray: press Print Screen, select a part of the screen, mark it up, then copy and save it, or record it as a GIF.

Bediz is in early development.

## Download

Download `Bediz_<version>_x64-setup.exe` from the [latest release](https://github.com/halledilirya/bediz/releases/latest) and run it. It installs for your Windows user only, without administrator rights. Later versions install from **Help ▸ Check for updates** in the tray menu.

The setup file is not code-signed yet, so Windows SmartScreen may say that it protected your PC: choose **More info**, then **Run anyway**. With Smart App Control on, Windows may refuse to run it at all.

To check that the file is the one this repository's release workflow built, compare its SHA-256 hash (`Get-FileHash` in PowerShell) with `SHA256SUMS.txt` in the release, or verify its build attestation with the [GitHub CLI](https://cli.github.com):

```
gh attestation verify Bediz_<version>_x64-setup.exe --repo halledilirya/bediz
```

To build Bediz yourself, see [CONTRIBUTING.md](CONTRIBUTING.md); [ARCHITECTURE.md](ARCHITECTURE.md) describes how the code is organized.

## Usage

Press **Print Screen** or click the tray icon. The screen freezes on every monitor; with HDR on, it keeps the colours you see instead of coming out washed out. Drag to select a region, which can span monitors, then press **Enter** to copy it and save it to Pictures\Screenshots as a PNG. If copying or saving fails, a short label says why. A file that exists is never replaced; the new one gets a number. Save as replaces a file only after Windows asks and you agree.

To capture a menu or a tooltip that closes when a key is pressed, choose **Delayed screenshot** in the tray menu, then open it before the seconds in the corner run out.

Bediz starts with Windows unless you turn that off in the settings.

| Key | Action |
| --- | --- |
| Drag | Select a region; drag its edges or corners to resize it, its inside to move it |
| Shift while dragging | Keep a square |
| Alt while dragging | Grow a new selection from its centre |
| Arrow keys | Move the selection by one pixel, ten with Shift |
| Ctrl + arrow keys | Resize the selection from its right or bottom edge |
| Ctrl+A | Select the whole monitor under the cursor, or every word while text is highlighted |
| R | Select the region copied or saved last |
| C | Copy the colour under the cursor as #RRGGBB |
| Enter or a double-click | Copy and save the selection, or what the settings say |
| Ctrl+C | Only copy the selection, or the words chosen while text is highlighted |
| Ctrl+S | Only save the selection |
| Ctrl+Shift+S | Save the selection where you choose |
| O | Copy the text in the selection and highlight its words, or stop highlighting them |
| Right-click, the menu key or Shift+F10 | Open a menu with these actions |
| Esc | Cancel |

To mark up a screenshot, pick a tool in the bar beside the selection or press its key, then draw inside the selection. The mark just drawn stays selected: drag it or its corners, recolour it, change its width or delete it, until a click elsewhere begins the next one. With no tool chosen (pick the same tool or press its key again), the selection can be moved and resized, and a click on any mark selects it. Marks lie on each other in the order they are drawn, and are copied and saved with the screenshot.

To copy the text in a selection, press **O** or pick **Copy text** in the bar or the right-click menu. Windows reads it on your computer, in the first of your Windows languages it can read or the one chosen in the settings, and the words it found are highlighted while the selection stays open. Drag across them or click one to choose it, then press Ctrl+C to copy only those; the selection stays open for more. **Join lines** and **Keep indentation** are in the right-click menu and the settings. Esc, O or a tool's key goes back to marking up. Text under a black box is not read. When no language that reads text is installed, add one in Settings › Time & language › Language & region.

| Key | Tool or action |
| --- | --- |
| P | Pen |
| H | Highlighter |
| L | Line; Shift keeps it straight or at 45° |
| A | Arrow; Shift keeps it straight or at 45° |
| B | Rectangle; Shift keeps a square |
| E | Ellipse; Shift keeps a circle |
| T | Text; Enter ends it, Shift+Enter starts a new line |
| N | Numbered step: 1, 2, 3… |
| X | Hide with an opaque black box |
| U | Blur |
| M | Pixelate |
| Mouse wheel, + or − | A thicker or thinner line, larger text and steps, a stronger blur or pixelation |
| Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z | Undo or redo |
| Delete | Delete the selected mark |
| Esc | Stop typing or highlighting text, then deselect the mark, then close |

Blur and pixelation only soften what is under them, and pixelated text can sometimes be read back; to hide a password or a name, use X, which keeps nothing of it.

To mark up an image you already have, choose **Open an image ▸ From the clipboard** or **From a file…** in the tray menu. The image opens on the monitor under the pointer, at its own size or made smaller to fit, and the same tools and keys work on it. It is copied and saved at its own size, to a new file beside your screenshots; the file you opened is left as it was.

To record a GIF, choose **Record ▸ GIF** in the tray menu or press the GIF shortcut, which you set in the settings; the selection then opens on the live screen. **Record a GIF** in the right-click menu records a selection made on a screenshot. Select a region on one monitor and press **Start**, or **Lock** it first to keep using the screen until you start. A frame marks the area and a bar shows the time with **Pause**, **Finish**, **Restart** and **Discard**; neither appears in the GIF. Pressed again, the GIF shortcut starts the recording, and once more, it finishes it. The GIF is saved with your screenshots and copied as a file, so it stays animated when you paste it into a chat, and a small window plays it and can copy, show, open or delete it.

**Settings** in the tray menu, or starting Bediz again while it runs, opens the settings: the shortcuts, what Enter and a double-click do, where files are saved and how they are named, keeping screenshots out of the Win+V clipboard history, the GIF's frame rate, size and length, the language and the look. About shows the version, the licence notices and the log folder.

If Print Screen opens the Snipping Tool, Bediz asks once as it starts whether it may turn that off; **Let Bediz use Print Screen** in the tray menu and the settings does the same. When another app holds the shortcut, the tray menu and the settings say so; the tray icon still takes a screenshot. Windows does not send Print Screen, or any other key alone, to Bediz while a window running as administrator is in front; a shortcut with Ctrl, Alt or Shift, such as Ctrl+Shift+F9, still arrives. Since Windows 11 24H2, Windows does not hand the frames of a game in exclusive full screen to screen capture, so the screenshot shows an older image; run such a game borderless or windowed.

Bediz follows the Windows display language unless you pick one. It speaks Arabic, Chinese (Simplified), English, French, German, Hindi, Italian, Japanese, Korean, Portuguese, Russian, Spanish and Turkish. It follows Windows' high contrast themes.

Bediz sends nothing anywhere: it has no telemetry and uploads nothing, and the screenshots it copies are kept out of the Windows cloud clipboard. It goes online only to ask GitHub for the latest version, when you check for updates or turn on checking at start (off by default); an update is installed only when you choose to. Saved files are like any other file in their folder: if OneDrive or another service syncs the folder, it uploads them too; the settings say when OneDrive syncs it.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
