#!/usr/bin/env python3
"""
Stitch が生成した JPEG アイコンの外側（角丸長方形より外）を透過する。

使い方:
    python3 make_transparent.py icon-braces-512.jpg icon-braces.png
"""
import sys
from PIL import Image, ImageDraw, ImageFilter
import numpy as np


def detect_bbox(img: Image.Image, bg_threshold: int = 45) -> tuple[int, int, int, int]:
    """背景色（暗い均一色）より明るいピクセルの bounding box を返す。"""
    arr = np.array(img.convert("RGB"))
    lum = arr.mean(axis=2)
    mask = lum >= bg_threshold
    rows = np.where(mask.any(axis=1))[0]
    cols = np.where(mask.any(axis=0))[0]
    return (int(cols.min()), int(rows.min()), int(cols.max()) + 1, int(rows.max()) + 1)


def make_squircle_mask(size: tuple[int, int], radius: int, ss: int = 4) -> Image.Image:
    """角丸長方形のアルファマスク（アンチエイリアス付き）。"""
    w, h = size
    big = Image.new("L", (w * ss, h * ss), 0)
    ImageDraw.Draw(big).rounded_rectangle(
        (0, 0, w * ss - 1, h * ss - 1),
        radius=radius * ss,
        fill=255,
    )
    return big.resize((w, h), Image.LANCZOS)


def main(src: str, dst: str) -> None:
    img = Image.open(src).convert("RGBA")
    w, h = img.size

    bbox = detect_bbox(img)
    bw, bh = bbox[2] - bbox[0], bbox[3] - bbox[1]
    # macOS Big Sur アイコンの角丸は icon body の約 22% が定番
    corner_radius = int(round(min(bw, bh) * 0.22))
    print(f"size: {w}x{h}, body bbox: {bbox}, corner radius: {corner_radius}px")

    # 全面透過の RGBA を作り、bbox の位置に元画像を貼る（マスク経由）
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    body = img.crop(bbox)
    mask = make_squircle_mask((bw, bh), corner_radius)
    out.paste(body, (bbox[0], bbox[1]), mask)

    out.save(dst, "PNG")
    print(f"saved: {dst}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(1)
    main(sys.argv[1], sys.argv[2])
