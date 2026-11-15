# Kaja CI

Kaja CI is a local CI tool for running project automation tasks (build, test, release, custom commands) from a web UI.

It runs entirely on your machine:
- Written in Rust
- WebAssembly frontend
- WebSocket streaming for live task output
- Integrates well with the terminal on your system and supports running command in a terminal emulator in the browser.

## What it does

- Manage local projects and per-project `ci.json` task settings
- Run common CI-like tasks:
  - `git`
  - `build`
  - `test`
  - `release`
  - `clean`
  - `terminal`
  - `custom commands`
- Stream task logs live in the browser
- Persist logs/history under `~/.kaja-ci/logs`

## Project structure

- `backend/` — The runner
- `frontend/` — Rust/WASM frontend 

## Build requirements

- Rust toolchain (`cargo`, `rustc`)
- `wasm-pack` (for building frontend)

## Run locally

1. Build frontend assets:

```sh
cd frontend
./build.sh
```

2. Start backend server:

```sh
cd ../backend
cargo run
```

3. Open:

- `http://127.0.0.1:2003/`

Backend also exposes WebSocket endpoints:
- `ws://127.0.0.1:2003/runner`
- `ws://127.0.0.1:2003/task`

## Configuration

Each project can have a `ci.json` file with commands for tasks. Example:

```json
{
  "git": "git pull",
  "build": "cargo build",
  "test": "cargo test",
  "release": "cargo build --release",
  "clean": "cargo clean",
  "terminal": "bash",
  "tasks": ["git", "build", "test"]
}
```

## Notes

- Commands run on your machine with your local environment and permissions.
