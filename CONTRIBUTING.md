# Contributing

[ARCHITECTURE.md](ARCHITECTURE.md) describes how the code is organized and the rules it keeps.

Thank you for helping. Bugs and ideas go in [issues](https://github.com/halledilirya/bediz/issues); for a larger change, open an issue first so we can agree on it before you write code.

## Building

You need Windows 11, [Rust](https://rustup.rs) and [Node.js](https://nodejs.org).

```
cd app
npm ci
npm run tauri dev
```

This starts Vite for the settings window, which reloads when `app/src` changes, and builds and starts the app again when the Rust code changes. Timings are measured on an optimized build: `npm run tauri dev -- -- --profile quick`.

## Checks

CI runs these, and a pull request needs them to pass:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cd app
npm run format:check
npm run lint
npm test
npm run build
```

## The Microsoft Store package

The same `bediz.exe` is installed by the setup file and by an MSIX package from the Microsoft Store. Packaged, it starts with Windows through the startup task in `app/src-tauri/msix/AppxManifest.xml` and leaves updates to the Store. The Store takes Bediz from version 1.0.0 on, since it refuses a package version that starts with 0. After `npm run tauri build`, this makes the package in `target/msix` with the Windows SDK's tools:

```
powershell -ExecutionPolicy Bypass -File app/src-tauri/msix/pack.ps1
```

To try it, turn on Developer Mode in Windows and register the package's folder; remove it before packing again:

```
Add-AppxPackage -Register target\msix\Bediz\AppxManifest.xml
Get-AppxPackage Bediz | Remove-AppxPackage
```

## How the code is organized

- `crates/core`: logic with no operating system calls, so all of it can be unit tested.
- `crates/platform`: every operating system call, one folder per system. The only crate with `unsafe`.
- `app`: the Tauri app: the tray, the settings window, the GIF window and the start of the platform thread.
- `locales`: the translations, shared by Rust and the settings window. A new message goes into every language with the same variables.

[ARCHITECTURE.md](ARCHITECTURE.md) has the full code map.

Every Rust source file under `crates/*/src` and `app/src-tauri/src` starts with a `//!` comment saying what it is for, which a test checks; tests, benchmarks and build scripts do not need one. Other comments explain why, not what, and only when the code cannot. Nothing may panic outside tests, and nothing a user saves is ever replaced.
