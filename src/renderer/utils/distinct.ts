// 重複の整理の「別人」の記録まわり（docs/CONTACT_MODEL.md §1-5）。

/** 統合でチェックを外した人（組にいたが、統合に含めなかった人）。統合後の 1 人と「別人」として
 *  記録し、次から同じ組に出さない。 */
export const excludedFromMerge = (memberIds: number[], included: ReadonlySet<number>): number[] =>
  memberIds.filter((id) => !included.has(id));

/** 「別人」の記録を組にまとめたもの（取り消しの入口の 1 行）。 */
export type DistinctGroup = {
  /** 組の人（ID と名前。並びは記録順）。 */
  members: { id: number; name: string }[];
  /** 組を作っている対（戻すときに全部消す）。 */
  pairs: { a: number; b: number }[];
};

/** 対の記録を、つながった人どうしの組にまとめる。5 人の組を「別人」にすると対は 10 になるが、
 *  利用者が判断したのは 1 組なので、入口にも 1 行で出す。 */
export function groupDistinctPairs(
  pairs: { a_id: number; a_name: string; b_id: number; b_name: string }[]
): DistinctGroup[] {
  const parent = new Map<number, number>();
  const find = (x: number): number => {
    let r = x;
    while (parent.get(r) !== r) r = parent.get(r) ?? r;
    parent.set(x, r);
    return r;
  };
  const names = new Map<number, string>();
  for (const p of pairs) {
    for (const [id, name] of [
      [p.a_id, p.a_name],
      [p.b_id, p.b_name],
    ] as const) {
      if (!parent.has(id)) parent.set(id, id);
      names.set(id, name);
    }
    parent.set(find(p.a_id), find(p.b_id));
  }
  const groups = new Map<number, DistinctGroup>();
  for (const p of pairs) {
    const root = find(p.a_id);
    const g = groups.get(root) ?? { members: [], pairs: [] };
    for (const id of [p.a_id, p.b_id]) {
      if (!g.members.some((m) => m.id === id)) g.members.push({ id, name: names.get(id) ?? '' });
    }
    g.pairs.push({ a: p.a_id, b: p.b_id });
    groups.set(root, g);
  }
  return [...groups.values()];
}
