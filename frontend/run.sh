#!/bin/bash
set -e

./build.sh

cd pkg
python3 -m http.server 8080 --bind 127.0.0.1
