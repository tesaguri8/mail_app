// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import { Tooltip } from './Tooltip';

/**
 * 不具合の再現条件（下からカーソルを乗せると出ない）は native の title 依存だったため
 * 再現テストは書けない。代わりに「乗せれば出る・離せば消える」という、こちらが持つように
 * なった表示条件を固定する。
 */
let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

const render = () =>
  act(() => {
    root.render(
      <Tooltip label="スター">
        <button aria-label="スター">★</button>
      </Tooltip>,
    );
  });

/** Tooltip が包んでいる span（ホバーを受ける側）。 */
const wrapper = () => container.firstElementChild as HTMLElement;
const tooltipText = () => container.querySelector('[role="tooltip"]')?.textContent ?? null;

describe('Tooltip', () => {
  it('乗せた直後は出ず、待ち時間の経過後に出る', () => {
    render();
    expect(tooltipText()).toBeNull();

    act(() => {
      wrapper().dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
    });
    expect(tooltipText()).toBeNull(); // まだ待ち時間の途中

    act(() => void vi.advanceTimersByTime(300));
    expect(tooltipText()).toBe('スター');
  });

  it('離すと消える', () => {
    render();
    act(() => {
      wrapper().dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
      vi.advanceTimersByTime(300);
    });
    expect(tooltipText()).toBe('スター');

    act(() => {
      wrapper().dispatchEvent(new MouseEvent('mouseout', { bubbles: true }));
    });
    expect(tooltipText()).toBeNull();
  });

  it('待ち時間の途中で離せば出ない（通りすがりでは出さない）', () => {
    render();
    act(() => {
      wrapper().dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
      vi.advanceTimersByTime(150);
      wrapper().dispatchEvent(new MouseEvent('mouseout', { bubbles: true }));
      vi.advanceTimersByTime(300);
    });
    expect(tooltipText()).toBeNull();
  });

  it('押したら消える（クリック後に残らない）', () => {
    render();
    act(() => {
      wrapper().dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
      vi.advanceTimersByTime(300);
    });
    expect(tooltipText()).toBe('スター');

    act(() => {
      wrapper().dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    });
    expect(tooltipText()).toBeNull();
  });

  it('キーボードで辿っても出る（native title には無い挙動）', () => {
    render();
    act(() => {
      container.querySelector('button')!.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
      vi.advanceTimersByTime(300);
    });
    expect(tooltipText()).toBe('スター');
  });

  it('包んだボタンはそのまま描画され、押せる', () => {
    render();
    const btn = container.querySelector('button')!;
    expect(btn.getAttribute('aria-label')).toBe('スター');
    // native の title は外してある（二重表示を避けるため）
    expect(btn.hasAttribute('title')).toBe(false);
  });
});
