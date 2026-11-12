import { describe, expect, it } from 'vitest';
import type { ThreadView } from '@bindings/ThreadView';
import {
  keepMatch,
  markMessagesRead,
  sameThreadView,
  scrollAfterRefresh,
  shouldFetchMissingRefs,
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

describe('shouldFetchMissingRefs（手元に無いメールを探しに行くか）', () => {
  const withRefs = (states: ('due' | 'absent')[]): ThreadView =>
    ({
      ...view([1]),
      missing_refs: states.map((state, i) => ({ message_id: `r${i}@x`, state })),
    }) as unknown as ThreadView;

  it('探す時期の ID があり、この画面でまだ探していなければ探す', () => {
    expect(shouldFetchMissingRefs(withRefs(['absent', 'due']), new Set())).toBe(true);
  });
  it('探した会話・探す時期の ID が無い会話・読む前は探さない', () => {
    expect(shouldFetchMissingRefs(withRefs(['due']), new Set([1]))).toBe(false);
    expect(shouldFetchMissingRefs(withRefs(['absent']), new Set())).toBe(false);
    expect(shouldFetchMissingRefs(withRefs([]), new Set())).toBe(false);
    expect(shouldFetchMissingRefs(null, new Set())).toBe(false);
  });
});

describe('markMessagesRead（一覧で既読にしたら、会話のバブルの未読の点も消す）', () => {
  const thread = (msgs: [number, boolean][]): ThreadView =>
    ({
      thread: { id: 1 },
      messages: msgs.map(([id, is_read]) => ({ id, is_read })),
    }) as unknown as ThreadView;

  it('合図の id の未読だけを既読にする', () => {
    const v = markMessagesRead(
      thread([
        [1, false],
        [2, false],
        [3, true],
      ]),
      [1, 3]
    );
    expect(v?.messages.map((m) => [m.id, m.is_read])).toEqual([
      [1, true],
      [2, false],
      [3, true],
    ]);
  });

  it('合図に無いメール（手動で未読に戻したもの等）は触らない', () => {
    const v = markMessagesRead(thread([[5, false]]), [1]);
    expect(v?.messages[0].is_read).toBe(false);
  });

  it('変わるものが無ければ同じ値を返す（描き直さない）', () => {
    const v = thread([[1, true]]);
    expect(markMessagesRead(v, [1])).toBe(v);
    expect(markMessagesRead(null, [1])).toBeNull();
  });
});
