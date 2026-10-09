# 郵便番号データの出典

- 元データ: 日本郵便「住所の郵便番号（1レコード1行、UTF-8形式）」
- 取得元: https://www.post.japanpost.jp/service/search/zipcode/download/utf/zip/utf_ken_all.zip
- 取得日: 2026-10-09
- 元の行数: 124526 / 同梱した行数: 124525（町域の括弧を注記へ分け、重複を除いた）
- 同梱ファイル: ken_all.tsv.zst（857229 バイト。zstd -19）
- 作り方: `node scripts/gen-postal-data.mjs`（このファイルも書き直される）

日本郵便の郵便番号データは、著作権を主張しないとされ、自由に配布できる。
