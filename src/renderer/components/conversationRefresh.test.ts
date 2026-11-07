import { describe, expect, it } from 'vitest';
import type { ThreadView } from '@bindings/ThreadView';
import {
  keepMatch,
  sameThreadView,
  scrollAfterRefresh,
  shouldRefreshOnSync,
} from './conversationRefresh';

/** 会話の作り物（中身は比べるだけなので最小限）。 */
const view = (ids: number[]): ThreadView =>
  ({
    thread: { id: 1, subject: '打ち合わせの日程' },
    messages: ids.map((id) => ({ id, is_read: true })),
  }) as unknown as ThreadView;

describe('shouldRefreshOnSync（同期の合図で読み直すか）', () => {
  it('新しく取り込んだときだけ読み直す', () => {
    expect(shouldRefreshOnSync(1)).toBe(true);
    expect(shouldRefreshOnSync(0)).toBe(false);
    expect(shouldRefreshOnSync(undefined)).toBe(false);
  });
});

describe('sameThreadView（変化が無ければ差し替えない）', () => {
  it('同じ中身なら同じ、送った返信が 1 通増えたら違う', () => {
    expect(sameThreadView(view([1, 2]), view([1, 2]))).toBe(true);
    expect(sameThreadView(view([1, 2]), view([1, 2, 3]))).toBe(false);
  });
  it('まだ読んでいない（null）なら違う', () => {
    expect(sameThreadView(null, view([1]))).toBe(false);
  });
});

describe('scrollAfterRefresh（読み直したあとのスクロール）', () => {
  it('下端にいたら新しい最後のバブルへ追う', () => {
    expect(scrollAfterRefresh({ scrollTop: 600, scrollHeight: 1000, clientHeight: 400 })).toBe(
      'follow'
    );
    // 端数で数 px 届かなくても下端とみなす
    expect(scrollAfterRefresh({ scrollTop: 597.5, scrollHeight: 1000, clientHeight: 400 })).toBe(
      'follow'
    );
  });
  it('途中を読んでいたらその場に留まる', () => {
    expect(scrollAfterRefresh({ scrollTop: 200, scrollHeight: 1000, clientHeight: 400 })).toBe(
      'keep'
    );
  });
  it('スクロールしない短い会話は下端にいるのと同じ', () => {
    expect(scrollAfterRefresh({ scrollTop: 0, scrollHeight: 300, clientHeight: 400 })).toBe(
      'follow'
    );
  });
});

describe('keepMatch（検索の位置を保つ）', () => {
  it('見ている一致の番号を保つ', () => {
    expect(keepMatch(2, 5)).toEqual({ idx: 2, total: 5 });
  });
  it('一致が減ってはみ出したら最後へ、無くなれば 0', () => {
    expect(keepMatch(4, 3)).toEqual({ idx: 3, total: 3 });
    expect(keepMatch(2, 0)).toEqual({ idx: 0, total: 0 });
  });
  it('まだどれも見ていなければ最初の一致', () => {
    expect(keepMatch(0, 3)).toEqual({ idx: 1, total: 3 });
  });
});
