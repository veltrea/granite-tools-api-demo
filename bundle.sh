#!/bin/bash
# Build and package ChatClient.app
set -euo pipefail
cd "$(dirname "$0")"

echo "Building release..."
cargo build --release

APP="ChatClient.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/granite-tools-api-demo "$APP/Contents/MacOS/granite-tools-api-demo"
chmod +x "$APP/Contents/MacOS/granite-tools-api-demo"

echo "Done: $APP ($(du -sh "$APP" | cut -f1))"
echo "  open ChatClient.app"
echo "  ENDPOINT=http://host:port open ChatClient.app"
