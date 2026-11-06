import { describe, expect, it } from 'vitest';
import type { ThreadListItem } from '@bindings/ThreadListItem';
import {
  dayLayout,
  daysOfRows,
  emptySlots,
  fillSlots,
  foldAllBut,
  foldedDayOf,
  indexOfId,
  jumpRow,
  loadedPrefix,
  loadedRows,
  pagesToLoad,
  retallyDays,
  rowAt,
  rowOfSlot,
  shownRows,
  slotAtRow,
  slotSegments,
  stickyDay,
  neighborDay,
  dayTone,
  rowBar,
  stickyPush,
  toDayCounts,
  totalOf,
  updateLoaded,
  unreadOf,
  withUnread,
  type ListDay,
} from './mailListWindow';

const row = (id: number): ThreadListItem => ({
  thread_id: id,
  id,
  account_id: 1,
  subject: `件名 ${id}`,
  from_address: 'taro@example.invalid',
  from_name: '試験 太郎',
  to_addresses: null,
  to_name: null,
  date: null,
  preview: '',
  is_read: true,
  has_real_attachments: false,
  is_starred: false,
  is_bookmarked: false,
  tag_ids: [],
  is_known: false,
  is_vip: false,
  is_green: false,
  message_count: 1,
  unread_count: 0,
  is_replied: false,
  email_ids: [id],
});

describe('toDayCounts / withUnread', () => {
  it('日時の無いスレッドを直前の日に数え、合計を保つ（未読はあとから入るので 0）', () => {
    const rows = [
      { date_key: '2026-10-02', count: 3 },
      { date_key: '2026-10-01', count: 2 },
      { date_key: null, count: 4 },
    ];
    expect(toDayCounts(rows)).toEqual([
      { dateKey: '2026-10-02', count: 3, unread: 0 },
      { dateKey: '2026-10-01', count: 6, unread: 0 },
    ]);
    expect(totalOf(rows)).toBe(9);
  });
  it('日の分かるスレッドが無ければ空', () => {
    expect(toDayCounts([{ date_key: null, count: 4 }])).toEqual([]);
    expect(toDayCounts([])).toEqual([]);
  });
  it('あとから引いた未読を日に入れる（載っていない日は 0・日時の無い分は最後の日）', () => {
    const days = toDayCounts([
      { date_key: '2026-10-03', count: 3 },
      { date_key: '2026-10-02', count: 2 },
      { date_key: '2026-10-01', count: 2 },
      { date_key: null, count: 2 },
    ]);
    const unread = [
      { date_key: '2026-10-03', unread: 2 },
      { date_key: '2026-10-01', unread: 1 },
      { date_key: null, unread: 1 },
    ];
    const got = withUnread(days, unread);
    expect(got.map((d) => d.unread)).toEqual([2, 0, 2]);
    // 読み直しの間は前の数を引き継ぐ（unreadOf → withUnread）
    expect(withUnread(days, unreadOf(got)).map((d) => d.unread)).toEqual([2, 0, 2]);
    // 件数を超えない
    expect(withUnread(days, [{ date_key: '2026-10-02', unread: 9 }])[1].unread).toBe(2);
    expect(withUnread([], unread)).toEqual([]);
  });
});

describe('daysOfRows', () => {
  // 端末ローカルの日付にする（時間帯に依らず、その日の正午）
  const at = (id: number, day: string | null, read = true): ThreadListItem => ({
    ...row(id),
    date: day ? new Date(`${day}T12:00:00`).toISOString() : null,
    is_read: read,
  });
  it('手元の行を並び順のまま日ごとに数え、日時の無い行は直前の日に入れる', () => {
    const rows = [
      at(1, null),
      at(2, '2026-11-06', false),
      at(3, '2026-11-06'),
      at(4, '2026-11-05', false),
      at(5, null, false),
    ];
    expect(daysOfRows(rows)).toEqual([
      { dateKey: '2026-11-06', count: 3, unread: 1 },
      { dateKey: '2026-11-05', count: 2, unread: 2 },
    ]);
    expect(daysOfRows([at(1, null)])).toEqual([]);
  });
});

