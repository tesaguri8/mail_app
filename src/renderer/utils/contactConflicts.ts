import type { ContactMatch } from '@bindings/ContactMatch';
import type { ConflictOwner } from '../components/ContactConflictHint';

/**
 * 重複候補から「値 → その値と一致した連絡先」を引く表を作る（⚠ の説明に名前で出すため）。
 * キーはバックエンドが返した値そのもの（呼び出し元が渡した文字列）。同じ連絡先は 1 回だけ。
 */
export function conflictOwnersByValue(
  matches: readonly ContactMatch[],
  field: 'matched_emails' | 'matched_phones'
): Map<string, ConflictOwner[]> {
  const out = new Map<string, ConflictOwner[]>();
  for (const m of matches) {
    for (const value of m[field]) {
      const owners = out.get(value) ?? [];
      if (!owners.some((o) => o.id === m.id))
        owners.push({ id: m.id, display_name: m.display_name });
      out.set(value, owners);
    }
  }
  return out;
}
