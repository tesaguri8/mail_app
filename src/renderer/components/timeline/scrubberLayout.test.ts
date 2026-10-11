import { describe, expect, it } from 'vitest';
import type { DayCount } from './dayCounts';
import {
  buildTicks,
  fisheye,
  layoutSegments,
  lensMagnification,
  offsetAtY,
  parentBands,
  segmentIndexAtY,
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
