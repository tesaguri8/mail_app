import { describe, it, expect } from 'vitest';
import { dateBounds, matchesDate, type DateRange } from './DateFilter';

/** DB と同じ判定（会話の最後のやり取りの epoch 秒が since 以上・until 未満。日時が無ければ外す）。 */
function inBounds(dateStr: string | null, df: DateRange | null): boolean {
  const b = dateBounds(df);
  if (!b) return true;
  if (!dateStr) return false;
  const ts = Math.floor(new Date(dateStr).getTime() / 1000);
  return (b.since == null || ts >= b.since) && (b.until == null || ts < b.until);
}

describe('dateBounds（期間の絞り込みを DB の境目にする）', () => {
  // 端末ローカルの日時（時間帯に依らない）
  const local = (s: string) => new Date(s).toISOString();
  const dates = [
    null,
    local('2026-11-04T23:59:59'),
    local('2026-11-05T00:00:00'),
    local('2026-11-05T12:00:00'),
    local('2026-11-07T23:59:59'),
    local('2026-11-08T00:00:00'),
  ];
  const ranges: (DateRange | null)[] = [
    null,
    { mode: 'after', start: '2026-11-05', end: '' },
    { mode: 'before', start: '', end: '2026-11-07' },
    { mode: 'range', start: '2026-11-05', end: '2026-11-07' },
    { mode: 'range', start: '', end: '' },
    // 片方の端しか使わないモードでは、もう片方の値を見ない
    { mode: 'after', start: '2026-11-05', end: '2026-11-05' },
    { mode: 'before', start: '2026-11-07', end: '2026-11-07' },
  ];
  it('どの期間・どの日時でも matchesDate と同じ結果になる', () => {
    for (const df of ranges) {
      for (const d of dates) {
        expect([JSON.stringify(df), d, inBounds(d, df)]).toEqual([
          JSON.stringify(df),
          d,
          matchesDate(d, df),
        ]);
      }
    }
  });
  it('境目は日の始まり（until はその次の日の始まり）', () => {
    const b = dateBounds({ mode: 'range', start: '2026-11-05', end: '2026-11-07' });
    expect(b).toEqual({
      since: new Date('2026-11-05T00:00:00').getTime() / 1000,
      until: new Date('2026-11-08T00:00:00').getTime() / 1000,
    });
    expect(dateBounds(null)).toBeNull();
  });
});
