// 郵便番号の自動整形。国別ルール（今は日本のみ: 7桁 → NNN-NNNN）。

/** 郵便番号を国に応じて整形。ルールに合わなければ元のトリム文字列を返す。 */
export function formatPostal(raw: string, region: string): string {
  const s = (raw ?? '').trim();
  if (!s) return '';
  if (region === 'JP') {
    // 全角数字も半角へ畳み、数字のみ抽出。7桁なら NNN-NNNN。
    const digits = s
      .replace(/[０-９]/g, (c) => String.fromCharCode(c.charCodeAt(0) - 0xfee0))
      .replace(/[^0-9]/g, '');
    if (digits.length === 7) return `${digits.slice(0, 3)}-${digits.slice(3)}`;
  }
  return s;
}