describe('fillSlots / pagesToLoad', () => {
  it('飛んだ先のページだけを読み、読んだ位置に入れる', () => {
    let slots = emptySlots(250);
    expect(pagesToLoad(slots, 0, 30, new Set(), 100)).toEqual([0]);
    // 120〜230 件目を見せる → ページ 1 と 2
    expect(pagesToLoad(slots, 120, 230, new Set(), 100)).toEqual([1, 2]);
    // 読んでいる最中のページは頼まない
    expect(pagesToLoad(slots, 120, 230, new Set([1]), 100)).toEqual([2]);
    slots = fillSlots(slots, 100, [101, 102, 103].map(row));
    expect(slots[100]?.id).toBe(101);
    expect(slots[103]).toBeNull();
    // 枠の外は捨てる
    slots = fillSlots(slots, 248, [1, 2, 3].map(row));
    expect(slots.length).toBe(250);
    expect(slots[249]?.id).toBe(2);
  });
  it('読み込み済みのページは頼まないが、穴が残っていれば頼む', () => {
    let slots = emptySlots(150);
    slots = fillSlots(
      slots,
      0,
      Array.from({ length: 100 }, (_, i) => row(i + 1))
    );
    expect(pagesToLoad(slots, 0, 100, new Set(), 100)).toEqual([]);
    expect(pagesToLoad(slots, 50, 120, new Set(), 100)).toEqual([1]);
  });
  it('並びが動いて同じ行が別の位置にあれば、古い位置を未読み込みに戻す', () => {
    let slots = fillSlots(emptySlots(5), 0, [1, 2, 3].map(row));
    slots = fillSlots(slots, 2, [2, 3, 4].map(row));
    expect(slots.map((m) => m?.id ?? null)).toEqual([1, null, 2, 3, 4]);
  });
  it('先頭から途切れずに読んだ件数と、読んだ行', () => {
    const slots = fillSlots(fillSlots(emptySlots(6), 0, [1, 2].map(row)), 4, [5].map(row));
    expect(loadedPrefix(slots)).toBe(2);
    expect(loadedRows(slots).map((m) => m.id)).toEqual([1, 2, 5]);
    expect(indexOfId(slots, 5)).toBe(4);
    expect(indexOfId(slots, 9)).toBe(-1);
  });
});

describe('updateLoaded / retallyDays', () => {
  const base = fillSlots(fillSlots(emptySlots(6), 0, [1, 2].map(row)), 3, [4, 5].map(row));
  const days: ListDay[] = [
    { dateKey: '2026-10-03', count: 2, unread: 1 },
    { dateKey: '2026-10-02', count: 1, unread: 0 },
    { dateKey: '2026-10-01', count: 3, unread: 1 },
  ];
  it('書き換えは同じ位置へ戻し、既読・未読の変化を日の未読に足す', () => {
    const { slots, removed } = updateLoaded(base, (rows) =>
      rows.map((m) => (m.id === 4 ? { ...m, is_read: false } : m))
    );
    expect(removed).toEqual([]);
    expect(slots.map((m) => m?.id ?? null)).toEqual([1, 2, null, 4, 5, null]);
    expect(slots[3]?.is_read).toBe(false);
    const marked = retallyDays(days, base, slots, removed);
    expect(marked).toEqual([
      { dateKey: '2026-10-03', count: 2, unread: 1 },
      { dateKey: '2026-10-02', count: 1, unread: 0 },
      { dateKey: '2026-10-01', count: 3, unread: 2 },
    ]);
    // 戻す（未読 → 既読）
    expect(retallyDays(marked, slots, base, [])[2].unread).toBe(1);
  });
  it('削除はその位置を詰め、日ごとの件数（未読なら未読も）を減らす', () => {
    const before = updateLoaded(base, (rows) =>
      rows.map((m) => (m.id === 2 ? { ...m, is_read: false } : m))
    ).slots;
    const { slots, removed } = updateLoaded(before, (rows) =>
      rows.filter((m) => m.id !== 2 && m.id !== 4)
    );
    expect(removed).toEqual([1, 3]);
    expect(slots.map((m) => m?.id ?? null)).toEqual([1, null, 5, null]);
    expect(retallyDays(days, before, slots, removed)).toEqual([
      { dateKey: '2026-10-03', count: 1, unread: 0 },
      { dateKey: '2026-10-02', count: 1, unread: 0 },
      { dateKey: '2026-10-01', count: 2, unread: 1 },
    ]);
    // 0 件になった日は消す
    const mid = fillSlots(base, 2, [row(3)]);
    const gone = updateLoaded(mid, (rows) => rows.filter((m) => m.id !== 3));
    expect(gone.removed).toEqual([2]);
    expect(retallyDays(days, mid, gone.slots, gone.removed).map((d) => d.dateKey)).toEqual([
      '2026-10-03',
      '2026-10-01',
    ]);
  });
});

