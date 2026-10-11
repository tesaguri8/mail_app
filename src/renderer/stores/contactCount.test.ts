import { describe, expect, it } from 'vitest';
import { contactCountLabel } from './contactCount';

describe('contactCountLabel', () => {
  it('絞り込みが無ければ総数だけ', () => {
    expect(contactCountLabel({ shown: 3005, total: 3005, trash: false })).toEqual({
      key: 'contact.listTotal',
      values: { total: (3005).toLocaleString() },
    });
  });

  it('絞り込んでいれば「出している数 / 総数」', () => {
    expect(contactCountLabel({ shown: 120, total: 3005, trash: false })).toEqual({
      key: 'contact.listTotalOf',
      values: { shown: '120', total: (3005).toLocaleString() },
    });
  });

  it('ゴミ箱はゴミ箱の件数（生きている人の総数は使わない）', () => {
    expect(contactCountLabel({ shown: 12, total: 3005, trash: true })).toEqual({
      key: 'contact.trashTotal',
      values: { total: '12' },
    });
  });
});
