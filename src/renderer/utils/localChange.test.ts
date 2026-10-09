// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { changed, debounce, LOCAL_CHANGE_EVENT } from './localChange';

describe('debounce', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('続けて呼んでも、最後の呼び出しから待って 1 回だけ呼ぶ', () => {
    const fn = vi.fn();
    const d = debounce(fn, 3000);
    d.call();
    vi.advanceTimersByTime(1000);
    d.call();
    vi.advanceTimersByTime(2999);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('取り消すと呼ばない', () => {
    const fn = vi.fn();
    const d = debounce(fn, 100);
    d.call();
    d.cancel();
    vi.advanceTimersByTime(1000);
    expect(fn).not.toHaveBeenCalled();
  });
});

describe('changed', () => {
  it('成功したら合図を出し、失敗したら出さない', async () => {
    const seen = vi.fn();
    window.addEventListener(LOCAL_CHANGE_EVENT, seen);
    await expect(changed(Promise.resolve(7))).resolves.toBe(7);
    expect(seen).toHaveBeenCalledTimes(1);
    await expect(changed(Promise.reject(new Error('x')))).rejects.toThrow('x');
    expect(seen).toHaveBeenCalledTimes(1);
    window.removeEventListener(LOCAL_CHANGE_EVENT, seen);
  });
});
