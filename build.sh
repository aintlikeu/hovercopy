#!/bin/sh
# Cross-builds dist/HoverCopy.exe on Linux.
# Needs: rustup target add x86_64-pc-windows-gnu; apt install gcc-mingw-w64-x86-64
set -e
cd "$(dirname "$0")"
cargo build --release --target x86_64-pc-windows-gnu
mkdir -p dist
cp "${CARGO_TARGET_DIR:-target}/x86_64-pc-windows-gnu/release/HoverCopy.exe" dist/
ls -l dist/HoverCopy.exe
