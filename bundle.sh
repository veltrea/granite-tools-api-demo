#!/bin/bash
# Build and package ChatClient.app
set -euo pipefail
cd "$(dirname "$0")"

echo "Building release..."
cargo build --release

APP="ChatClient.app"
BIN="granite-tools-api-demo"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# Binary
cp "target/release/$BIN" "$APP/Contents/MacOS/$BIN"
chmod +x "$APP/Contents/MacOS/$BIN"

# Resources: prompts, config, icon
cp -R prompts "$APP/Contents/Resources/"
cp config.json "$APP/Contents/Resources/"

# Icon (build if missing)
if [ ! -f assets/AppIcon.icns ]; then
  echo "Building AppIcon.icns..."
  (cd assets && ./build_icns.sh)
fi
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

# Info.plist
cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>           <string>Granite Dev Chat</string>
  <key>CFBundleDisplayName</key>    <string>Granite Dev Chat</string>
  <key>CFBundleExecutable</key>     <string>$BIN</string>
  <key>CFBundleIdentifier</key>     <string>com.veltrea.granite-tools-api-demo</string>
  <key>CFBundleVersion</key>        <string>0.1.0</string>
  <key>CFBundleShortVersionString</key> <string>0.1.0</string>
  <key>CFBundlePackageType</key>    <string>APPL</string>
  <key>CFBundleIconFile</key>       <string>AppIcon</string>
  <key>LSMinimumSystemVersion</key> <string>11.0</string>
  <key>NSHighResolutionCapable</key> <true/>
</dict>
</plist>
EOF

echo "Done: $APP ($(du -sh "$APP" | cut -f1))"
echo "  open ChatClient.app"
