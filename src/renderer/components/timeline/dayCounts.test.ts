import { describe, expect, it } from 'vitest';
import {
  countByLocalDay,
  localDayKey,
  scrollTopForFraction,
  startFractions,
  topVisibleIndex,
} from './dayCounts';

// 端末のタイムゾーンに左右されないよう、日時はオフセット無し（ローカル時刻）で書く。
describe('localDayKey', () => {
  it('ローカルの年月日にする', () => {
    expect(localDayKey('2026-10-03T23:59:00')).toBe('2026-10-03');
    expect(localDayKey('2026-01-09T00:00:00')).toBe('2026-01-09');
  });

  it('無い・読めない日時は null', () => {
    expect(localDayKey(null)).toBeNull();
    expect(localDayKey('')).toBeNull();
    expect(localDayKey('not a date')).toBeNull();
  });
});

describe('countByLocalDay', () => {
  it('並び順のまま、同じ日が続くところを数える', () => {
    expect(
      countByLocalDay([
        '2026-10-01T09:00:00',
        '2026-10-01T18:00:00',
        '2026-10-03T08:00:00',
        '2026-11-02T08:00:00',
        '2026-11-02T09:00:00',
        '2026-11-02T10:00:00',
      ])
    ).toEqual([
      { dateKey: '2026-10-01', count: 2 },
      { dateKey: '2026-10-03', count: 1 },
      { dateKey: '2026-11-02', count: 3 },
    ]);
  });

  it('件数の合計は通数と同じ（日時の無いメールを落とさない）', () => {
    const dates = [null, '2026-10-01T09:00:00', 'bad', null, '2026-10-02T09:00:00'];
    const days = countByLocalDay(dates);
    expect(days.reduce((s, d) => s + d.count, 0)).toBe(dates.length);
    // 先頭の日時なしは最初に分かる日へ、途中の日時なしは直前の日へ数える
    expect(days).toEqual([
      { dateKey: '2026-10-01', count: 4 },
      { dateKey: '2026-10-02', count: 1 },
    ]);
  });

  it('並びが前後した日は、続いていなければ別に数える（offset をバブルの番号と揃えるため）', () => {
    expect(
      countByLocalDay(['2026-10-01T09:00:00', '2026-10-02T09:00:00', '2026-10-01T10:00:00'])
    ).toEqual([
      { dateKey: '2026-10-01', count: 1 },
      { dateKey: '2026-10-02', count: 1 },
      { dateKey: '2026-10-01', count: 1 },
    ]);
  });

  it('日の分かるメールが無ければ空', () => {
    expect(countByLocalDay([])).toEqual([]);
    expect(countByLocalDay([null, 'bad'])).toEqual([]);
  });
});

describe('topVisibleIndex', () => {
  const tops = [12, 200, 450, 900];

  it('上端がスクロール量以下になる最後のバブル', () => {
    expect(topVisibleIndex(tops, 0)).toBe(0);
    expect(topVisibleIndex(tops, 12)).toBe(0);
    expect(topVisibleIndex(tops, 199)).toBe(0);
    expect(topVisibleIndex(tops, 200)).toBe(1);
    expect(topVisibleIndex(tops, 5000)).toBe(3);
  });

  it('バブルが無ければ null', () => {
    expect(topVisibleIndex([], 0)).toBeNull();
  });
});

describe('startFractions', () => {
  it('上端を全体の高さに対する割合にする（0〜1 に収める）', () => {
    expect(startFractions([0, 250, 500, 1200], 1000)).toEqual([0, 0.25, 0.5, 1]);
    expect(startFractions([-5], 1000)).toEqual([0]);
  });

  it('高さが無ければ空', () => {
    expect(startFractions([0, 10], 0)).toEqual([]);
  });
});

describe('scrollTopForFraction', () => {
  it('割合の位置から余白を引いたスクロール量', () => {
    expect(scrollTopForFraction(0.25, 1000, 600, 12)).toBe(238);
    expect(scrollTopForFraction(0, 1000, 600, 12)).toBe(0);
  });

  it('最大スクロール量で頭打ち', () => {
    expect(scrollTopForFraction(0.9, 1000, 600, 12)).toBe(600);
    expect(scrollTopForFraction(0.5, 1000, -10, 0)).toBe(0);
  });

  it('バブルの始まりの割合へ飛ぶと、そのバブルが一番上に来る', () => {
    const tops = [12, 200, 450, 900];
    const fractions = startFractions(tops, 1500);
    const scrollTop = scrollTopForFraction(fractions[2], 1500, 10_000, 12);
    expect(topVisibleIndex(tops, scrollTop + 12)).toBe(2);
  });
});
