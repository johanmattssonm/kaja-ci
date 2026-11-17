# Kaja CI

<img src="font/kaja-ci.svg" alt="Kaja CI" width="160" />

Kaja CI is a local CI tool for running project automation tasks (build, test, release, custom commands) from a web UI.

It runs entirely on your machine:
- Written in Rust
- WebAssembly frontend
- WebSocket streaming for live task output
- Integrates well with the terminal on your system and supports running command in a terminal emulator in the browser.

## What it does

- Manage local projects and per-project `ci.json` task settings
- Run common CI-like tasks:
  - `git fetch`
  - `build`
  - `test`
  - `release`
  - `clean`
  - `custom commands`
- Stream task logs live in the browser
- Persist logs/history under `~/.kaja-ci/logs`
- Provides a terminal in the webview that puts you rigth into the project folder

## Project structure

- `backend/` — The runner
- `frontend/` — Rust/WASM frontend 

## Run locally

```sh
cd src-tauri
cargo run
```

## Configuration

Each project have a `ci.json` file with commands for tasks.

## Notes

- Commands run on your machine with your local environment and permissions.
