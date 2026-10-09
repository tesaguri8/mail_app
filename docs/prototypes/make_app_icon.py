"""アプリの仮アイコン（雲のある青空＋送信のつばめ）を 1024x1024 で生成する。

使い方（リポジトリ直下で）:
    python docs/prototypes/make_app_icon.py src-tauri/icons/app-icon.png
    npx tauri icon src-tauri/icons/app-icon.png
要 Pillow。つばめは src/renderer/assets/swallow.png（Fly 送信演出と同じ素材）。
"""
import random
import sys

from PIL import Image, ImageDraw, ImageFilter

N = 1024
TOP, BOTTOM = (58, 128, 214), (150, 200, 245)
# 雲の中心 x, y と幅（下と右上に寄せる）
CLOUDS = [(220, 900, 360), (820, 830, 300), (860, 170, 220)]
BIRD_SCALE = 0.8


def sky() -> Image.Image:
    im = Image.new("RGBA", (N, N))
    d = ImageDraw.Draw(im)
    for y in range(N):
        t = y / (N - 1)
        d.line([(0, y), (N, y)], fill=tuple(int(TOP[i] + (BOTTOM[i] - TOP[i]) * t) for i in range(3)) + (255,))
    layer = Image.new("L", (N, N), 0)
    ld = ImageDraw.Draw(layer)
    rnd = random.Random(1)
    for cx, cy, w in CLOUDS:
        for _ in range(9):
            r = rnd.uniform(0.25, 0.5) * w
            x = cx + rnd.uniform(-w * 0.6, w * 0.6)
            y = cy + rnd.uniform(-w * 0.12, w * 0.08)
            ld.ellipse([x - r, y - r * 0.75, x + r, y + r * 0.75], fill=255)
        ld.rectangle([cx - w * 0.75, cy, cx + w * 0.75, cy + w * 0.25], fill=255)
    layer = layer.filter(ImageFilter.GaussianBlur(28))
    white = Image.new("RGBA", (N, N), (255, 255, 255, 255))
    return Image.composite(white, im, layer.point(lambda v: int(v * 0.92)))


def main(out: str) -> None:
    bird = Image.open("src/renderer/assets/swallow.png").convert("RGBA")
    im = sky()
    w = int(N * BIRD_SCALE)
    h = int(bird.height * w / bird.width)
    im.alpha_composite(bird.resize((w, h), Image.LANCZOS), ((N - w) // 2, (N - h) // 2))
    im.save(out)


if __name__ == "__main__":
    main(sys.argv[1])
