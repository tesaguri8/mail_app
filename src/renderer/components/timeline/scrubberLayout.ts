/**
 * 日付のつまみ（会話の左の縦の線）の配置の計算（純粋な計算だけ。React に依存しない）。
 *
 * Photosky の src/renderer/components/timeline/scrubberLayout.ts（962b00c）から写した。
 * メールには撮影日不明にあたる行が無いので、その区間・目盛りは落とした。
 *
 * - 長さはメールの通数に比例（スクロールバーと同じ感覚）。メールの無い期間は詰まる
 * - 目盛りは普段は年だけ。マウスに近いところだけ 月 → 週 → 日 と細かくなる
 * 設計: docs/CONVERSATION_TIMELINE.md
 */
import type { DayCount } from './dayCounts';

/** 線の上の 1 日分の区間 */
export interface Segment {
  dateKey: string;
  count: number;
  /** 一覧の何件目からか */
  start: number;
  /** 線の上の位置（px。y0 が上） */
  y0: number;
  y1: number;
}

/** 日ごとの件数を、高さ height の線の上に並べる（件数に比例）。 */
export function layoutSegments(days: readonly DayCount[], height: number): Segment[] {
  const valid = days.filter((d) => d.count > 0);
  const total = valid.reduce((sum, d) => sum + d.count, 0);
  if (total === 0 || height <= 0) return [];
  const segments: Segment[] = [];
  let offset = 0;
  for (const day of valid) {
    const y0 = (height * offset) / total;
    const y1 = (height * (offset + day.count)) / total;
    segments.push({ dateKey: day.dateKey, count: day.count, start: offset, y0, y1 });
    offset += day.count;
  }
  return segments;
}

