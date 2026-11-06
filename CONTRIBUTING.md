# Contributing

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

## How the code is organized

- `crates/core`: logic with no operating system calls, so all of it can be unit tested.
- `crates/platform`: every operating system call, one folder per system. The only crate with `unsafe`.
- `app`: the Tauri app: the tray, the settings window and the start of the platform thread.
- `locales`: the translations, shared by Rust and the settings window. A new message goes into every language with the same variables.

Code has no comments unless a reader would get it wrong without one, and then one short line says why. Nothing may panic outside tests, and nothing a user saves is ever replaced.
