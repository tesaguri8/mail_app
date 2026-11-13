// 「最初から同期し直す」の結果を、画面の文言の値にする（docs/CALENDAR_SYNC.md）。

import type { GcalFullResync } from '@bindings/GcalFullResync';

/** 文言に渡す値。「消した」は Google 側の削除の取り込みと、返ってこなかった分を合わせる。 */
export const resyncVars = (r: GcalFullResync) => ({
  fetched: r.fetched,
  inserted: r.inserted,
  restored: r.restored,
  updated: r.updated,
  removed: r.deleted + r.removed,
});

/** 手元と Google が食い違っていた（戻した・返ってこなかったので消した・1 回だけの削除を外した）か。 */
export const resyncHadDrift = (r: GcalFullResync) => r.restored + r.removed + r.uncancelled > 0;
