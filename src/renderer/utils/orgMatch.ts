import type { OrgMatchReason } from '@bindings/OrgMatchReason';

/**
 * 似ている組織の理由を、翻訳キーと差し込む値にする（「電話 0980-51-0322 が同じ」など）。
 * 電話は `formatPhone` で表示用に整える（保存値は E.164）。
 */
export function orgMatchReasonLabel(
  reason: OrgMatchReason,
  formatPhone: (stored: string) => string
): { key: string; values?: Record<string, string> } {
  switch (reason.kind) {
    case 'same_name':
      return { key: 'contact.orgMatchSameName' };
    case 'similar_name':
      return { key: 'contact.orgMatchSimilarName' };
    case 'domain':
      return { key: 'contact.orgMatchDomain', values: { domain: reason.domain } };
    case 'phone':
      return { key: 'contact.orgMatchPhone', values: { phone: formatPhone(reason.phone) } };
  }
}

/** 「組織に登録」でこの人のほかにつながる人数（下見のつながる人数から本人を引く）。 */
export function othersLinked(linked: number): number {
  return Math.max(0, linked - 1);
}

/** 組織欄の状態の色（`null` は色なし）。 */
export type OrgFieldStatus = 'similar' | 'unlinked' | null;

/**
 * 組織欄の状態。カードにつながっていない会社名（`unlinked`）のうち、似た組織の候補があれば
 * 黄（`similar`）、無ければ赤（`unlinked`）。候補を取得中（`matchCount` が null）の間は色なし
 * （赤から黄へちらつかないように）。会社名が空・カードにつながっているときも色なし。
 */
export function orgFieldStatus(unlinked: boolean, matchCount: number | null): OrgFieldStatus {
  if (!unlinked || matchCount === null) return null;
  return matchCount > 0 ? 'similar' : 'unlinked';
}

/** 似た組織の結果がどの会社名・どの人のものかを表すキー（古い結果で色を付けないため）。 */
export function orgMatchKey(name: string, contactId: number | null): string {
  return `${contactId ?? 'new'}|${name.trim()}`;
}
