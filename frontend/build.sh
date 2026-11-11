#!/bin/sh
set -e

wasm-pack build \
    --release \
    --target web \
    --out-dir pkg \
    --out-name kajaci \
    --no-typescript

cp main.js pkg/
cp styles.css pkg/
