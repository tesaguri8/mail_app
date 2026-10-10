import { describe, expect, it } from 'vitest';
import { excludedFromMerge, groupDistinctPairs } from './distinct';

describe('統合でチェックを外した人', () => {
  it('組にいて、統合に含めなかった人だけ', () => {
    expect(excludedFromMerge([1, 2, 3, 4], new Set([1, 3]))).toEqual([2, 4]);
  });

  it('全員を含めたら誰も別人にしない', () => {
    expect(excludedFromMerge([1, 2], new Set([1, 2]))).toEqual([]);
  });
});

describe('「別人」の記録を組にまとめる', () => {
  const pair = (a: number, b: number) => ({ a_id: a, a_name: `n${a}`, b_id: b, b_name: `n${b}` });

  it('つながった対は 1 組（3 人を別人にした 3 対 → 1 組）', () => {
    const g = groupDistinctPairs([pair(1, 2), pair(1, 3), pair(2, 3)]);
    expect(g).toHaveLength(1);
    expect(g[0].members.map((m) => m.id).sort()).toEqual([1, 2, 3]);
    expect(g[0].pairs).toHaveLength(3);
  });

  it('別々の判断は別の組', () => {
    expect(groupDistinctPairs([pair(1, 2), pair(3, 4)])).toHaveLength(2);
    expect(groupDistinctPairs([])).toEqual([]);
  });
});
