// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { ThreadListItem } from '@bindings/ThreadListItem';
import {
  READ_AFTER_MS,
  createReadTimer,
  passesWhileReading,
  useReadAfterDelay,
  type ReadTimer,
} from './readAfterDelay';

// 未読のスレッドは、見ている時間が READ_AFTER_MS に達したら既読にする（利用者の要望）。
// それより前に離れたら未読のまま。手動の切り替えで予約を取り消す。窓が裏の間は数えない。

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('createReadTimer', () => {
  it('5 秒経ったら 1 回だけ既読にする', () => {
    const onElapsed = vi.fn();
    const timer = createReadTimer(onElapsed);
    timer.start();
    vi.advanceTimersByTime(READ_AFTER_MS - 1);
    expect(onElapsed).not.toHaveBeenCalled();
    expect(timer.pending).toBe(true);
    vi.advanceTimersByTime(1);
    expect(onElapsed).toHaveBeenCalledTimes(1);
    expect(timer.pending).toBe(false);
    vi.advanceTimersByTime(READ_AFTER_MS * 3);
    expect(onElapsed).toHaveBeenCalledTimes(1);
  });

  it('5 秒より前に離れたら既読にしない', () => {
    const onElapsed = vi.fn();
    const timer = createReadTimer(onElapsed);
    timer.start();
    vi.advanceTimersByTime(READ_AFTER_MS - 1);
    timer.cancel();
    vi.advanceTimersByTime(READ_AFTER_MS * 3);
    expect(onElapsed).not.toHaveBeenCalled();
    expect(timer.pending).toBe(false);
  });

  it('別のスレッドへ移ったら、そのスレッドの時間を最初から数える', () => {
    const onElapsed = vi.fn();
    const timer = createReadTimer(onElapsed);
    timer.start();
    vi.advanceTimersByTime(3_000);
    timer.start(); // 別のスレッドを開いた
    vi.advanceTimersByTime(3_000);
    expect(onElapsed).not.toHaveBeenCalled();
    vi.advanceTimersByTime(2_000);
    expect(onElapsed).toHaveBeenCalledTimes(1);
  });

  it('手動で切り替えたら、待っている予約を取り消す', () => {
    const onElapsed = vi.fn();
    const timer = createReadTimer(onElapsed);
    timer.start();
    vi.advanceTimersByTime(2_000);
    timer.cancel(); // 手動で既読/未読を切り替えた
    vi.advanceTimersByTime(READ_AFTER_MS);
    expect(onElapsed).not.toHaveBeenCalled();
  });

  it('裏に回っている間は数えず、戻ったら残りから続ける', () => {
    const onElapsed = vi.fn();
    const timer = createReadTimer(onElapsed);
    timer.start();
    vi.advanceTimersByTime(2_000);
    timer.setAttentive(false);
    vi.advanceTimersByTime(60_000);
    expect(onElapsed).not.toHaveBeenCalled();
    timer.setAttentive(true);
    vi.advanceTimersByTime(2_999);
    expect(onElapsed).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onElapsed).toHaveBeenCalledTimes(1);
  });

  it('裏に回ったまま開いたら、表に出るまで数え始めない', () => {
    const onElapsed = vi.fn();
    const timer = createReadTimer(onElapsed, READ_AFTER_MS, false);
    timer.start();
    vi.advanceTimersByTime(60_000);
    expect(onElapsed).not.toHaveBeenCalled();
    timer.setAttentive(true);
    vi.advanceTimersByTime(READ_AFTER_MS);
    expect(onElapsed).toHaveBeenCalledTimes(1);
  });

  it('裏に回っている間に離れたら、表に出ても既読にしない', () => {
    const onElapsed = vi.fn();
    const timer = createReadTimer(onElapsed);
    timer.start();
    timer.setAttentive(false);
    timer.cancel();
    timer.setAttentive(true);
    vi.advanceTimersByTime(READ_AFTER_MS * 2);
    expect(onElapsed).not.toHaveBeenCalled();
  });
});

const row = (id: number, read: boolean): ThreadListItem =>
  ({ id, is_read: read, unread_count: read ? 0 : 1 }) as ThreadListItem;
const unreadOnly = (m: ThreadListItem) => !m.is_read;

