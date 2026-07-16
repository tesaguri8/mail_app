/**
 * 予定の説明文などに紛れ込む HTML を、テキストエリアで読める素のテキストへ変換する。
 *
 * Google カレンダーの `description` は HTML を含むことがあり（Zoom 招待の `<br>` や
 * `<a href>`、`&amp;` 等）、そのままテキストエリアに出すとタグがむき出しになる。ここでは
 * DOM を使わずに（vitest の node 環境でも動くよう）文字列処理だけで整形する。
 *
 * - `<br>` と段落/リスト等のブロック要素は改行に変換
 * - `<a href="…">表示</a>` は表示テキストを残す。表示が空ならリンク先 URL を使い、表示が
 *   URL を含まないときは「表示 (URL)」の形で URL も添える（テキストエリアでは押せないため）。
 *   Google のリダイレクト URL（`google.com/url?q=実URL`）は実 URL へ展開する
 * - HTML エンティティ（`&amp;` `&lt;` `&nbsp;` 数値参照 …）を復号
 * - HTML と判定できない入力はそのまま返す（冪等）
 */

/** ブロック（前後で改行を作る）要素。 */
const BLOCK_TAG =
  /<\/?(?:p|div|li|ul|ol|tr|td|th|table|thead|tbody|section|article|h[1-6]|blockquote|pre)\b[^>]*>/gi;

/** 中身を残しても意味のない（描画しない）要素は開始タグ〜終了タグごと捨てる。 */
const DROP_BLOCK = /<(script|style|head|title|noscript)\b[^>]*>[\s\S]*?<\/\1>/gi;

/** 入力が HTML を含むと見なせるか（既知タグ・終了タグ・エンティティのいずれか）。 */
function looksLikeHtml(s: string): boolean {
  return (
    /<\/?(?:br|a|p|div|span|ul|ol|li|tr|td|th|table|h[1-6]|blockquote|b|i|u|strong|em|font|pre|img)\b[^>]*>/i.test(
      s,
    ) || /&(?:amp|lt|gt|quot|apos|nbsp|#\d+|#x[0-9a-f]+);/i.test(s)
  );
}

/** タグを取り除く（属性は問わない）。 */
function stripTags(s: string): string {
  return s.replace(/<[^>]+>/g, '');
}

function fromCodePointSafe(cp: number): string {
  try {
    return String.fromCodePoint(cp);
  } catch {
    return '';
  }
}

/** HTML エンティティ・非改行スペースを復号する（`&amp;` は多重復号を避けるため最後に）。 */
function decodeEntities(s: string): string {
  return s
    .replace(/&nbsp;/gi, ' ')
    .replace(/\u00a0/g, ' ')
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/&quot;/gi, '"')
    .replace(/&(?:apos|#0*39);/gi, "'")
    .replace(/&#x([0-9a-f]+);/gi, (_, h: string) => fromCodePointSafe(parseInt(h, 16)))
    .replace(/&#(\d+);/g, (_, d: string) => fromCodePointSafe(parseInt(d, 10)))
    .replace(/&amp;/gi, '&');
}

/** Google のリダイレクト URL（`https://www.google.com/url?q=実URL&…`）を実 URL へ展開する。 */
function unwrapGoogleRedirect(href: string): string {
  try {
    const u = new URL(href);
    if (/(^|\.)google\.[a-z.]+$/i.test(u.hostname) && u.pathname === '/url') {
      const q = u.searchParams.get('q');
      if (q) return q;
    }
  } catch {
    // URL として解釈できなければそのまま。
  }
  return href;
}

/** 開始タグから href の値を取り出す（無ければ null）。 */
function extractHref(openTag: string): string | null {
  const m = openTag.match(/\shref\s*=\s*("([^"]*)"|'([^']*)'|([^\s>]+))/i);
  if (!m) return null;
  return m[2] ?? m[3] ?? m[4] ?? null;
}

/** `<a …>…</a>` を、テキストエリアで読める形（表示テキスト＋必要なら URL）へ置換する。 */
function replaceAnchors(s: string): string {
  return s.replace(/<a\b[^>]*>([\s\S]*?)<\/a>/gi, (whole, inner: string) => {
    const rawText = stripTags(inner);
    const decodedText = decodeEntities(rawText).trim();
    const rawHref = extractHref(whole.slice(0, whole.indexOf('>') + 1));
    const url = rawHref ? unwrapGoogleRedirect(decodeEntities(rawHref).trim()) : '';
    const isHttp = /^https?:\/\//i.test(url);
    if (!decodedText) return url;
    if (isHttp && !decodedText.includes(url)) return `${rawText} (${url})`;
    return rawText;
  });
}

/**
 * HTML 混じりの文字列を素のテキストへ整形する。HTML でなければそのまま返す。
 * `null`/`undefined` は空文字にする。
 */
export function htmlToText(input: string | null | undefined): string {
  const s = (input ?? '').trim();
  if (!s || !looksLikeHtml(s)) return s;

  let out = s;
  out = out.replace(DROP_BLOCK, '');
  out = replaceAnchors(out);
  out = out.replace(/<br\s*\/?>/gi, '\n');
  out = out.replace(BLOCK_TAG, '\n');
  out = stripTags(out);
  out = decodeEntities(out);

  return out
    .replace(/\r\n?/g, '\n')
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}
