import { describe, expect, it } from 'vitest';
import { topVisibleIndex, type DayCount } from './dayCounts';
import {
  buildDayLabels,
  buildTicks,
  fisheye,
  layoutSegments,
  layoutSegmentsAt,
  lensMagnification,
  offsetAtY,
  parentBands,
  segmentIndexAtY,
  snapToNearest,
  weekKeyOf,
  yOfOffset,
} from './scrubberLayout';

const days: DayCount[] = [
  { dateKey: '2025-12-30', count: 2 },
  { dateKey: '2026-01-05', count: 1 },
  { dateKey: '2026-02-10', count: 5 },
  { dateKey: '2026-02-11', count: 2 },
];

describe('layoutSegments', () => {
  it('件数に比例して線の上に並べる', () => {
    const segs = layoutSegments(days, 100);
    expect(segs.map((s) => [s.start, s.y0, s.y1])).toEqual([
      [0, 0, 20],
      [2, 20, 30],
      [3, 30, 80],
      [8, 80, 100],
    ]);
  });

  it('件数 0 の日は落とし、全体が 0 件か高さが無ければ空', () => {
    expect(layoutSegments([{ dateKey: '2026-01-01', count: 0 }, ...days], 100)).toHaveLength(4);
    expect(layoutSegments([], 100)).toEqual([]);
    expect(layoutSegments(days, 0)).toEqual([]);
  });
});

describe('位置 ↔ 何件目', () => {
  const segs = layoutSegments(days, 100);

  it('位置の区間（線の外は端の区間）', () => {
    expect(segmentIndexAtY(segs, -5)).toBe(0);
    expect(segmentIndexAtY(segs, 25)).toBe(1);
    expect(segmentIndexAtY(segs, 500)).toBe(3);
    expect(segmentIndexAtY([], 10)).toBe(-1);
  });

  it('区間の中は件数で按分する', () => {
    expect(offsetAtY(segs, 0)).toBe(0);
    expect(offsetAtY(segs, 19)).toBe(1);
    expect(offsetAtY(segs, 30)).toBe(3);
    expect(offsetAtY(segs, 79)).toBe(7);
    expect(offsetAtY(segs, 100)).toBe(9);
  });

  it('何件目 → 位置 → 何件目 が戻る', () => {
    for (let offset = 0; offset < 10; offset++) {
      expect(offsetAtY(segs, yOfOffset(segs, offset))).toBe(offset);
    }
  });
});

describe('魚眼', () => {
  it('焦点は動かず、単調に増え、端を越えない', () => {
    const ys = Array.from({ length: 101 }, (_, i) => fisheye(i, 40, 30, 5, 100));
    expect(fisheye(40, 40, 30, 5, 100)).toBe(40);
    ys.slice(1).forEach((y, i) => expect(y).toBeGreaterThanOrEqual(ys[i]));
    expect(Math.min(...ys)).toBeGreaterThanOrEqual(0);
    expect(Math.max(...ys)).toBeLessThanOrEqual(100);
  });

  it('倍率 0 なら動かさない', () => {
    expect(fisheye(35, 40, 30, 0, 100)).toBe(35);
  });

  it('倍率は 1 週間の幅が狭いほど大きく、上限で止まる', () => {
    expect(lensMagnification(layoutSegments(days, 1000))).toBe(0);
    expect(lensMagnification(layoutSegments(days, 10))).toBeGreaterThan(0);
    expect(lensMagnification(layoutSegments(days, 0.01))).toBe(60);
  });
});

describe('weekKeyOf', () => {
  it('その週の日曜', () => {
    expect(weekKeyOf('2026-10-07')).toBe('2026-10-04');
    expect(weekKeyOf('2026-10-04')).toBe('2026-10-04');
    expect(weekKeyOf('2026-01-01')).toBe('2025-12-28');
  });
});