describe('dayLayout（日の見出しと畳み）', () => {
  // 3 日: 11/06 に 2 件・11/05 に 3 件・11/04 に 1 件（枠は 6）
  const days: ListDay[] = [
    { dateKey: '2026-11-06', count: 2, unread: 1 },
    { dateKey: '2026-11-05', count: 3, unread: 0 },
    { dateKey: '2026-11-04', count: 1, unread: 1 },
  ];
  const kinds = (l: ReturnType<typeof dayLayout>) =>
    Array.from({ length: l.rows }, (_, i) => {
      const r = rowAt(l, i);
      return r.kind === 'day' ? `d${r.day}` : `s${r.slot}`;
    });

  it('全部開いているときは、日ごとに見出し ＋ 枠を並べる', () => {
    const l = dayLayout(days, new Set(), 6);
    expect(l.rows).toBe(9);
    expect(kinds(l)).toEqual(['d0', 's0', 's1', 'd1', 's2', 's3', 's4', 'd2', 's5']);
    // 見出しを含めた offset: 枠 k の行番号
    expect([0, 1, 2, 3, 4, 5].map((k) => rowOfSlot(l, k))).toEqual([1, 2, 4, 5, 6, 8]);
  });

  it('畳んだ日は見出しだけになり、行の数と後ろの日の位置から外れる', () => {
    const l = dayLayout(days, new Set(['2026-11-05']), 6);
    expect(l.rows).toBe(6);
    expect(kinds(l)).toEqual(['d0', 's0', 's1', 'd1', 'd2', 's5']);
    expect(rowOfSlot(l, 3)).toBeNull();
    expect(rowOfSlot(l, 5)).toBe(5);
    // つまみの印: 畳んだ日の見出しは、その日の最初の枠
    expect(slotAtRow(l, 3)).toBe(2);
    expect(slotAtRow(l, 4)).toBe(5);
    // 読みに行くのは開いている日の枠だけ（畳んだ日を飛ばして区間を分ける）
    expect(slotSegments(l, 0, 6)).toEqual([
      [0, 2],
      [5, 6],
    ]);
    // 全部開いていれば、日をまたいでつながる
    expect(slotSegments(dayLayout(days, new Set(), 6), 2, 9)).toEqual([[1, 6]]);
  });

  it('つまみで畳んだ日へ飛ぶときは、先に開いてから飛び、位置がずれない', () => {
    const collapsed = foldAllBut(days, '2026-11-06');
    expect([...collapsed]).toEqual(['2026-11-05', '2026-11-04']);
    let l = dayLayout(days, collapsed, 6);
    expect(kinds(l)).toEqual(['d0', 's0', 's1', 'd1', 'd2']);
    // 11/05 の 2 件目（枠 3）へ飛ぶ: その日を開く
    expect(foldedDayOf(l, 3)).toBe('2026-11-05');
    collapsed.delete('2026-11-05');
    l = dayLayout(days, collapsed, 6);
    expect(foldedDayOf(l, 3)).toBeNull();
    const r = jumpRow(l, 3);
    expect(rowAt(l, r)).toEqual({ kind: 'slot', slot: 3 });
    expect(slotAtRow(l, r)).toBe(3);
    // 日の最初の枠へ飛ぶときは見出しに着く（見出しのすぐ下がその枠）
    expect(jumpRow(l, 2)).toBe(3);
    expect(rowAt(l, 3)).toEqual({ kind: 'day', day: 1 });
    expect(rowAt(l, 4)).toEqual({ kind: 'slot', slot: 2 });
    // 畳んだままの日へは見出しに着く
    expect(jumpRow(l, 5)).toBe(l.heads[2]);
  });

  it('読んでいない日を畳んで開いても、枠の番号はそのまま（開いたら正しく読める）', () => {
    // 11/05 の枠（2〜4）は未読み込み
    const slots = fillSlots(fillSlots(emptySlots(6), 0, [1, 2].map(row)), 5, [row(6)]);
    const folded = dayLayout(days, new Set(['2026-11-05']), 6);
    expect(shownRows(slots, folded).map((m) => m.id)).toEqual([1, 2, 6]);
    const open = dayLayout(days, new Set(), 6);
    // 開いた日の行の範囲 → 読むべき枠の区間 → ページ
    const segs = slotSegments(open, open.heads[1], open.heads[2]);
    expect(segs).toEqual([[2, 5]]);
    expect(pagesToLoad(slots, segs[0][0], segs[0][1], new Set(), 2)).toEqual([1, 2]);
    const filled = fillSlots(slots, 2, [3, 4, 5].map(row));
    expect(shownRows(filled, open).map((m) => m.id)).toEqual([1, 2, 3, 4, 5, 6]);
  });

  it('日が無い・件数が合わないときは、見出しを置かずにそのまま並べる', () => {
    for (const l of [dayLayout([], new Set(), 4), dayLayout(days, new Set(), 5)]) {
      expect(l.heads).toEqual([]);
      expect(rowAt(l, 2)).toEqual({ kind: 'slot', slot: 2 });
      expect(rowOfSlot(l, 2)).toBe(2);
      expect(jumpRow(l, 2)).toBe(2);
      expect(foldedDayOf(l, 2)).toBeNull();
    }
    expect(dayLayout([], new Set(), 4).rows).toBe(4);
    expect(dayLayout(days, new Set(), 5).rows).toBe(5);
  });
});

