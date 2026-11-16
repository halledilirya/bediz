# Contributing

Thank you for helping. Bugs and ideas go in [issues](https://github.com/halledilirya/bediz/issues); for a larger change, open an issue first so we can agree on it before you write code.

## Building

You need Windows 11, [Rust](https://rustup.rs) and [Node.js](https://nodejs.org).

```
cd app
npm ci
npm run tauri dev
```

This starts Vite for the app's pages, which reload when `app/src` changes, and builds and starts the app again when the Rust code changes. The log records how long the selection takes to appear after the shortcut; measure it on an optimized build: `npm run tauri dev -- -- --profile quick`.

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

## The code

[ARCHITECTURE.md](ARCHITECTURE.md) describes how the code is organized and the rules it keeps.

Every Rust source file under `crates/*/src` and `app/src-tauri/src` starts with a `//!` comment saying what it is for, which a test checks; tests, benchmarks and build scripts do not need one. Other comments explain why, not what, and only when the code cannot. Nothing may panic outside tests, and a saved file never replaces another unless the user agrees in Save as. A new message goes into every language in `locales` with the same variables.

## The Microsoft Store package

Bediz will also be offered in the Microsoft Store from version 1.0.0, since the Store refuses a package version that starts with 0. The package holds the same `bediz.exe` as the setup file; packaged, it starts with Windows through the startup task in `app/src-tauri/msix/AppxManifest.xml` and leaves updates to the Store. After `npm run tauri build`, this makes the package in `target/msix` with the Windows SDK's tools:

```
powershell -ExecutionPolicy Bypass -File app/src-tauri/msix/pack.ps1
```

To try it, turn on Developer Mode in Windows and register the package's folder; remove it before packing again:

```
Add-AppxPackage -Register target\msix\Bediz\AppxManifest.xml
Get-AppxPackage Bediz | Remove-AppxPackage
```
