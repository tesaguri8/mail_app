import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { formatQuoteHeaderDate, formatQuotedDateTime, parseQuotedDate } from './datetime';

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

/**
 * 返信の引用見出し・転送の見出しの日時。Rust 側の往復の試験（quotes/tests/compose_header.rs）が
 * 同じ文字列を Rondine の解析に通しているので、形を変えるなら両方をそろえる。
 */
describe('formatQuoteHeaderDate', () => {
  describe('端末が日本', () => {
    beforeAll(() => {
      process.env.TZ = 'Asia/Tokyo';
    });
    afterAll(() => {
      process.env.TZ = 'America/New_York';
    });

    it('日本語は "年/月/日(曜) 時:分 時差"、英語は Gmail と同じ形', () => {
      const d = '2026-10-14T03:50:00Z';
      expect(formatQuoteHeaderDate(d, 'ja')).toBe('2026/10/14(水) 12:50 +0900');
      expect(formatQuoteHeaderDate(d, 'en')).toBe('Wed, Oct 14, 2026 at 12:50 PM +0900');
    });

    it('英語の 0 時台は 12 AM、正午前は AM', () => {
      expect(formatQuoteHeaderDate('2026-10-13T15:05:00Z', 'en')).toBe(
        'Wed, Oct 14, 2026 at 12:05 AM +0900'
      );
      expect(formatQuoteHeaderDate('2026-10-14T00:07:00Z', 'en-US')).toBe(
        'Wed, Oct 14, 2026 at 9:07 AM +0900'
      );
    });
  });

  it('元のメールの時差ではなく、端末の時刻と時差で書く（夏時間も端末に従う）', () => {
    const d = '2026-10-14T12:50:00+09:00';
    expect(formatQuoteHeaderDate(d, 'ja')).toBe('2026/10/13(火) 23:50 -0400');
    expect(formatQuoteHeaderDate(d, 'en')).toBe('Tue, Oct 13, 2026 at 11:50 PM -0400');
    expect(formatQuoteHeaderDate('2026-12-01T03:05:00Z', 'ja')).toBe('2026/11/30(月) 22:05 -0500');
  });

  it('日時が無ければ空、読めなければ元の文字列のまま', () => {
    expect(formatQuoteHeaderDate(null, 'ja')).toBe('');
    expect(formatQuoteHeaderDate('not a date', 'en')).toBe('not a date');
  });
});
