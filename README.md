# Bediz

[![CI](https://github.com/halledilirya/bediz/actions/workflows/ci.yml/badge.svg)](https://github.com/halledilirya/bediz/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Bediz is a screenshot app for Windows 11. It waits in the tray: press Print Screen, select a part of the screen, then copy and save it. Recording a region as a GIF and marking up screenshots are planned.

Bediz is in early development and has no release yet.

## Download

There is no release yet. To try Bediz now, install [Rust](https://rustup.rs) and run `cargo run --release -p bediz` in a copy of this repository.

## Usage

Press **Print Screen** or click the tray icon. The screen freezes on every monitor. Drag to select a region, which can span monitors, then press **Enter** to copy it and save it to Pictures\Screenshots as a PNG. A file that exists is never replaced; the new one gets a number.

| Key | Action |
| --- | --- |
| Drag | Select a region; drag its edges or corners to resize it, its inside to move it |
| Shift while dragging | Keep a square |
| Alt while dragging | Grow a new selection from its centre |
| Arrow keys | Move the selection by one pixel, ten with Shift |
| Ctrl + arrow keys | Resize the selection from its right or bottom edge |
| Ctrl+A | Select the whole monitor under the cursor |
| R | Select the region copied or saved last since Bediz started |
| C | Copy the colour under the cursor as #RRGGBB |
| Enter or a double-click | Copy and save the selection |
| Ctrl+C | Only copy the selection |
| Ctrl+S | Only save the selection |
| Esc | Cancel |

If Print Screen opens the Snipping Tool, choose **Let Bediz use Print Screen** in the tray menu. When another app holds the key, the tray menu says so; the tray icon still takes a screenshot. Windows does not send Print Screen to Bediz while a window running as administrator is in front.

Bediz follows the Windows display language; it speaks English and Turkish so far.

Bediz sends nothing anywhere: it has no telemetry and uploads nothing, and the screenshots it copies are kept out of the Windows cloud clipboard. Saved files are like any other file in their folder: if OneDrive or another service syncs Pictures\Screenshots, it uploads them too.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