describe('passesWhileReading（未読で絞り込み中）', () => {
  it('時間が来て既読にした読書中の行は、離れるまで未読の絞り込みに残す', () => {
    const passes = passesWhileReading(unreadOnly, { rowId: 1, ids: [1], read: true });
    expect(passes(row(1, true))).toBe(true);
    // 読書中でない行は普段どおり
    expect(passes(row(2, true))).toBe(false);
    expect(passes(row(3, false))).toBe(true);
  });

  it('離れたら（予約が無ければ）普段の述語に戻り、既読の行は消える', () => {
    expect(passesWhileReading(unreadOnly, null)(row(1, true))).toBe(false);
  });

  it('時間が来る前は普段の述語のまま（行はもともと未読）', () => {
    const passes = passesWhileReading(unreadOnly, { rowId: 1, ids: [1], read: false });
    expect(passes).toBe(unreadOnly);
  });

  it('未読以外の条件は読書中の行にもそのまま当てる', () => {
    const starredUnread = (m: ThreadListItem) => !m.is_read && m.is_starred;
    const passes = passesWhileReading(starredUnread, { rowId: 1, ids: [1], read: true });
    expect(passes({ ...row(1, true), is_starred: false })).toBe(false);
    expect(passes({ ...row(1, true), is_starred: true })).toBe(true);
  });
});

describe('useReadAfterDelay', () => {
  let container: HTMLDivElement;
  let root: Root;
  let hidden = false;
  let focused = true;
  let handle: Pick<ReadTimer, 'start' | 'cancel'> | null = null;

  function Probe({ onElapsed, tabActive }: { onElapsed: () => void; tabActive: boolean }) {
    handle = useReadAfterDelay(onElapsed, tabActive);
    return null;
  }
  const render = (onElapsed: () => void, tabActive = true) =>
    act(() => root.render(<Probe onElapsed={onElapsed} tabActive={tabActive} />));
  const setHidden = (v: boolean) =>
    act(() => {
      hidden = v;
      document.dispatchEvent(new Event('visibilitychange'));
    });
  const setFocused = (v: boolean) =>
    act(() => {
      focused = v;
      window.dispatchEvent(new Event(v ? 'focus' : 'blur'));
    });

  beforeEach(() => {
    (globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
      true;
    hidden = false;
    focused = true;
    vi.spyOn(document, 'visibilityState', 'get').mockImplementation(() =>
      hidden ? 'hidden' : 'visible'
    );
    vi.spyOn(document, 'hasFocus').mockImplementation(() => focused);
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    handle = null;
  });

  it('表に出ていれば 5 秒で既読にする', () => {
    const onElapsed = vi.fn();
    render(onElapsed);
    act(() => handle?.start());
    act(() => vi.advanceTimersByTime(READ_AFTER_MS));
    expect(onElapsed).toHaveBeenCalledTimes(1);
  });

  it('最小化（見えない）の間は数えない', () => {
    const onElapsed = vi.fn();
    render(onElapsed);
    act(() => handle?.start());
    act(() => vi.advanceTimersByTime(1_000));
    setHidden(true);
    act(() => vi.advanceTimersByTime(60_000));
    expect(onElapsed).not.toHaveBeenCalled();
    setHidden(false);
    act(() => vi.advanceTimersByTime(READ_AFTER_MS - 1_000));
    expect(onElapsed).toHaveBeenCalledTimes(1);
  });

  it('窓が裏に回った（フォーカスが外れた）間は数えない', () => {
    const onElapsed = vi.fn();
    render(onElapsed);
    act(() => handle?.start());
    setFocused(false);
    act(() => vi.advanceTimersByTime(60_000));
    expect(onElapsed).not.toHaveBeenCalled();
    setFocused(true);
    act(() => vi.advanceTimersByTime(READ_AFTER_MS));
    expect(onElapsed).toHaveBeenCalledTimes(1);
  });

  it('メールのタブが隠れている間は数えない', () => {
    const onElapsed = vi.fn();
    render(onElapsed);
    act(() => handle?.start());
    render(onElapsed, false);
    act(() => vi.advanceTimersByTime(60_000));
    expect(onElapsed).not.toHaveBeenCalled();
    render(onElapsed, true);
    act(() => vi.advanceTimersByTime(READ_AFTER_MS));
    expect(onElapsed).toHaveBeenCalledTimes(1);
  });

  it('画面を離れたら（アンマウント）既読にしない', () => {
    const onElapsed = vi.fn();
    render(onElapsed);
    act(() => handle?.start());
    act(() => root.unmount());
    vi.advanceTimersByTime(READ_AFTER_MS * 2);
    expect(onElapsed).not.toHaveBeenCalled();
    // afterEach の unmount を二重にしない
    root = createRoot(container);
  });

  it('時間が来たら最新の関数を呼ぶ', () => {
    const first = vi.fn();
    const latest = vi.fn();
    render(first);
    act(() => handle?.start());
    render(latest);
    act(() => vi.advanceTimersByTime(READ_AFTER_MS));
    expect(first).not.toHaveBeenCalled();
    expect(latest).toHaveBeenCalledTimes(1);
  });
});
