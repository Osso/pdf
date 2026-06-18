#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"

pdfium_library="${PDFIUM_LIBRARY_PATH:-vendor/pdfium/libpdfium.so}"
if [[ ! -f "$pdfium_library" ]]; then
    echo "missing patched pdfium library: $pdfium_library" >&2
    exit 1
fi
pdfium_dir="$(dirname "$pdfium_library")"

cargo install --path . --locked
install -m 0755 "$pdfium_dir"/*.so "$HOME/.cargo/bin/"
