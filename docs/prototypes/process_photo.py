#!/usr/bin/env python
"""
無地の青空背景のツバメ写真から背景を抜いて、透過 PNG の切り抜きを作る.

使い方:
    python process_photo.py "<入力画像>" [出力PNG]

青空は「青みが強い(b が r,g より明確に大きい)」画素として判定し、
エッジは blueMetric の連続値で半透明にしてギザギザを抑える。
最後にツバメの外接矩形でトリミングして保存する。
"""
import sys
import os
from PIL import Image, ImageFilter

LO, HI = 14, 40   # blueMetric: <=LO を鳥(不透明) / >=HI を空(透明) として線形補間


def key_out(src_path, out_path):
    im = Image.open(src_path).convert("RGB")
    w, h = im.size
    px = im.load()
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    op = out.load()

    minx, miny, maxx, maxy = w, h, 0, 0
    for y in range(h):
        for x in range(w):
            r, g, b = px[x, y]
            blue = b - max(r, g)               # 空ほど大きい
            if blue >= HI:
                a = 0
            elif blue <= LO:
                a = 255
            else:
                a = int(255 * (HI - blue) / (HI - LO))
            op[x, y] = (r, g, b, a)
            if a > 24:
                if x < minx: minx = x
                if y < miny: miny = y
                if x > maxx: maxx = x
                if y > maxy: maxy = y

    # 縁をわずかにぼかしてフリンジを馴染ませる
    alpha = out.getchannel("A").filter(ImageFilter.GaussianBlur(0.6))
    out.putalpha(alpha)

    # 外接矩形でトリム（少し余白）
    pad = 6
    box = (max(0, minx - pad), max(0, miny - pad),
           min(w, maxx + pad), min(h, maxy + pad))
    cropped = out.crop(box)
    cropped.save(out_path)
    print(f"src {w}x{h} -> bird box {box} -> {cropped.size} saved {out_path}")


if __name__ == "__main__":
    src = sys.argv[1]
    out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(
        os.path.dirname(__file__), "frames", "swallow_real.png")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    key_out(src, out)
