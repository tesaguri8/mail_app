// メールヘッダのアドレス文字列を「表示名」と「メールアドレス」に分解する。
// 例: '"末松" <a@b.com>' → { name: '末松', email: 'a@b.com' }
//     'a@b.com'          → { name: '',   email: 'a@b.com' }

export type ParsedAddress = { name: string; email: string };

/** 単一アドレスを分解（表示名の前後の引用符は剥がす）。 */
export function parseAddress(raw: string | null | undefined): ParsedAddress {
  const s = (raw ?? '').trim();
  if (!s) return { name: '', email: '' };
  const m = s.match(/^(.*?)<([^>]+)>\s*$/);
  if (m) {
    const name = m[1].trim().replace(/^"(.*)"$/, '$1').trim();
    return { name, email: m[2].trim() };
  }
  return { name: '', email: s };
}

/** カンマ区切りの複数宛先を「表示名（無ければメール）」の一覧に整形。 */
export function parseAddressList(raw: string | null | undefined): ParsedAddress[] {
  const s = (raw ?? '').trim();
  if (!s) return [];
  return s
    .split(',')
    .map((part) => parseAddress(part))
    .filter((p) => p.name || p.email);
}