/** 位置 y にある区間の番号（線の外は端の区間）。区間が無ければ -1 */
export function segmentIndexAtY(segments: readonly Segment[], y: number): number {
  if (segments.length === 0) return -1;
  let lo = 0;
  let hi = segments.length - 1;
  // y0 <= y となる最後の区間
  let found = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (segments[mid].y0 <= y) {
      found = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return found;
}

/** 位置 y が指す一覧の何件目か（区間の中は件数で按分） */
export function offsetAtY(segments: readonly Segment[], y: number): number {
  const i = segmentIndexAtY(segments, y);
  if (i < 0) return 0;
  const seg = segments[i];
  const h = seg.y1 - seg.y0;
  const frac = h > 0 ? Math.min(1, Math.max(0, (y - seg.y0) / h)) : 0;
  return seg.start + Math.min(seg.count - 1, Math.floor(frac * seg.count));
}

/** 一覧の offset 件目の、線の上の位置（いま見ている位置の印） */
export function yOfOffset(segments: readonly Segment[], offset: number): number {
  if (segments.length === 0) return 0;
  let lo = 0;
  let hi = segments.length - 1;
  let found = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (segments[mid].start <= offset) {
      found = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  const seg = segments[found];
  const frac = Math.min(1, Math.max(0, (offset - seg.start) / seg.count));
  return seg.y0 + (seg.y1 - seg.y0) * frac;
}

export type TickLevel = 'year' | 'month' | 'week' | 'day';

export interface Tick {
  y: number;
  level: TickLevel;
  /** その目盛りが指す日（区間の先頭の日） */
  dateKey: string;
}

/**
 * 魚眼（マウスの近くだけ拡大して見せる）。線の上の位置 y を、マウス focus の近くで引き伸ばした
 * 表示の位置へ写す（Sarkar–Brown の魚眼。focus の位置そのものは動かない）。
 * 上下それぞれ線の端（0 / height）を越えないよう、半径を端までで打ち切る
 */
export function fisheye(
  y: number,
  focus: number,
  radius: number,
  magnification: number,
  height: number
): number {
  const d = y - focus;
  const r = d < 0 ? Math.min(radius, focus) : Math.min(radius, height - focus);
  if (r <= 0 || Math.abs(d) >= r || magnification <= 0) return y;
  const x = Math.abs(d) / r;
  const g = ((magnification + 1) * x) / (magnification * x + 1);
  return focus + Math.sign(d) * g * r;
}

/**
 * 魚眼の倍率。マウスのそばで 1 週間がおおよそ targetWeekPx になるように決める
 * （メールの多い会話ほど 1 週間が細いので、強く拡大する）
 */
export function lensMagnification(
  segments: readonly Segment[],
  targetWeekPx = 24,
  max = 60
): number {
  if (segments.length === 0) return 0;
  const weeks = new Set(segments.map((s) => weekKeyOf(s.dateKey))).size;
  const span = segments[segments.length - 1].y1 - segments[0].y0;
  const weekPx = span / weeks;
  if (weekPx <= 0) return 0;
  return Math.min(max, Math.max(0, Math.ceil(targetWeekPx / weekPx) - 1));
}

export interface TickOptions {
  /** マウスからこの距離（px）より近い目盛りだけ、月を出す */
  monthRadius: number;
  /** 週を出す距離 */
  weekRadius: number;
  /** 日を出す距離 */
  dayRadius: number;
  /** 目盛りどうしの最小の間隔（重なる目盛りは、粗いほう・マウスに近いほうを残す） */
  minGap: number;
  /** 魚眼の半径（px） */
  lensRadius: number;
}

export const DEFAULT_TICK_OPTIONS: TickOptions = {
  monthRadius: 160,
  weekRadius: 64,
  dayRadius: 28,
  minGap: 14,
  lensRadius: 180,
};

/** その日を含む週（日曜はじまり）の日曜の日付キー */
export function weekKeyOf(dateKey: string): string {
  const [y, m, d] = dateKey.split('-').map(Number);
  const t = Date.UTC(y, m - 1, d);
  const dow = new Date(t).getUTCDay();
  const s = new Date(t - dow * 86_400_000);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${s.getUTCFullYear()}-${pad(s.getUTCMonth() + 1)}-${pad(s.getUTCDate())}`;
}

const BUCKET_OF: Record<TickLevel, (key: string) => string> = {
  year: (k) => k.slice(0, 4),
  month: (k) => k.slice(0, 7),
  week: weekKeyOf,
  day: (k) => k,
};

/**
 * 目盛りを選ぶ。年は常に、月・週・日はマウス（pointerY）に近いところだけ出す。
 * 目盛りの位置はその年・月・週・日が線の上で始まるところ（並び順で最初に現れる区間の上端）を、
 * マウスの近くで魚眼に引き伸ばした位置（magnification = 0 なら引き伸ばさない）。
 * マウスの真下の位置は引き伸ばしても動かないので、押したときに飛ぶ先（offsetAtY）とずれない
 */
export function buildTicks(
  segments: readonly Segment[],
  pointerY: number | null,
  height: number,
  magnification: number,
  options: TickOptions = DEFAULT_TICK_OPTIONS
): Tick[] {
  const place = (y: number) =>
    pointerY === null ? y : fisheye(y, pointerY, options.lensRadius, magnification, height);
  const accepted: Tick[] = [];
  const fits = (y: number) => accepted.every((t) => Math.abs(t.y - y) >= options.minGap);

  const levels: { level: TickLevel; radius: number }[] = [
    { level: 'year', radius: Infinity },
    { level: 'month', radius: options.monthRadius },
    { level: 'week', radius: options.weekRadius },
    { level: 'day', radius: options.dayRadius },
  ];
  for (const { level, radius } of levels) {
    if (pointerY === null && level !== 'year') break;
    const bucketOf = BUCKET_OF[level];
    const candidates: Tick[] = [];
    let prev: string | null = null;
    for (const seg of segments) {
      const bucket = bucketOf(seg.dateKey);
      if (bucket !== prev) {
        const y = place(seg.y0);
        if (pointerY === null || Math.abs(y - pointerY) <= radius) {
          candidates.push({ y, level, dateKey: seg.dateKey });
        }
        prev = bucket;
      }
    }
    // マウスに近いものから置く（重なったら近いほうが残る）
    if (pointerY !== null) {
      candidates.sort((a, b) => Math.abs(a.y - pointerY) - Math.abs(b.y - pointerY));
    }
    for (const c of candidates) {
      if (fits(c.y)) accepted.push(c);
    }
  }
  return accepted.sort((a, b) => a.y - b.y);
}

/** 線の上の範囲（表示の位置。px） */
export interface Band {
  y0: number;
  y1: number;
}

/**
 * マウス位置の日を含む**年**と**月**の範囲（魚眼を当てた表示の位置）。帯を敷いて、いまどの年・月の
 * 中かを見せる。区間は並び順で同じ年・月が連続するので、最初と最後の区間の端をとる。
 * マウスが無ければ null
 */
export function parentBands(
  segments: readonly Segment[],
  pointerY: number | null,
  height: number,
  magnification: number,
  options: TickOptions = DEFAULT_TICK_OPTIONS
): { year: Band; month: Band } | null {
  if (pointerY === null || segments.length === 0) return null;
  const hovered = segments[segmentIndexAtY(segments, pointerY)];
  const place = (y: number) => fisheye(y, pointerY, options.lensRadius, magnification, height);
  const bandOf = (prefix: string): Band => {
    const inside = segments.filter((s) => s.dateKey.startsWith(prefix));
    const y0 = Math.min(...inside.map((s) => s.y0));
    const y1 = Math.max(...inside.map((s) => s.y1));
    return { y0: place(y0), y1: place(y1) };
  };
  return {
    year: bandOf(hovered.dateKey.slice(0, 4)),
    month: bandOf(hovered.dateKey.slice(0, 7)),
  };
}
