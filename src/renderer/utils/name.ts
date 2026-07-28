/**
 * 表示名（差出人名など）を「姓・名」に推定分割する。
 * 住所録の＋追加で、差出人情報から予測できる範囲を自動入力するために使う。
 *
 * 方針（誤りより空欄を優先する安全側）:
 *  - 「姓, 名」（欧米のカンマ表記）はカンマで分割。
 *  - 空白（半角/全角）で 2 語以上に分かれるとき:
 *      日本語（CJK を含む） → 「姓 名」順とみなし 先頭=姓・残り=名。
 *      ラテン文字 → 「given ... family」順とみなし 末尾=姓・残り=名。
 *  - 空白が無く分割できない（例:「末松慎吾」）／組織らしい名前（株式会社・Inc 等）は
 *    分割しない（表示名だけ入れて姓名は空欄のまま人に委ねる）。
 */
export function splitPersonName(display: string): {
  family: string | null;
  given: string | null;
} {
  const raw = stripQuotes(display).trim();
  if (!raw || looksLikeOrg(raw)) return { family: null, given: null };

  // 「Family, Given」（欧米の 姓,名 表記）。
  const comma = raw.match(/^([^,]+),\s*(.+)$/);
  if (comma) {
    return {
      family: nonEmpty(comma[1]),
      given: nonEmpty(comma[2]),
    };
  }

  // 空白（半角スペース・全角スペース U+3000 など）で分割。1 語のみなら分割しない。
  const parts = raw.split(/[\s\u3000]+/).filter(Boolean);
  if (parts.length < 2) return { family: null, given: null };

  if (hasCjk(raw)) {
    // 日本語など: 先頭を姓、残りを名。
    return { family: parts[0], given: parts.slice(1).join(' ') };
  }
  // ラテン: 末尾を姓、残り（ミドルネーム含む）を名。
  return {
    family: parts[parts.length - 1],
    given: parts.slice(0, -1).join(' '),
  };
}

/**
 * 姓・名から表示名を組み立てる（splitPersonName の逆）。
 * 日本語（CJK を含む）は「姓 名」、ラテン文字は「given family」の順に並べる。
 * 片方だけのときはその値をそのまま返し、両方空なら空文字を返す。
 */
export function joinPersonName(family: string | null, given: string | null): string {
  const f = family?.trim() ?? '';
  const g = given?.trim() ?? '';
  if (!f) return g;
  if (!g) return f;
  return hasCjk(`${f}${g}`) ? `${f} ${g}` : `${g} ${f}`;
}

/** 前後の引用符（"" '' 「」 “” 等）や空白を取り除く。 */
function stripQuotes(s: string): string {
  return s.replace(/^[\s\u3000"'“”「」『』]+|[\s\u3000"'“”「」『』]+$/g, '');
}

// ひらがな/カタカナ/CJK 統合漢字/半角カナ を含むか。
const CJK_RE = /[぀-ヿ㐀-䶿一-鿿豈-﫿ｦ-ﾟ]/;
function hasCjk(s: string): boolean {
  return CJK_RE.test(s);
}

// 会社・組織・自動送信らしい名前。人名の姓/名に割ると不自然なので分割しない。
const ORG_RE =
  /(株式会社|有限会社|合同会社|合名会社|合資会社|一般社団|一般財団|財団法人|社団法人|事務局|サポート|カスタマー|センター|\bInc\b|\bLtd\b|\bLLC\b|\bLLP\b|\bCorp\b|\bCo\.|\bGmbH\b|\bTeam\b|\bSupport\b|no-?reply)/i;
function looksLikeOrg(s: string): boolean {
  return ORG_RE.test(s);
}

function nonEmpty(s: string): string | null {
  const t = s.trim();
  return t === '' ? null : t;
}
