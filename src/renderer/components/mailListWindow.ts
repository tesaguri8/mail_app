/**
 * メール一覧の「全期間ぶんの枠」と、飛んだ先だけを読むための計算（純粋な関数だけ）。
 *
 * 一覧はフォルダ内の全スレッドを最初から枠（slots）として持ち、読み込んだ位置だけに行が入る。
 * 枠の数と日付の位置は thread_day_counts（日ごとの件数）から決まり、累計がそのまま一覧の offset になる。
 * Photosky の §2「読み込んでいない日付へ飛ぶ仕組み」と同じ作り。設計: docs/MAIL_LIST_TIMELINE.md
 */
import type { ThreadDayCount } from '@bindings/ThreadDayCount';
import type { ThreadListItem } from '@bindings/ThreadListItem';
import type { DayCount } from './timeline/dayCounts';

/** 一覧の枠。null はまだ読み込んでいない位置（高さだけのプレースホルダを出す）。 */
export type Slots = readonly (ThreadListItem | null)[];

/** 1 回に読む件数（OFFSET で読む単位）。 */
export const PAGE_SIZE = 100;

/**
 * DB の日ごとの件数を、つまみの形にする。日時の無いスレッド（一覧の最後）は、直前の日に数える
 * （落とすと件数と一覧の位置がずれる。会話の countByLocalDay と同じ扱い）。日の分かるスレッドが
 * 無ければ空（つまみを出さない）。
 */
export function toDayCounts(rows: readonly ThreadDayCount[]): DayCount[] {
  const days: DayCount[] = [];
  let undated = 0;
  for (const r of rows) {
    if (r.date_key === null) undated += r.count;
    else days.push({ dateKey: r.date_key, count: r.count });
  }
  if (days.length === 0) return [];
  if (undated > 0) {
    const last = days[days.length - 1];
    days[days.length - 1] = { ...last, count: last.count + undated };
  }
  return days;
}

/** 日ごとの件数の合計（＝一覧の全スレッド数）。 */
export function totalOf(rows: readonly ThreadDayCount[]): number {
  return rows.reduce((n, r) => n + r.count, 0);
}

/** 全部が未読み込みの枠。 */
export function emptySlots(total: number): Slots {
  return new Array<ThreadListItem | null>(Math.max(0, total)).fill(null);
}

/**
 * offset 件目から読んだ行を枠へ入れた新しい枠。枠からはみ出す分は捨てる（読む間に件数が減ったとき）。
 * 同じ id が別の位置に残っていれば、そこは未読み込みに戻す（読む間に並びが動いたとき、同じ行を
 * 二重に出さない）。
 */
export function fillSlots(slots: Slots, offset: number, rows: readonly ThreadListItem[]): Slots {
  const next = slots.slice();
  const end = Math.min(next.length, offset + rows.length);
  const ids = new Set(rows.slice(0, Math.max(0, end - offset)).map((r) => r.id));
  for (let i = 0; i < next.length; i++) {
    if ((i < offset || i >= end) && next[i] && ids.has(next[i]!.id)) next[i] = null;
  }
  for (let i = offset; i < end; i++) next[i] = rows[i - offset];
  return next;
}

/**
 * [start, end) 件目を見せるのに読む必要があるページ（PAGE_SIZE 単位の番号）。
 * 未読み込みの位置を含み、読んでいる最中でないページだけ。
 */
export function pagesToLoad(
  slots: Slots,
  start: number,
  end: number,
  inFlight: ReadonlySet<number>,
  pageSize = PAGE_SIZE
): number[] {
  const lo = Math.max(0, start);
  const hi = Math.min(slots.length, end);
  if (lo >= hi) return [];
  const pages: number[] = [];
  for (let p = Math.floor(lo / pageSize); p * pageSize < hi; p++) {
    if (inFlight.has(p)) continue;
    const from = Math.max(lo, p * pageSize);
    const to = Math.min(hi, (p + 1) * pageSize);
    for (let i = from; i < to; i++) {
      if (slots[i] === null) {
        pages.push(p);
        break;
      }
    }
  }
  return pages;
}

/** 先頭から途切れずに読み込んである件数（絞り込み中は、ここまでを上から順に見せる）。 */
export function loadedPrefix(slots: Slots): number {
  const i = slots.indexOf(null);
  return i === -1 ? slots.length : i;
}

/** 読み込んである行（並び順のまま。未読み込みの位置は飛ばす）。 */
export function loadedRows(slots: Slots): ThreadListItem[] {
  return slots.filter((m): m is ThreadListItem => m !== null);
}

/**
 * 読み込んである行に一覧の更新（既読・スター・削除など）をかける。fn は読み込んである行の並びを
 * 受け取って新しい並びを返す（いままでの setMails と同じ形）。
 * - 行を書き換えただけ（件数が同じ）なら、同じ位置へ戻す
 * - 行が減った（削除・移動）なら、その位置を枠から抜いて詰める（DB からも消えるので、後ろの offset も揃う）。
 *   抜いた位置（更新前の番号）を removed に返す（日ごとの件数を詰めるのに使う）
 */
export function updateLoaded(
  slots: Slots,
  fn: (rows: ThreadListItem[]) => ThreadListItem[]
): { slots: Slots; removed: number[] } {
  const rows = loadedRows(slots);
  const next = fn(rows);
  if (next.length === rows.length) {
    let k = 0;
    return { slots: slots.map((m) => (m === null ? null : next[k++])), removed: [] };
  }
  const byId = new Map(next.map((m) => [m.id, m]));
  const removed: number[] = [];
  const out: (ThreadListItem | null)[] = [];
  slots.forEach((m, i) => {
    if (m === null) out.push(null);
    else {
      const kept = byId.get(m.id);
      if (kept) out.push(kept);
      else removed.push(i);
    }
  });
  return { slots: out, removed };
}

/** 一覧から抜いた位置（番号）の分だけ、その日の件数を減らす。0 件になった日は消す。 */
export function dropFromDays(days: readonly DayCount[], removed: readonly number[]): DayCount[] {
  if (removed.length === 0) return days.slice();
  const counts = days.map((d) => d.count);
  let start = 0;
  let k = 0;
  const sorted = [...removed].sort((a, b) => a - b);
  days.forEach((d, j) => {
    while (k < sorted.length && sorted[k] < start + d.count) {
      counts[j] -= 1;
      k++;
    }
    start += d.count;
  });
  return days.map((d, j) => ({ ...d, count: counts[j] })).filter((d) => d.count > 0);
}

/** 枠の中で、その id の行の位置（無ければ -1）。 */
export function indexOfId(slots: Slots, id: number): number {
  return slots.findIndex((m) => m !== null && m.id === id);
}
