#!/bin/sh
# Run the desktop app in dev mode: screen and style changes appear in the open window without
# a rebuild (Rust changes recompile by themselves). The window shows a "Live" tag beside the logo.
# Closing the window stops it; run this again to reopen.
cd "$(dirname "$0")/.." || exit 1
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
export PROTOC="$PWD/.spike-data/protoc/bin/protoc"
export INVECTUS_MODELS_DIR="$PWD/.spike-data/models-store"
exec npm run tauri dev -- --release