describe('stickyDay / stickyPush（貼り付ける見出し）', () => {
  const days: ListDay[] = [
    { dateKey: '2026-11-06', count: 2, unread: 0 },
    { dateKey: '2026-11-05', count: 3, unread: 0 },
    { dateKey: '2026-11-04', count: 1, unread: 0 },
  ];
  it('一番上の行が属する、開いている日を貼り付ける', () => {
    // 行: d0 s0 s1 | d1(畳み) | d2 s5
    const l = dayLayout(days, new Set(['2026-11-05']), 6);
    expect([0, 1, 2, 3, 4, 5].map((i) => stickyDay(l, i))).toEqual([0, 0, 0, null, 2, 2]);
    expect(stickyDay(l, 99)).toBeNull();
    expect(stickyDay(dayLayout([], new Set(), 3), 1)).toBeNull();
  });
  it('次の日の見出しが重なり始めたら、その分だけ上へ押し出す', () => {
    // 見出しの高さ 30px・スクロール 1000px
    expect(stickyPush(1100, 1000, 30)).toBe(0); // まだ遠い
    expect(stickyPush(1030, 1000, 30)).toBe(0); // ちょうど接する
    expect(stickyPush(1010, 1000, 30)).toBe(-20); // 20px 重なった
    expect(stickyPush(null, 1000, 30)).toBe(0);
  });
});

describe('dayTone / rowBar（左の縦の線）', () => {
  it('色は日付の通し番号の偶奇で決まり、続いた日は交互になる', () => {
    expect(dayTone('2026-11-06')).not.toBe(dayTone('2026-11-05'));
    expect(dayTone('2026-11-06')).toBe(dayTone('2026-11-04'));
    // 月・年の境目でも交互
    expect(dayTone('2026-11-01')).not.toBe(dayTone('2026-10-31'));
    expect(dayTone('2026-01-01')).not.toBe(dayTone('2025-12-31'));
  });
  it('線はその日のメールの行だけ（見出しには引かない）。最初のメールで始め、最後のメールで切る', () => {
    const days: ListDay[] = [
      { dateKey: '2026-11-06', count: 2, unread: 0 },
      { dateKey: '2026-11-05', count: 3, unread: 0 },
      { dateKey: '2026-11-04', count: 1, unread: 0 },
    ];
    // 行: d0 s0 s1 | d1(畳み) | d2 s5
    const l = dayLayout(days, new Set(['2026-11-05']), 6);
    const bars = Array.from({ length: l.rows }, (_, i) => rowBar(l, i));
    expect(bars.map((b) => (b ? `${b.first ? 'F' : '-'}${b.last ? 'L' : '-'}` : null))).toEqual([
      null,
      'F-',
      '-L',
      null,
      null,
      'FL',
    ]);
    expect(bars[1]?.tone).toBe(dayTone('2026-11-06'));
    expect(bars[5]?.tone).toBe(dayTone('2026-11-04'));
    expect(rowBar(dayLayout([], new Set(), 3), 0)).toBeNull();
  });
});

describe('neighborDay（見出しの ＜ ＞ で移る先）', () => {
  // メールのある日だけが並ぶ（11/5・11/2 にはメールが無い）
  const days: ListDay[] = [
    { dateKey: '2026-11-06', count: 2, unread: 0 },
    { dateKey: '2026-11-04', count: 3, unread: 0 },
    { dateKey: '2026-11-03', count: 1, unread: 0 },
    { dateKey: '2026-11-01', count: 2, unread: 0 },
  ];
  it('隣の日へ。メールの無い日は飛ばす', () => {
    const l = dayLayout(days, new Set(), 8);
    expect(neighborDay(l, 0, 'older')).toBe(1); // 11/6 → 11/4（11/5 を飛ばす）
    expect(neighborDay(l, 2, 'older')).toBe(3); // 11/3 → 11/1（11/2 を飛ばす）
    expect(neighborDay(l, 2, 'newer')).toBe(1);
  });
  it('端では移らない', () => {
    const l = dayLayout(days, new Set(), 8);
    expect(neighborDay(l, 0, 'newer')).toBeNull();
    expect(neighborDay(l, 3, 'older')).toBeNull();
  });
  it('畳んだ日へも移り、開かずに見出しを一番上に出す（jumpRow で見出しの行）', () => {
    const l = dayLayout(days, new Set(['2026-11-04']), 8);
    const to = neighborDay(l, 0, 'older');
    expect(to).toBe(1);
    expect(l.folded[1]).toBe(true);
    expect(jumpRow(l, l.starts[1])).toBe(l.heads[1]);
    expect(rowAt(l, l.heads[1])).toEqual({ kind: 'day', day: 1 });
    // 畳んだ日から次へ
    expect(jumpRow(l, l.starts[2])).toBe(l.heads[2]);
  });
});