describe('buildTicks', () => {
  const segs = layoutSegments(days, 400);

  it('マウスが無ければ年だけ', () => {
    expect(buildTicks(segs, null, 400, 0).map((t) => [t.level, t.dateKey])).toEqual([
      ['year', '2025-12-30'],
      ['year', '2026-01-05'],
    ]);
  });

  it('マウスの近くでは月・日も出し、重ならない', () => {
    // 区間の上端: 2025-12-30=0, 2026-01-05=80, 2026-02-10=120, 2026-02-11=320
    const nearMonth = buildTicks(segs, 130, 400, 0);
    expect(nearMonth.some((t) => t.level === 'month' && t.dateKey === '2026-02-10')).toBe(true);
    const nearDay = buildTicks(segs, 315, 400, 0);
    expect(nearDay.some((t) => t.level === 'day' && t.dateKey === '2026-02-11')).toBe(true);
    // 遠い日は出さない
    expect(nearMonth.some((t) => t.level === 'day' && t.dateKey === '2026-02-11')).toBe(false);
    for (const ticks of [nearMonth, nearDay]) {
      ticks.slice(1).forEach((t, i) => expect(t.y - ticks[i].y).toBeGreaterThanOrEqual(14));
    }
  });
});

describe('parentBands', () => {
  const segs = layoutSegments(days, 100);

  it('マウス位置の日を含む年・月の範囲', () => {
    expect(parentBands(segs, 50, 100, 0)).toEqual({
      year: { y0: 20, y1: 100 },
      month: { y0: 30, y1: 100 },
    });
  });

  it('マウスが無ければ null', () => {
    expect(parentBands(segs, null, 100, 0)).toBeNull();
  });
});

describe('layoutSegmentsAt', () => {
  // 10 件。各件の始まりの割合（バブルの高さがまちまち）
  const starts = [0, 0.05, 0.3, 0.32, 0.34, 0.36, 0.38, 0.4, 0.7, 0.9];

  it('区間の上端はその日の最初の件の始まり、下端は次の区間の上端', () => {
    const segs = layoutSegmentsAt(days, starts, 100);
    expect(segs.map((s) => [s.start, s.y0, s.y1])).toEqual([
      [0, 0, 30],
      [2, 30, 32],
      [3, 32, 70],
      [8, 70, 100],
    ]);
  });

  it('始まりが件数に足りなければ件数に比例する配置に戻す', () => {
    expect(layoutSegmentsAt(days, [0, 0.5], 100)).toEqual(layoutSegments(days, 100));
  });

  it('逆転した始まりは前の区間より上へ戻さない', () => {
    const segs = layoutSegmentsAt(days, [0, 0, 0.5, 0.4, 0, 0, 0, 0, 0.2, 0], 100);
    expect(segs.map((s) => s.y0)).toEqual([0, 50, 50, 50]);
  });

  it('全体が 0 件か高さが無ければ空', () => {
    expect(layoutSegmentsAt([], starts, 100)).toEqual([]);
    expect(layoutSegmentsAt(days, starts, 0)).toEqual([]);
  });
});

describe('buildDayLabels', () => {
  const seg = (dateKey: string, y0: number) => ({ dateKey, count: 1, start: 0, y0, y1: y0 });

  it('各日の始まりに札。最初と年が変わるところは年も出す', () => {
    const labels = buildDayLabels(
      [seg('2025-12-30', 0), seg('2026-01-05', 40), seg('2026-01-06', 80)],
      14
    );
    expect(labels.map((l) => [l.dateKey, l.withYear])).toEqual([
      ['2025-12-30', true],
      ['2026-01-05', true],
      ['2026-01-06', false],
    ]);
  });

  it('詰まったら 年の変わり目 → 月の変わり目 → 上のほう の順に残す', () => {
    const labels = buildDayLabels(
      [
        seg('2025-12-30', 0),
        seg('2025-12-31', 20),
        seg('2026-01-01', 25), // 年が変わる: 12/31 を押しのける
        seg('2026-01-02', 50), // 2/1 に押しのけられる
        seg('2026-01-03', 80),
        seg('2026-01-04', 85), // 同じ順位なら上の 1/3 が残る
        seg('2026-02-01', 60), // 月が変わる: 1/2 より優先
      ],
      14
    );
    expect(labels.map((l) => l.dateKey)).toEqual([
      '2025-12-30',
      '2026-01-01',
      '2026-02-01',
      '2026-01-03',
    ]);
  });
});

