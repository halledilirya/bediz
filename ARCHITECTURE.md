# Architecture

This file is for anyone about to change Bediz's code. It describes how the code is organized and the rules it keeps. It names files, modules and types instead of linking them, so use your editor's search to find them. Every Rust source file opens with a `//!` line saying what it is for, so this map stays at the level of modules.

## Bird's-eye view

Bediz is a tray app for Windows 11 that takes screenshots and records GIFs. It starts in the tray and waits for a hotkey.

1. **Start.** The app opens its log, loads the settings, makes the tray icon, starts the platform thread and starts the events thread. A second start does not run a second copy; it wakes the first, which opens its settings.
2. **Waiting.** The platform thread registers the hotkeys and keeps an overlay window for every monitor, with its GPU resources, ready before any key is pressed. Nothing runs on a timer while it waits.
3. **A screenshot.** The hotkey or the tray icon captures every monitor and shows the frozen image in the overlays. The user drags a selection and picks an action. The selection is read back from the GPU, encoded as PNG and saved on worker threads, and put on the clipboard. A short label says what happened.
4. **A recording.** The GIF hotkey or the tray menu opens the selection on the live screen, and a bar starts the recording; the overlay's menu can also record a selection. While it records, frames are captured on a timer, cut out and scaled on the GPU, read back, and handed to the GIF pipeline, which prepares them on worker threads and writes the GIF in order. When it ends, the GIF window shows the result unless the settings turn it off, and the GIF can be copied as a file.
5. **Settings.** The settings window is created when it is opened and destroyed when it is closed, so its WebView does not stay in memory.

```
 app/src-tauri                                  crates/platform
 tray, settings and GIF windows,   Command -->  platform thread: hotkeys, overlays,
 settings store, events thread     <-- Event    capture, recording, saving, clipboard
        |                                              |
 app/src: the pages in the WebViews            crates/core: the logic both use
```

## Code map

### The repository

- `crates/core`: the logic with no OS calls and no `unsafe`, so all of it can be unit tested. It produces no user-facing text, only typed values and locale ids.
- `crates/platform`: every OS call, one folder per OS; only `windows/` exists. The only crate with `unsafe`.
- `app/src-tauri`: the Tauri app: the tray, the windows, the settings store, the start of the platform thread and the updater.
- `app/src`: the pages shown in the settings window (`index.html`) and the GIF window (`gif.html`), in plain TypeScript with Vite, with their stylesheets in `app/src/styles`.
- `locales/<code>/bediz.ftl`: the Fluent translations, shared by the Rust side and the pages; `en` is the reference.
- `.github/workflows`: `ci.yml` runs the checks, `release.yml` builds and drafts a release from a tag.
- `app/src-tauri/msix`: the Microsoft Store package. `about.toml` and `about.hbs` make the licence notices; `deny.toml` checks licences and advisories.

### `crates/core`

