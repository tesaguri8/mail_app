/**
 * 日ごとの件数（日付のつまみの元になる数え）と、会話のバブルとの対応（純粋な計算だけ）。
 *
 * `DayCount` は Photosky の src/renderer/components/timeline/gridWindow.ts（962b00c）から写した。
 * Photosky は日ごとの件数を DB で数えるが、会話は threadView で全メールを読み込み済みなので、
 * 手元で会話の並び順のまま数える。撮影日不明（Photosky の UNKNOWN_DAY）にあたる行は持たない。
 * 設計: docs/CONVERSATION_TIMELINE.md
 */

/** 日ごとの件数。並びは一覧（会話）の並び順どおり。 */
export interface DayCount {
  /** 端末ローカルの `YYYY-MM-DD` */
  dateKey: string;
  count: number;
}

/** 日時の文字列を端末ローカルの `YYYY-MM-DD` にする。読めなければ null。 */
export function localDayKey(date: string | null): string | null {
  if (!date) return null;
  const d = new Date(date);
  if (Number.isNaN(d.getTime())) return null;
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/**
 * 並び順のまま、同じ日が続くところを 1 つに数える（i 通目 = 累計の i 件目になる）。
 *
 * 日時の無い・読めないメールは件数から落とさず、直前のメールの日に数える（先頭なら最初に
 * 日の分かるメールの日）。落とすと「何通目」とバブルの番号がずれるため。
 * 日の分かるメールが 1 通も無ければ空。
 */
export function countByLocalDay(dates: readonly (string | null)[]): DayCount[] {
  const keys = dates.map(localDayKey);
  const first = keys.find((k): k is string => k !== null);
  if (first === undefined) return [];
  const days: DayCount[] = [];
  let current = first;
  for (const key of keys) {
    if (key !== null) current = key;
    const last = days[days.length - 1];
    if (last && last.dateKey === current) last.count += 1;
    else days.push({ dateKey: current, count: 1 });
  }
  return days;
}

/**
 * いま一番上に見えているバブルの番号。`tops` は各バブルの上端（スクロール域の中身の先頭からの
 * px・並び順に昇順）、`scrollTop` はスクロール量。上端が scrollTop 以下になる最後のバブル
 * （先頭より上なら 0）。バブルが無ければ null。
 */
export function topVisibleIndex(tops: readonly number[], scrollTop: number): number | null {
  if (tops.length === 0) return null;
  let lo = 0;
  let hi = tops.length - 1;
  let found = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (tops[mid] <= scrollTop) {
      found = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return found;
}

/**
 * 各バブルの始まりの位置を、スクロールできる全体の高さに対する割合（0〜1）にする。
 * `tops` は各バブルの上端（スクロール域の中身の先頭からの px）。高さが無ければ空。
 */
export function startFractions(tops: readonly number[], scrollHeight: number): number[] {
  if (scrollHeight <= 0) return [];
  return tops.map((top) => Math.min(1, Math.max(0, top / scrollHeight)));
}

/**
 * 全体に対する割合 fraction の位置を表示域の上端に合わせるスクロール量（`gap` だけ上に余白を残し、
 * 0 〜 最大で頭打ち）。
 */
export function scrollTopForFraction(
  fraction: number,
  scrollHeight: number,
  maxScroll: number,
  gap = 0
): number {
  return Math.min(Math.max(0, maxScroll), Math.max(0, fraction * scrollHeight - gap));
}
