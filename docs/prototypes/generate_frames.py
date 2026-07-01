#!/usr/bin/env python
"""
Rondine — Fly 送信アニメ用の「羽ばたき連番」プレースホルダ生成スクリプト.

実写/CG の連番が用意できるまでの検証用に、ツバメの *シルエット* を
1 羽ばたき周期ぶん（既定 12 枚）の透過 PNG として書き出す。
本番では同じ命名 (swallow_00.png .. swallow_11.png) の実写フレームに
差し替えるだけで、HTML ハーネス側は無改修で動く。

参考写真に合わせた「横向き（頭が左）」の飛翔シルエット。ハーネス側で
進行方向に応じて左右反転(scaleX)＋バンク(rotate)する。
"""
import math
import os
from PIL import Image, ImageDraw

OUT_DIR = os.path.join(os.path.dirname(__file__), "frames")
FRAME_COUNT = 12
SIZE = 300          # 最終出力の 1 辺(px)
SS = 3              # スーパーサンプリング倍率(輪郭を滑らかに)
INK = (20, 26, 38, 240)       # 逆光シルエットの色(濃紺に近い黒)
INK_FAR = (20, 26, 38, 150)   # 奥の翼(やや薄く)

S = SIZE * SS


def wing_polygon(shoulder, alpha_deg, length, width):
    """尖って後方へ swept した翼。alpha_deg: 翼軸の角度(+下 / -上, +x=尾側)."""
    sx, sy = shoulder
    a = math.radians(alpha_deg)
    ax, ay = math.cos(a), math.sin(a)      # 翼軸
    px, py = -math.sin(a), math.cos(a)     # 直交(幅方向)
    L, w = length, width
    return [
        (sx + px * w, sy + py * w),                                    # 付け根・前縁
        (sx + ax * 0.35 * L + px * w * 1.05, sy + ay * 0.35 * L + py * w * 1.05),
        (sx + ax * 0.68 * L + px * w * 0.75, sy + ay * 0.68 * L + py * w * 0.75),
        (sx + ax * L, sy + ay * L),                                    # 翼端(尖り)
        (sx + ax * 0.45 * L - px * w * 0.18, sy + ay * 0.45 * L - py * w * 0.18),  # 後縁(えぐれ)
        (sx - px * w * 0.55, sy - py * w * 0.55),                      # 付け根・後縁
    ]


def draw_frame(idx):
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    t = idx / FRAME_COUNT
    # 翼軸角: -66°(打ち上げ) .. +54°(打ち下ろし) を正弦で往復
    alpha = -6 + 60 * math.sin(2 * math.pi * t)

    shoulder = (0.42 * S, 0.43 * S)

    # 奥の翼(体の後ろ, 少し短く薄く) — 先に描く
    d.polygon(wing_polygon((shoulder[0] + 0.03 * S, shoulder[1] + 0.01 * S),
                           alpha, 0.34 * S, 0.045 * S), fill=INK_FAR)

    # 長い燕尾(深く二又・細い尾羽が右へ流れる)
    tail = [
        (0.56 * S, 0.42 * S),
        (0.95 * S, 0.34 * S),   # 上の尾羽
        (0.82 * S, 0.45 * S),   # フォークの谷
        (0.98 * S, 0.52 * S),   # 下の尾羽
        (0.56 * S, 0.49 * S),
    ]
    d.polygon(tail, fill=INK)

    # 胴体(頭→尾へ流線)
    d.ellipse([0.24 * S, 0.385 * S, 0.60 * S, 0.50 * S], fill=INK)
    # 頭
    hr = 0.058 * S
    hx, hy = 0.29 * S, 0.435 * S
    d.ellipse([hx - hr, hy - hr, hx + hr, hy + hr], fill=INK)
    # くちばし(左へ)
    d.polygon([(hx - hr * 0.6, hy - 0.018 * S), (hx - 0.055 * S, hy),
               (hx - hr * 0.6, hy + 0.018 * S)], fill=INK)

    # 手前の翼(羽ばたきの主役)
    d.polygon(wing_polygon(shoulder, alpha, 0.44 * S, 0.058 * S), fill=INK)

    img = img.resize((SIZE, SIZE), Image.LANCZOS)
    return img


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    for i in range(FRAME_COUNT):
        p = os.path.join(OUT_DIR, f"swallow_{i:02d}.png")
        draw_frame(i).save(p)
        print("wrote", p)
    print(f"done: {FRAME_COUNT} frames in {OUT_DIR}")


if __name__ == "__main__":
    main()