describe('snapToNearest', () => {
  it('半径以内の最も近い位置へ吸い付ける', () => {
    expect(snapToNearest([10, 30, 50], 26, 6)).toBe(30);
    expect(snapToNearest([10, 30, 50], 12, 6)).toBe(10);
  });

  it('半径の外ならそのまま', () => {
    expect(snapToNearest([10, 30, 50], 20, 6)).toBe(20);
    expect(snapToNearest([], 20, 6)).toBe(20);
  });
});

// 会話のつまみには、バブルの下の未受信の引用の枠も載る。枠は転送の中の新しい順に並ぶので、
// 日付が逆に進む区間ができる（例: 10/12 のバブル → 10/9・10/7・…・10/2 の枠 → 10/13 のバブル）。
describe('日付が逆に進む並び（転送の中の引用）', () => {
  const reversed: DayCount[] = [
    { dateKey: '2026-10-12', count: 1 },
    { dateKey: '2026-10-09', count: 1 },
    { dateKey: '2026-10-07', count: 1 },
    { dateKey: '2026-10-02', count: 2 },
    { dateKey: '2026-10-13', count: 1 },
  ];
  const starts = [0, 0.1, 0.3, 0.5, 0.6, 0.8];

  it('区間は位置どおりに並び、日付の順には並べ替えない', () => {
    const segs = layoutSegmentsAt(reversed, starts, 100);
    expect(segs.map((s) => [s.dateKey, s.start, s.y0, s.y1])).toEqual([
      ['2026-10-12', 0, 0, 10],
      ['2026-10-09', 1, 10, 30],
      ['2026-10-07', 2, 30, 50],
      ['2026-10-02', 3, 50, 80],
      ['2026-10-13', 5, 80, 100],
    ]);
  });

  it('札は各日に 1 つ・その日の最初の件の番号つき。年が戻るところにも年を付ける', () => {
    const segs = layoutSegmentsAt(reversed, starts, 100);
    expect(buildDayLabels(segs, 5).map((l) => [l.dateKey, l.start, l.withYear])).toEqual([
      ['2026-10-12', 0, true],
      ['2026-10-09', 1, false],
      ['2026-10-07', 2, false],
      ['2026-10-02', 3, false],
      ['2026-10-13', 5, false],
    ]);
    const acrossYear = layoutSegmentsAt(
      [
        { dateKey: '2026-01-05', count: 1 },
        { dateKey: '2025-12-26', count: 1 },
        { dateKey: '2026-01-06', count: 1 },
      ],
      [0, 0.4, 0.8],
      100
    );
    expect(buildDayLabels(acrossYear, 5).map((l) => [l.dateKey, l.withYear])).toEqual([
      ['2026-01-05', true],
      ['2025-12-26', true],
      ['2026-01-06', true],
    ]);
  });

  it('月の帯は、マウスの区間から同じ月が続くところまで（離れた同じ月はつなげない）', () => {
    const segs = layoutSegmentsAt(
      [
        { dateKey: '2026-10-12', count: 1 },
        { dateKey: '2026-09-30', count: 1 },
        { dateKey: '2026-10-13', count: 1 },
      ],
      [0, 0.4, 0.8],
      100
    );
    expect(parentBands(segs, 90, 100, 0)).toEqual({
      year: { y0: 0, y1: 100 },
      month: { y0: 80, y1: 100 },
    });
  });

  it('押した位置の件は、位置の並び（昇順）で引ける', () => {
    expect(topVisibleIndex(starts, 0.55)).toBe(3);
    expect(topVisibleIndex(starts, 0.85)).toBe(5);
  });
});
