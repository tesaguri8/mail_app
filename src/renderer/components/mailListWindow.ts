/**
 * メール一覧の「全期間ぶんの枠」と、飛んだ先だけを読むための計算（純粋な関数だけ）。
 *
 * 一覧はフォルダ内の全スレッドを最初から枠（slots）として持ち、読み込んだ位置だけに行が入る。
 * 枠の数と日付の位置は thread_day_counts（日ごとの件数）から決まり、累計がそのまま一覧の offset になる。
 * Photosky の §2「読み込んでいない日付へ飛ぶ仕組み」と同じ作り。設計: docs/MAIL_LIST_TIMELINE.md
 *
 * 一覧の行は「日の見出し ＋ その日の枠」の並び。畳んだ日は見出しだけになり、その日の枠は
 * 仮想スクロールの行数・位置から外れる（`dayLayout`。設計の §6）。
 */
import type { ThreadDayCount } from '@bindings/ThreadDayCount';
import type { ThreadListItem } from '@bindings/ThreadListItem';
import { localDayKey, type DayCount } from './timeline/dayCounts';

/** 一覧の枠。null はまだ読み込んでいない位置（高さだけのプレースホルダを出す）。 */
export type Slots = readonly (ThreadListItem | null)[];

/** 一覧の 1 日（つまみの日ごとの件数に、未読のあるスレッドの数を足したもの）。 */
export interface ListDay extends DayCount {
  unread: number;
}

/** 1 回に読む件数（OFFSET で読む単位）。 */
export const PAGE_SIZE = 100;

/**
 * DB の日ごとの件数を、つまみの形にする。日時の無いスレッド（一覧の最後）は、直前の日に数える
 * （落とすと件数と一覧の位置がずれる。会話の countByLocalDay と同じ扱い）。日の分かるスレッドが
 * 無ければ空（つまみを出さない）。
 */
export function toDayCounts(rows: readonly ThreadDayCount[]): ListDay[] {
  const days: ListDay[] = [];
  let undated = 0;
  let undatedUnread = 0;
  for (const r of rows) {
    if (r.date_key === null) {
      undated += r.count;
      undatedUnread += r.unread;
    } else days.push({ dateKey: r.date_key, count: r.count, unread: r.unread });
  }
  if (days.length === 0) return [];
  if (undated > 0) {
    const last = days[days.length - 1];
    days[days.length - 1] = {
      ...last,
      count: last.count + undated,
      unread: last.unread + undatedUnread,
    };
  }
  return days;
}

/**
 * 手元の行の並び（絞り込み中の一覧）を日ごとに数える。日時の無い行は直前の行の日に数える
 * （先頭なら最初に日の分かる行の日。会話の countByLocalDay と同じ扱い）。日の分かる行が無ければ空。
 */
export function daysOfRows(rows: readonly ThreadListItem[]): ListDay[] {
  const keys = rows.map((m) => localDayKey(m.date));
  let current = keys.find((k): k is string => k !== null);
  if (current === undefined) return [];
  const days: ListDay[] = [];
  rows.forEach((m, i) => {
    current = keys[i] ?? current;
    const last = days[days.length - 1];
    const unread = isUnread(m) ? 1 : 0;
    if (last && last.dateKey === current) {
      last.count += 1;
      last.unread += unread;
    } else days.push({ dateKey: current as string, count: 1, unread });
  });
  return days;
}

