# Changelog

All notable changes to Bediz are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Delayed screenshot** in the tray menu takes the screenshot 3, 5 or 10 seconds later, so a menu or a tooltip that closes when a key is pressed can be open when it is taken. The seconds left show in a corner and stay out of the screenshot; Print Screen takes it at once, and the tray menu can cancel it.
- **Copy text** copies the text in a selection, read on your computer by Windows' own text recognition: press O, or pick it in the bar beside the selection or the right-click menu. The words it read are highlighted and the selection stays open; marks count, so text under a black box is not read. Small text is enlarged before it is read, and tilted text is highlighted at its angle. When nothing is found, or Windows has no language installed that reads text, a label says so and the clipboard is left as it was.
- While the words are highlighted, drag across them or click one to choose it, and Ctrl+A chooses them all; Ctrl+C, or Copy in the bar, copies the words chosen, or all of them, and a label beside the selection says so. The selection stays open, so other words can be copied from the same screenshot, or the screenshot itself copied or saved. Esc, O or a tool's key goes back to marking up.
- **Join lines** copies the text as one line, and **Keep indentation** starts each line with as many spaces as it is indented on the screen, as code needs. Both are in the right-click menu while the words are highlighted and in the settings, under **Copying text**, and are remembered.

### Changed

- The tray menu is shorter: checking for updates, the log folder and About are under **Help**, and starting with Windows is set in the settings. A new version, once found, shows in the menu as **Update to version …**.
- While a GIF records, **Take a screenshot** in the tray menu is greyed out instead of finishing the recording; **Record ▸ Finish** finishes it.

## [0.3.1] - 2026-11-19

### Changed

- With more than one monitor, marking up redraws only the monitor the marks are on instead of every monitor.
- With many marks, drawing a new one is smoother: the marks drawn before it are kept as they are instead of being drawn again on every frame.
- Marks follow the pointer more closely while you draw: each frame is drawn from the pointer's newest position once the one before it is on the screen.

### Fixed

- Drawing on an opened image is as smooth as on a screenshot, and the live screen behind it, or behind a GIF selection, keeps moving while you draw, drag or resize with the mouse.
- Less graphics memory stays held after marking up or opening an image; part of it used to stay for minutes after the selection closed.

## [0.3.0] - 2026-11-18

### Added

- Marking up a screenshot before it is copied or saved: a pen, a highlighter, lines, arrows, rectangles, ellipses, text with emoji, numbered steps, an opaque black box that hides what is under it, and a blur and a pixelation that only soften it. Each has a key and a button in a bar beside the selection, with eight colours, a width set with the mouse wheel, undo and redo.
- Marks stay editable until the screenshot is copied or saved: select one to move it, drag its corners, change its colour or width, or delete it. Esc deselects it before it closes the screenshot.
- Text is typed as in any text box: the arrows, Home and End move the caret, Shift selects, Ctrl+A selects it all, Ctrl+Backspace and Ctrl+Delete erase a word, Ctrl+C, Ctrl+X and Ctrl+V copy, cut and paste, and a click puts the caret where it lands. Chinese, Japanese and Korean input methods write in the text itself, with their suggestions beneath it.
- Screen readers such as Narrator say the selection's size as it changes and the tool chosen, and read out the result of a copy or save from a Windows notification.
- Marking up an image from the clipboard or a file with the same tools, from **Open an image** in the tray menu; a large image is copied and saved at its own size.

### Changed

- A click beside a selection keeps it; drag there to start a new one.

## [0.2.0] - 2026-11-13

### Added

- GIF recording of a region on one monitor, started from the tray menu, a shortcut you set in the settings or the selection's right-click menu. A locked region leaves the screen usable until the recording starts.
- While it records, a frame marks the area and a bar shows the time with **Pause**, **Finish**, **Restart** and **Discard**; neither appears in the GIF. The recording stops by itself at its time limit, when the memory set aside for it is full, or when the screen changes or is locked, and says why.
- When a recording is done, the GIF is saved with your screenshots and copied as a file, so it stays animated when you paste it into a chat. A small window plays it, shows its size, length and file size, warns above 10 MB and 15 MB, and copies, shows, opens or deletes it.
- Interface recordings keep their common flat colours exact, which cuts the dither noise on backgrounds.
- Recording settings: frames per second (10 to 30), the largest size (640 to 1920 pixels, or the area's own size), the longest recording (15 seconds to 5 minutes), showing the pointer, a 3, 2, 1 countdown, and whether a finished GIF is copied and shown.
- Quitting while a GIF is still being saved asks first, with its progress: close once it is saved, delete it and close, or cancel.

## [0.1.1] - 2026-11-10

### Changed

- The settings window appears as soon as you open it, in the colours of its theme, and fills in once it is ready.

### Fixed

- The settings window no longer flashes white as it opens.
- The setup file and the uninstaller show Bediz's icon, and the files' details name the copyright holder.

## [0.1.0] - 2026-11-09

### Added

- Screenshots: press Print Screen or click the tray icon, select a region on any monitor or across monitors, then copy it, save it as a PNG or both. The selection can be resized and moved with the mouse or the keyboard; a magnifier shows the pixel and its colour.
- Copying puts a PNG and a bitmap on the clipboard, never on the cloud clipboard; keeping screenshots out of the Win+V history is a setting.
- Saving never replaces a file: a name that exists gets a number. Save as asks before replacing a file you picked.
- Screens with HDR on keep the colours you see.
- A right-click menu on the selection with every action and its key.
- Settings: the shortcut, what Enter does, the folder and the file name, starting with Windows, highlighting the window under the pointer, the "Copied" and "Saved" notices, releasing graphics memory while idle, language, look and accent colour, with a search.
- Updates: check from the tray or the settings, or at every start if you turn that on (off by default). An update is installed only when you choose to, and only when its signature and version match.
- Thirteen languages: Arabic (laid out right to left), Chinese (Simplified), English, French, German, Hindi, Italian, Japanese, Korean, Portuguese, Russian, Spanish and Turkish.
- High contrast themes are followed on the selection and in the settings.
