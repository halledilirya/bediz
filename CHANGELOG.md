# Changelog

All notable changes to Bediz are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