- **Selection and screens:** `geometry` (points and rectangles in virtual screen coordinates) and `selection` (drawing, moving, resizing and nudging the selection, kept on the screen).
- **Hotkeys and settings:** `hotkey` (hotkeys by name and their rules) and `settings` (the user's settings with their defaults, read so one bad value costs only itself, and merged with changes made elsewhere).
- **Captures:** `capture` (how a screen was captured), `image` (BGRA images, PNG encoding and the clipboard's row order) and `cursor` (drawing the pointer into a recorded frame).
- **Copying and saving:** `delivery` (what was asked, the errors either can meet and the report that turns results into notices), `file_name` (the name pattern, numbered names and temporary names) and `notice` (what the user is told, with locale ids).
- **Recording:** `recording` (why a recording stops, its pauses, countdown, size, memory, threads and warnings) and `recording::options` (frame rate, longest side and time limit).
- **The GIF pipeline:** `animation` and its submodules. `store` holds frames as compressed changed areas (`codec`) until they are encoded, comparing each with the colours last sent to encoding (`change`). `PreparedFrame` builds a frame's palette (`palette`), dithers it (`dither`, with `nearest`) and makes the GIF frame (`frame`) on any thread. `GifEncoder` writes prepared frames in order with their delays (`timing`).
- **Timings:** `measurement` (the time from the hotkey to the overlay, for the log).

### `crates/platform`

`lib.rs` exposes the platform: `Platform` (in `windows::platform`) starts the platform thread and is the app's handle to it, and `event` holds the `Command`s the app sends and the `Event`s it gets back. It also exposes the few functions the app calls directly: the folder picker and shell actions, saving the app's own files, the task dialogs, the preferred languages, the dark theme and whether Bediz runs as a package.

In `crates/platform/src/windows`:

- **The runtime:** `runtime` is the platform thread: it owns a message-only window, the overlay, the hotkeys, recordings, copies and saves, acts on commands and reports events. Its submodules split the work: `screenshot` (opening the overlay and acting in it), `delivery` (copies and saves), `save_as`, `recording`, `ready` (the bar before a recording), `displays` (rebuilding the overlay when displays change), `commands`, `quit` (quitting while a GIF is unsaved), `discard` (deleting discarded GIFs) and `hold` (readying for an update's installer).
- **Overlays:** `scene` (every monitor's overlay with its capture, handled as one), `overlay` and `overlay::paint` (one monitor's window and its drawing), `session` (one selection: mouse and keys into a selection and an action), `overlay_menu` (the right-click menu) and `window_list` (the windows a click can select).
- **Graphics and capture:** `displays` (monitors and a device per adapter), `gpu` (Direct3D and Direct2D devices and resources), `capture` with `duplication`, `gdi`, `scrgb` and `white_level` (Desktop Duplication, GDI where it fails, and HDR frames brought to sRGB), `readback` (pixels back from the GPU) and `shader`.
- **Recording:** `recording` and its submodules: `start`, `border` (the frame around the area), `controls` (the bar of buttons, with `drag`, `fonts`, `layout`, `paint` and `tooltips`), `countdown`, `timer`, `scaler` (cropping and scaling on the GPU), `ring` (reading frames back without holding up capture), `pointer`, and `pipeline` with `writer` and `pace` (preparing frames on worker threads, writing the GIF in order, and the log line about how it was written).
- **Files, clipboard and shell:** `save` (writing through a temporary renamed into place), `worker` (the threads that encode and save screenshots), `save_dialog`, `folder` (the folder picker), `shell` (showing a file in its folder, the Recycle Bin) and `clipboard`.
- **Windows integration:** `hotkey`, `print_screen_setting` (the setting that gives Print Screen to screen snipping), `label_window` with `automation` (the short notice and what screen readers hear), `question` (task dialogs), `startup` (starting with Windows), `single_instance`, `package` (running as a Store package), `contrast` (high contrast and the dark theme), `language`, `memory`, `clock`, `foreground`, `desktop_switch`, `virtual_desktop`, `webview` (WebView2 settings Tauri does not expose) and `window_class` (window classes and their procedures).

### `app/src-tauri`

- `main.rs` builds the app with its plugins and commands; `setup` starts the app's parts in order and ends them in order on exit.
- `events` is the events thread: it handles platform events and the app's messages, and updates the tray, the windows, the labels and the log.
- `tray`, `settings_window` and `gif_window` are the tray and the two kinds of window. `commands` holds the commands the pages call; `update` holds the payloads of the events sent to them.
- `settings` is the settings store; `i18n` picks the language and gives the texts; `log` writes the log file.
- `question` (the Print Screen question at start), `quit` (asking before quitting while a GIF is unsaved), `startup` and `updater`.

### `app/src`

`main.ts` builds the settings page from one module per section or shared concern: `settings`, `sections`, `search`, `shortcuts`, `hotkey`, `saving`, `file-name`, `pattern`, `folder`, `recording-options`, `desktop`, `look`, `language`, `updates`, `about`, `licences` and `failure`, with `i18n` for the texts. `gif.ts` is the GIF window's page, with `gif-facts`. Tests sit beside their modules as `*.test.js`.

## What Bediz writes

- **Screenshots and GIFs** go to the save folder (Pictures\Screenshots unless the user picks another), named by the user's pattern. Each is written to a temporary file and renamed when complete, with a rename that never replaces; a name that exists gets a number. The one exception is Save as, where Windows asks before replacing a file. A GIF is written to its temporary file while it is encoded and is never held whole in memory.
- **The settings** are `settings.json` in the app's config folder, written on their own thread to a temporary file that then replaces the old one.
- **The log** is in a `logs` folder in the app's local data folder, with one older file kept.
- **Starting with Windows:** the `Run` value under the user's registry key, or the startup task when Bediz runs as a Store package.
- **The Print Screen setting:** only when the user agrees, the registry setting that makes Print Screen open screen snipping is turned off.

## Invariants

- **The hotkey path never waits.** From the hotkey to the overlay on screen nothing waits on the disk, the network, the WebView or a large allocation; the overlay windows and their GPU resources exist before the key is pressed. The budget is 50 ms to the first frame on screen. The one exception is the setting that releases GPU memory while idle.
- **Idle means idle.** No polling timers while waiting for a hotkey; a recording's timer runs only while it records.
- **Nothing leaves the computer** without the user's action: no telemetry, no upload. The only request goes to GitHub for updates.
- **A recording is always visible:** a frame around the area and a bar with its clock, both kept out of the recording.
- **Nothing is overwritten:** see "What Bediz writes". `std::fs::rename` replaces, so the rename that does not lives in `crates/platform`.
- **No panics.** `unwrap`, `expect`, `panic` and unchecked indexing are denied outside tests, since a crash in a tray app silently takes away the user's hotkeys. Calls from the OS into Rust are guarded so a panic cannot cross them.
- **`unsafe` only in `crates/platform`.**
- **Every user-visible string comes from `locales`,** in every language with the same variables.
- **No administrator rights** are needed to install or run Bediz.

## Boundaries

- **`crates/core` and the rest.** Core knows nothing of Windows, Tauri or threads. Platform and app call it; it never calls them.
- **The app and the platform thread.** The app drives the platform thread only through `Platform`: a `Command` is queued and the platform thread's window is told, and every `Event` comes back through the callback the app gave `Platform::start`, which passes it to the events thread. Neither side touches the other's state. The functions the app calls directly hold no state of the platform thread.
- **Threads.** The Tauri main thread runs the windows and the tray. `bediz-platform` is the runtime; it owns every window and GPU resource it creates. `bediz-events` handles events; `bediz-settings` writes the settings. The runtime hands work that may block to threads that report back to it: `bediz-encode` and `bediz-save` for screenshots, `bediz-save-as` for the Save as dialog, `bediz-discard`, and the GIF pipeline's `bediz-gif-prepare-N` and `bediz-gif-write`. The platform functions the app calls directly run the folder picker on `bediz-folder` and shell actions on `bediz-shell`, and the app shows its task dialogs on `bediz-question` and `bediz-quit`.
- **The pages and the app.** The pages reach Rust only through the commands in `commands.rs` and receive only the events in `update.rs`. The capability files in `app/src-tauri/capabilities` say what the settings window and the GIF windows may use.

## Cross-cutting concerns

- **Quitting.** Quit from the tray asks the platform first; while a GIF is unsaved, a question with a progress bar lets the user wait for it, delete it or stay. On the way out, queued saves finish, GIFs being written are finished and discarded GIFs are deleted.
- **Errors and notices.** Core describes what happened as typed values (`Notice`, `SaveError`) with locale ids. The platform reports them in events; the events thread turns them into text in the user's language and sends them back in `Command::ShowLabel`, and the platform shows them in the label window. Details that only matter for diagnosis go to the log as notes.
- **Translations.** Both the Rust side (`i18n`) and the pages read the same `locales` files. The platform has no texts of its own: menus, buttons and questions get theirs from the app. A new message goes into every language.
- **Accessibility.** The windows follow high contrast themes, and the label and the countdown are presented to UI Automation for screen readers.
- **Testing.** Core's logic has unit tests at the end of each file. Platform code that can run without the screen is tested the same way; tests never capture the screen, record sound or register hotkeys, and their data is built in memory. `crates/core/tests` holds integration tests, `crates/core/benches` the GIF benchmark, and the pages' tests run with Node's test runner (`npm test`).
- **Comments.** Code has no comments unless a reader would get it wrong without one, and then one short line says why. The one exception is each Rust source file's opening `//!` line, which `crates/core/tests/module_docs.rs` checks.

## Where common changes go

- **A new setting:** `settings` in core (the field, its default, how it is read), the settings store in `app/src-tauri`, a `Command` in `event` when the platform needs it, the page's section in `app/src`, and its labels in every `locales` file.
- **A new action in the overlay:** `session` for the key or click, `screenshot` in the runtime for what it does, `overlay_menu` for the menu, core's `selection` if it changes the selection, and the README's key table.
- **A new notice:** `notice` in core, a message in every `locales` file, and the list of ids the Rust side uses in `i18n`.
- **A new recording option:** `recording::options` and `settings` in core, `recording::start` in the platform, and `recording-options` in `app/src`.
- **A change to the GIF's pixels:** `animation` in core; `crates/core/benches` measures it.
- **A new OS:** a folder beside `crates/platform/src/windows`, chosen in `lib.rs` by target, giving the same `Platform`, `Command` and `Event`.
