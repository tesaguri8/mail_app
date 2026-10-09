#!/usr/bin/env node
// 日本郵便の郵便番号データ（住所の郵便番号・1 レコード 1 行・UTF-8）から、アプリに同梱する
// 郵便番号表（src-tauri/data/postal/ken_all.tsv.zst）を作る。住所を外部へ送らずに、
// 郵便番号 ⇄ 住所を引くため（services/postal.rs が include_bytes! で読む）。
//
// 使い方（リポジトリ直下で）:
//   node scripts/gen-postal-data.mjs                 # 取得から変換まで
//   node scripts/gen-postal-data.mjs utf_ken_all.zip # 手元の zip から変換だけ
// 要: unzip / zstd（コマンド）。
//
// 出力の 1 行: 郵便番号7桁 \t 都道府県 \t 市区町村 \t 町域 \t 注記
//   - 町域の「以下に掲載がない場合」「○○の次に番地がくる場合」「○○村一円」は空（市区町村の既定の番号）
//   - 町域の括弧は注記へ分ける（「大通西（１〜１９丁目）」→ 町域「大通西」・注記「１〜１９丁目」）
//   - UTF-8 版は 1 レコード 1 行なので、旧形式（Shift_JIS 版）の「町域が複数行に分かれる」癖は無い

import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const SOURCE_URL =
  'https://www.post.japanpost.jp/service/search/zipcode/download/utf/zip/utf_ken_all.zip';
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const OUT_DIR = join(ROOT, 'src-tauri/data/postal');

async function obtainZip(arg) {
  if (arg) return resolve(arg);
  const res = await fetch(SOURCE_URL);
  if (!res.ok) throw new Error(`取得に失敗: HTTP ${res.status} ${SOURCE_URL}`);
  const dir = mkdtempSync(join(tmpdir(), 'ken-all-'));
  const path = join(dir, 'utf_ken_all.zip');
  writeFileSync(path, Buffer.from(await res.arrayBuffer()));
  return path;
}

/** CSV の 1 行を列へ（引用符で囲まれた列にカンマは現れない形式だが、引用符は外す）。 */
const columns = (line) => line.split(',').map((c) => c.replace(/^"|"$/g, ''));

/** 町域を（町域, 注記）へ。市区町村の既定の番号を表す町域は空にする。 */
function splitTown(town, city) {
  if (town === '以下に掲載がない場合' || town.endsWith('の次に番地がくる場合') || town === `${city}一円`) {
    return ['', ''];
  }
  const open = town.indexOf('（');
  if (open < 0) return [town, ''];
  const close = town.lastIndexOf('）');
  return [town.slice(0, open), town.slice(open + 1, close > open ? close : town.length)];
}

const zip = await obtainZip(process.argv[2]);
const csv = execFileSync('unzip', ['-p', zip], { maxBuffer: 1 << 30 }).toString('utf8');
const seen = new Set();
const rows = [];
let source = 0;
for (const line of csv.split(/\r?\n/)) {
  if (!line) continue;
  source++;
  const c = columns(line);
  const [code, pref, city, rawTown] = [c[2], c[6], c[7], c[8]];
  const [town, note] = splitTown(rawTown, city);
  const row = [code, pref, city, town, note].join('\t');
  if (seen.has(row)) continue;
  seen.add(row);
  rows.push(row);
}

mkdirSync(OUT_DIR, { recursive: true });
const tsv = `${rows.join('\n')}\n`;
const out = join(OUT_DIR, 'ken_all.tsv.zst');
writeFileSync(out, execFileSync('zstd', ['-19', '-q', '-c'], { input: tsv, maxBuffer: 1 << 30 }));
const size = readFileSync(out).length;
const today = new Date().toISOString().slice(0, 10);
writeFileSync(
  join(OUT_DIR, 'SOURCE.md'),
  `# 郵便番号データの出典

- 元データ: 日本郵便「住所の郵便番号（1レコード1行、UTF-8形式）」
- 取得元: ${SOURCE_URL}
- 取得日: ${today}
- 元の行数: ${source} / 同梱した行数: ${rows.length}（町域の括弧を注記へ分け、重複を除いた）
- 同梱ファイル: ken_all.tsv.zst（${size} バイト。zstd -19）
- 作り方: \`node scripts/gen-postal-data.mjs\`（このファイルも書き直される）

日本郵便の郵便番号データは、著作権を主張しないとされ、自由に配布できる。
`,
);
console.log(`rows=${rows.length} (source ${source}) size=${size} -> ${out}`);
