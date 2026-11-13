import { describe, expect, it } from 'vitest';
import type { GcalFullResync } from '@bindings/GcalFullResync';
import { resyncHadDrift, resyncVars } from './gcalResync';

const base: GcalFullResync = {
  calendar_id: 5,
  reason: 'fresh',
  fetched: 12,
  inserted: 1,
  restored: 5,
  updated: 0,
  deleted: 2,
  removed: 1,
  uncancelled: 0,
};

describe('resyncVars', () => {
  it('消したは Google 側の削除と、返ってこなかった分を合わせる', () => {
    expect(resyncVars(base)).toEqual({
      fetched: 12,
      inserted: 1,
      restored: 5,
      updated: 0,
      removed: 3,
    });
  });
});

describe('resyncHadDrift', () => {
  it('戻した・返ってこなかった・1 回だけの削除を外したがあれば食い違い', () => {
    expect(resyncHadDrift(base)).toBe(true);
    expect(resyncHadDrift({ ...base, restored: 0, removed: 0 })).toBe(false);
    expect(resyncHadDrift({ ...base, restored: 0, removed: 0, uncancelled: 1 })).toBe(true);
  });
});
