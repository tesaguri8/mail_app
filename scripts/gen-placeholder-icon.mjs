// プレースホルダのアプリアイコン(app-icon.png, 1024x1024 RGBA)を生成する。
// 依存ライブラリ無し（Node 標準の zlib のみ）。後で本番アイコンに差し替え可。
// 使い方: node scripts/gen-placeholder-icon.mjs  → app-icon.png 生成
//        npx tauri icon app-icon.png            → 各形式を src-tauri/icons へ
import { deflateSync, crc32 } from 'node:zlib';
import { writeFileSync } from 'node:fs';

const W = 1024;
const H = 1024;

// 縦グラデーション（#1a1a2e → #0f3460）。ブランドの全面ビジュアルに合わせた色。
const top = [0x1a, 0x1a, 0x2e];
const bottom = [0x0f, 0x34, 0x60];
const lerp = (a, b, t) => Math.round(a + (b - a) * t);

const raw = Buffer.alloc(H * (1 + W * 4));
let o = 0;
for (let y = 0; y < H; y++) {
  raw[o++] = 0; // filter: none
  const t = y / (H - 1);
  const r = lerp(top[0], bottom[0], t);
  const g = lerp(top[1], bottom[1], t);
  const b = lerp(top[2], bottom[2], t);
  for (let x = 0; x < W; x++) {
    raw[o++] = r;
    raw[o++] = g;
    raw[o++] = b;
    raw[o++] = 0xff;
  }
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const typeBuf = Buffer.from(type, 'ascii');
  const body = Buffer.concat([typeBuf, data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body) >>> 0, 0);
  return Buffer.concat([len, body, crc]);
}

const sig = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(W, 0);
ihdr.writeUInt32BE(H, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // color type RGBA
ihdr[10] = 0; // compression
ihdr[11] = 0; // filter
ihdr[12] = 0; // interlace

const png = Buffer.concat([
  sig,
  chunk('IHDR', ihdr),
  chunk('IDAT', deflateSync(raw)),
  chunk('IEND', Buffer.alloc(0)),
]);

writeFileSync('app-icon.png', png);
console.log('app-icon.png を生成しました (1024x1024)。次: npx tauri icon app-icon.png');
