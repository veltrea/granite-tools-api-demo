#!/bin/bash
# icon-braces.png から AppIcon.icns を作る
# 使い方: ./build_icns.sh
set -euo pipefail
cd "$(dirname "$0")"

SRC="icon-braces.png"
ICONSET="AppIcon.iconset"
OUT="AppIcon.icns"

if [ ! -f "$SRC" ]; then
  echo "ERROR: $SRC not found. Run make_transparent.py first." >&2
  exit 1
fi

rm -rf "$ICONSET" "$OUT"
mkdir -p "$ICONSET"

# Apple の標準 .iconset サイズ一式（@1x と @2x）
# 元画像が 512x512 なので 1024 サイズは 512 から作る（軽い拡大、視覚的な劣化は最小）
declare -a SIZES=(
  "16:icon_16x16.png"
  "32:icon_16x16@2x.png"
  "32:icon_32x32.png"
  "64:icon_32x32@2x.png"
  "128:icon_128x128.png"
  "256:icon_128x128@2x.png"
  "256:icon_256x256.png"
  "512:icon_256x256@2x.png"
  "512:icon_512x512.png"
  "1024:icon_512x512@2x.png"
)

for entry in "${SIZES[@]}"; do
  size="${entry%%:*}"
  name="${entry#*:}"
  sips -z "$size" "$size" "$SRC" --out "$ICONSET/$name" >/dev/null
done

iconutil -c icns "$ICONSET" -o "$OUT"
rm -rf "$ICONSET"

echo "built: $OUT"
ls -la "$OUT"
