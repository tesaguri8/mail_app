import { describe, expect, it } from 'vitest';
import { summarizeMarks } from './marks';

// 端末のタイムゾーンに左右されないよう、日時はオフセット無し（ローカル時刻）で書く。
describe('summarizeMarks', () => {
  it('バブルと枠を並び順のまま数え、件数・上端・枠かどうかの並びをそろえる', () => {
    const got = summarizeMarks([
      { top: 0, date: '2026-10-12T10:00:00', quote: false },
      { top: 120, date: '2026-10-09T15:29:01', quote: true },
      { top: 200, date: '2026-10-07T19:36:00', quote: true },
      { top: 260, date: null, quote: true },
      { top: 300, date: '2026-10-02T14:50:00', quote: true },
    ]);
    expect(got.days).toEqual([
      { dateKey: '2026-10-12', count: 1 },
      { dateKey: '2026-10-09', count: 1 },
      // 日時の読めない件は直前の日に寄せる（件数は落とさない）
      { dateKey: '2026-10-07', count: 2 },
      { dateKey: '2026-10-02', count: 1 },
    ]);
    expect(got.tops).toEqual([0, 120, 200, 260, 300]);
    expect(got.quotes).toEqual([false, true, true, true, true]);
    expect(got.days.reduce((n, d) => n + d.count, 0)).toBe(got.tops.length);
  });
});
