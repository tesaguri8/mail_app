import { describe, expect, it } from 'vitest';
import type { ThreadListItem } from '@bindings/ThreadListItem';
import {
  dropFromDays,
  emptySlots,
  fillSlots,
  indexOfId,
  loadedPrefix,
  loadedRows,
  pagesToLoad,
  toDayCounts,
  totalOf,
  updateLoaded,
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

describe('toDayCounts', () => {
  it('日時の無いスレッドを直前の日に数え、合計を保つ', () => {
    const rows = [
      { date_key: '2026-10-02', count: 3 },
      { date_key: '2026-10-01', count: 2 },
      { date_key: null, count: 4 },
    ];
    expect(toDayCounts(rows)).toEqual([
      { dateKey: '2026-10-02', count: 3 },
      { dateKey: '2026-10-01', count: 6 },
    ]);
    expect(totalOf(rows)).toBe(9);
  });
  it('日の分かるスレッドが無ければ空', () => {
    expect(toDayCounts([{ date_key: null, count: 4 }])).toEqual([]);
    expect(toDayCounts([])).toEqual([]);
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

describe('updateLoaded / dropFromDays', () => {
  const base = fillSlots(fillSlots(emptySlots(6), 0, [1, 2].map(row)), 3, [4, 5].map(row));
  it('書き換えは同じ位置へ戻す', () => {
    const { slots, removed } = updateLoaded(base, (rows) =>
      rows.map((m) => (m.id === 4 ? { ...m, is_read: false } : m))
    );
    expect(removed).toEqual([]);
    expect(slots.map((m) => m?.id ?? null)).toEqual([1, 2, null, 4, 5, null]);
    expect(slots[3]?.is_read).toBe(false);
  });
  it('削除はその位置を詰め、日ごとの件数も減らす', () => {
    const { slots, removed } = updateLoaded(base, (rows) =>
      rows.filter((m) => m.id !== 2 && m.id !== 4)
    );
    expect(removed).toEqual([1, 3]);
    expect(slots.map((m) => m?.id ?? null)).toEqual([1, null, 5, null]);
    const days = [
      { dateKey: '2026-10-03', count: 2 },
      { dateKey: '2026-10-02', count: 1 },
      { dateKey: '2026-10-01', count: 3 },
    ];
    expect(dropFromDays(days, removed)).toEqual([
      { dateKey: '2026-10-03', count: 1 },
      { dateKey: '2026-10-02', count: 1 },
      { dateKey: '2026-10-01', count: 2 },
    ]);
    expect(dropFromDays(days, [2])).toEqual([
      { dateKey: '2026-10-03', count: 2 },
      { dateKey: '2026-10-01', count: 3 },
    ]);
  });
});
