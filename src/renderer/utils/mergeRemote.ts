// 統合で Google 側から消す件数を、確認画面の文に組み立てる（docs/CONTACTS_SYNC.md §3-5）。

import type { MergeRemoteDeletion } from '@bindings/MergeRemoteDeletion';

/** 翻訳関数（i18next の t と同じ呼び方）。 */
type Translate = (key: string, opts?: Record<string, unknown>) => string;

/** 消す件数の合計。 */
export const remoteDeletionTotal = (ds: MergeRemoteDeletion[]): number =>
  ds.reduce((n, d) => n + d.count, 0);

/**
 * 確認画面の補足: アカウントごとに「Google 連絡先からも N 件削除して 1 件にまとめます（アカウント名）」、
 * 最後に「Google のゴミ箱から 30 日は戻せる」。消すものが無ければ空。`lineKey` で 1 行目の文言を
 * 差し替えられる（まとめて統合では「各組 1 件に」）。
 */
export function remoteDeletionNotes(
  ds: MergeRemoteDeletion[],
  t: Translate,
  lineKey = 'dupes.googleDeleteLine'
): string[] {
  const lines = ds
    .filter((d) => d.count > 0)
    .map((d) => t(lineKey, { count: d.count, account: d.account_label }));
  return lines.length > 0 ? [...lines, t('dupes.googleTrashNote')] : [];
}
