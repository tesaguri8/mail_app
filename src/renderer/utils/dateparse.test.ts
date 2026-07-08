import { describe, it, expect } from 'vitest';
import { parseDateTime, matchDates } from './dateparse';

const BASE = '2026-07-08T09:00'; // メール受信日の基準（年推定に使う）

describe('parseDateTime', () => {
  it('和暦風の「YYYY年M月D日」を解釈する', () => {
    expect(parseDateTime('2026年7月10日', BASE)).toEqual({ day: '2026-07-10', allDay: true });
  });

  it('年なしの「M月D日」は基準年を補う', () => {
    expect(parseDateTime('7月10日', BASE)).toEqual({ day: '2026-07-10', allDay: true });
  });

  it('曜日カッコ付きも解釈する', () => {
    expect(parseDateTime('7月10日（金）', BASE)).toEqual({ day: '2026-07-10', allDay: true });
  });

  it('スラッシュ表記 YYYY/M/D と M/D', () => {
    expect(parseDateTime('2026/7/10', BASE)).toEqual({ day: '2026-07-10', allDay: true });
    expect(parseDateTime('7/10', BASE)).toEqual({ day: '2026-07-10', allDay: true });
  });

  it('全角数字も解釈する', () => {
    expect(parseDateTime('７月１０日', BASE)).toEqual({ day: '2026-07-10', allDay: true });
  });

  it('時刻付き（HH:MM）は allDay=false + time', () => {
    expect(parseDateTime('7月10日 14:00', BASE)).toEqual({
      day: '2026-07-10',
      time: '14:00',
      allDay: false,
    });
  });

  it('「H時MM分」表記', () => {
    expect(parseDateTime('7月10日 14時30分', BASE)).toEqual({
      day: '2026-07-10',
      time: '14:30',
      allDay: false,
    });
  });

  it('「午後H時」は 24 時間へ変換', () => {
    expect(parseDateTime('7月10日 午後2時', BASE)).toEqual({
      day: '2026-07-10',
      time: '14:00',
      allDay: false,
    });
  });

  it('「午前12時」は 00:00', () => {
    expect(parseDateTime('7/10 午前12時', BASE)).toEqual({
      day: '2026-07-10',
      time: '00:00',
      allDay: false,
    });
  });

  it('実在しない日付（2/31）は null', () => {
    expect(parseDateTime('2月31日', BASE)).toBeNull();
  });

  it('月・日の範囲外は null', () => {
    expect(parseDateTime('13/40', BASE)).toBeNull();
  });
});

describe('matchDates', () => {
  it('本文中の日付スパンを位置つきで返す', () => {
    const text = 'では7月10日 14:00にお願いします。';
    const hits = matchDates(text, BASE);
    expect(hits).toHaveLength(1);
    expect(hits[0].raw).toBe('7月10日 14:00');
    expect(hits[0].parsed).toEqual({ day: '2026-07-10', time: '14:00', allDay: false });
    expect(text.slice(hits[0].index, hits[0].index + hits[0].raw.length)).toBe('7月10日 14:00');
  });

  it('複数の日付を検出する', () => {
    const hits = matchDates('7/10 と 8/1 の二日', BASE);
    expect(hits.map((h) => h.parsed.day)).toEqual(['2026-07-10', '2026-08-01']);
  });

  it('電話番号（03-1234-5678）を日付として誤検出しない', () => {
    expect(matchDates('お電話は 03-1234-5678 まで', BASE)).toHaveLength(0);
  });

  it('日付らしくない数字（分数 7/10点）の後続数字は拾うが、桁続きは除外', () => {
    // 「7/10」の直後が全角の文字なら拾える（点は区切りではない）
    expect(matchDates('達成率は7/10です', BASE)).toHaveLength(1);
  });
});
