// 本文が実際に空かどうかの判定（表示側）。
//
// `body_state` は「サーバから本文を取ったか」の記録だが、**嘘をつくことがある**。
// mail_parser が text/plain のメールのヘッダだけから合成する `<html><body></body></html>` を
// 「本文あり」と数えてしまい、本文が空なのに 'present' になっていた不具合が実際にあった
// （2026-09-11。docs/SYNC.md §3.6）。記録ではなく**実体**を見れば、記録が壊れていても
// 開いた時点で自力で取り直せる。

/** 本文 3 列のうち、表示に使えるもの。 */
export type BodyFields = {
  clean_body?: string | null;
  body_plain?: string | null;
  body_html?: string | null;
};

const hasText = (s?: string | null) => !!s && s.trim() !== '';

/**
 * HTML に中身があるか。`<html><body></body></html>` のような空の骨組みは「無い」とみなす。
 *
 * 文字が残るかどうかでは判定しない。画像だけの HTML メールもタグを剥がすと文字が残らないが、
 * そちらは**取得済みの本文**なので「ある」と扱う必要がある。
 */
export const htmlHasContent = (html?: string | null): boolean => {
  if (!html) return false;
  return html.replace(/<\/?(?:html|head|body)\s*>/gi, '').trim() !== '';
};

/** 表示できる本文があるか。 */
export const hasReadableBody = (d: BodyFields): boolean =>
  hasText(d.clean_body) || hasText(d.body_plain) || htmlHasContent(d.body_html);
