import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { formatQuotedDateTime, parseQuotedDate } from './datetime';

/** バブルと同じ表記（formatDateTime の書式）で、端末の時刻として出したもの。 */
const local = (d: Date) =>
  d.toLocaleString(undefined, {
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });

// 時差のある日時を「ずらす」ことが見えるよう、端末を日本以外の時刻帯にする
const savedTz = process.env.TZ;
beforeAll(() => {
  process.env.TZ = 'America/New_York';
});
afterAll(() => {
  process.env.TZ = savedTz;
});

describe('formatQuotedDateTime', () => {
  it('時差のある日時は端末の時刻に直す', () => {
    expect(formatQuotedDateTime('2026-10-20T11:21:08+09:00', 'raw')).toBe(
      local(new Date(Date.UTC(2026, 9, 20, 2, 21, 8)))
    );
    expect(new Date('2026-10-20T11:21:08+09:00').getHours()).toBe(22);
  });

  it('時差の無い日時は、書かれたままの時刻で出す（ずらさない）', () => {
    expect(formatQuotedDateTime('2026-10-20T11:36:00', 'raw')).toBe(
      local(new Date(2026, 9, 20, 11, 36))
    );
    expect(formatQuotedDateTime('2026-10-02T12:10', 'raw')).toBe(
      local(new Date(2026, 9, 2, 12, 10))
    );
  });

  it('読めなければ元の文字列を返す', () => {
    expect(formatQuotedDateTime('', '2026/10/02 12:10')).toBe('2026/10/02 12:10');
    expect(formatQuotedDateTime('not a date', '元')).toBe('元');
  });
});

describe('parseQuotedDate', () => {
  it('時差の無い日時は端末の時刻として、時差のある日時はその時刻として読む', () => {
    expect(parseQuotedDate('2026-10-20T11:36:00')?.getTime()).toBe(
      new Date(2026, 9, 20, 11, 36).getTime()
    );
    expect(parseQuotedDate('2026-10-20T11:21:08+09:00')?.getTime()).toBe(
      Date.UTC(2026, 9, 20, 2, 21, 8)
    );
    expect(parseQuotedDate('')).toBeNull();
  });
});