/** 未読のあるスレッドか（一覧の未読の印と同じ判定）。 */
function isUnread(m: ThreadListItem): boolean {
  return !m.is_read;
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

/**
 * 一覧の更新（`updateLoaded`）に合わせて、日ごとの件数と未読の数を直す。
 * - removed（更新前の番号）の行は、その日の件数から引く（未読なら未読からも）。0 件になった日は消す
 * - 残った行は、既読・未読が変わった分だけ、その日の未読を増減する
 * before / after は更新の前と後の枠（after は removed を詰めたもの）。
 */
export function retallyDays(
  days: readonly ListDay[],
  before: Slots,
  after: Slots,
  removed: readonly number[]
): ListDay[] {
  const out = days.map((d) => ({ ...d }));
  const starts = startsOf(days);
  const gone = [...removed].sort((a, b) => a - b);
  let k = 0;
  before.forEach((m, i) => {
    if (m === null) return;
    const d = dayIndexOf(starts, i);
    if (d < 0 || d >= out.length) return;
    while (k < gone.length && gone[k] < i) k++;
    if (gone[k] === i) {
      out[d].count -= 1;
      if (isUnread(m)) out[d].unread -= 1;
      return;
    }
    const now = after[i - k];
    if (now && isUnread(now) !== isUnread(m)) out[d].unread += isUnread(now) ? 1 : -1;
  });
  return out
    .filter((d) => d.count > 0)
    .map((d) => ({ ...d, unread: Math.min(d.count, Math.max(0, d.unread)) }));
}

/** 各日の最初の位置（累計）。 */
function startsOf(days: readonly DayCount[]): number[] {
  let n = 0;
  return days.map((d) => {
    const s = n;
    n += d.count;
    return s;
  });
}

/** 昇順の並び xs で、値が x 以下の最後の番号（先頭より前なら -1）。 */
function lastAtOrBelow(xs: readonly number[], x: number): number {
  let lo = 0;
  let hi = xs.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (xs[mid] <= x) {
      found = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  return found;
}

/** 位置 i のある日の番号。 */
function dayIndexOf(starts: readonly number[], i: number): number {
  return lastAtOrBelow(starts, i);
}

// ---- 日の見出しと畳み（設計: docs/MAIL_LIST_TIMELINE.md §6） ----

/**
 * 一覧の行の並び。各日の頭に見出しの行を 1 つ置き、開いている日だけ、その日の枠を並べる。
 * 日が無い（日の分かるスレッドが無い・検索中）か、日ごとの件数の合計が枠の数と合わないときは、
 * 見出しを置かずに枠をそのまま並べる（行 i ＝ 枠 i）。
 */
export interface DayLayout {
  days: readonly ListDay[];
  /** 各日の最初の枠の番号 */
  starts: readonly number[];
  /** 各日の見出しの行番号 */
  heads: readonly number[];
  /** 各日が畳まれているか */
  folded: readonly boolean[];
  /** 行の数（見出しを含む。仮想スクロールの count） */
  rows: number;
}

/** 一覧の 1 行: 日の見出しか、枠（何件目のスレッドか）。 */
export type ListRow = { kind: 'day'; day: number } | { kind: 'slot'; slot: number };

/** 日ごとの件数と畳んだ日から、行の並びを作る。total は枠の数。 */
export function dayLayout(
  days: readonly ListDay[],
  collapsed: ReadonlySet<string>,
  total: number
): DayLayout {
  const sum = days.reduce((n, d) => n + d.count, 0);
  if (days.length === 0 || sum !== total) {
    return { days: [], starts: [], heads: [], folded: [], rows: Math.max(0, total) };
  }
  const starts = startsOf(days);
  const folded = days.map((d) => collapsed.has(d.dateKey));
  let row = 0;
  const heads = days.map((d, i) => {
    const h = row;
    row += 1 + (folded[i] ? 0 : d.count);
    return h;
  });
  return { days, starts, heads, folded, rows: row };
}

/** 行 i は見出しか、何件目の枠か。 */
export function rowAt(layout: DayLayout, i: number): ListRow {
  if (layout.heads.length === 0) return { kind: 'slot', slot: i };
  const d = Math.max(0, lastAtOrBelow(layout.heads, i));
  if (layout.heads[d] === i) return { kind: 'day', day: d };
  return { kind: 'slot', slot: layout.starts[d] + (i - layout.heads[d] - 1) };
}

/** 枠 slot の行番号。その日が畳まれていれば null。 */
export function rowOfSlot(layout: DayLayout, slot: number): number | null {
  if (layout.heads.length === 0) return slot;
  const d = dayIndexOf(layout.starts, slot);
  if (d < 0 || layout.folded[d]) return null;
  return layout.heads[d] + 1 + (slot - layout.starts[d]);
}

/** 枠 slot のある日の番号（見出しが無ければ -1）。 */
export function dayOfSlot(layout: DayLayout, slot: number): number {
  if (layout.heads.length === 0) return -1;
  return dayIndexOf(layout.starts, slot);
}

/** 行 i を、つまみの位置（何件目か）にする。見出しはその日の最初の枠。 */
export function slotAtRow(layout: DayLayout, i: number): number {
  const r = rowAt(layout, i);
  return r.kind === 'slot' ? r.slot : layout.starts[r.day];
}

/**
 * 行の範囲 [rowStart, rowEnd) に見えている枠の区間（[start, end) の並び）。畳んだ日の枠は含めない
 * （読みに行かない）。隣り合う区間はつなぐ。
 */
export function slotSegments(
  layout: DayLayout,
  rowStart: number,
  rowEnd: number
): [number, number][] {
  const lo = Math.max(0, rowStart);
  const hi = Math.min(layout.rows, rowEnd);
  if (lo >= hi) return [];
  if (layout.heads.length === 0) return [[lo, hi]];
  const out: [number, number][] = [];
  for (let d = Math.max(0, lastAtOrBelow(layout.heads, lo)); d < layout.days.length; d++) {
    const head = layout.heads[d];
    if (head >= hi) break;
    if (layout.folded[d]) continue;
    // その日の枠の行は [head+1, head+1+count)
    const from = Math.max(lo, head + 1);
    const to = Math.min(hi, head + 1 + layout.days[d].count);
    if (from >= to) continue;
    const seg: [number, number] = [
      layout.starts[d] + (from - head - 1),
      layout.starts[d] + (to - head - 1),
    ];
    const last = out[out.length - 1];
    if (last && last[1] === seg[0]) last[1] = seg[1];
    else out.push(seg);
  }
  return out;
}

/**
 * つまみで枠 slot へ飛ぶ先の行。日の最初の枠なら、その日の見出しを一番上に置く（見出しのすぐ下に
 * その行が来る）。日の途中ならその行。畳まれた日の途中なら見出し（先に開いてから呼ぶこと）。
 */
export function jumpRow(layout: DayLayout, slot: number): number {
  if (layout.heads.length === 0) return slot;
  const d = Math.max(0, dayIndexOf(layout.starts, slot));
  if (slot === layout.starts[d]) return layout.heads[d];
  return rowOfSlot(layout, slot) ?? layout.heads[d];
}

/** 枠 slot へ飛ぶ前に開く日（畳まれていなければ null）。 */
export function foldedDayOf(layout: DayLayout, slot: number): string | null {
  const d = dayOfSlot(layout, slot);
  return d >= 0 && layout.folded[d] ? layout.days[d].dateKey : null;
}

/** 「keep 以外を全部畳む」ときの畳む日の集合。 */
export function foldAllBut(days: readonly DayCount[], keep: string | null): Set<string> {
  return new Set(days.map((d) => d.dateKey).filter((k) => k !== keep));
}

/** 読み込んである行のうち、開いている日の行だけ（並び順のまま。矢印キー・範囲選択の対象）。 */
export function shownRows(
  slots: readonly (ThreadListItem | null)[],
  layout: DayLayout
): ThreadListItem[] {
  return slots.filter((m, i): m is ThreadListItem => m !== null && rowOfSlot(layout, i) !== null);
}

/** 枠の中で、その id の行の位置（無ければ -1）。 */
export function indexOfId(slots: Slots, id: number): number {
  return slots.findIndex((m) => m !== null && m.id === id);
}
