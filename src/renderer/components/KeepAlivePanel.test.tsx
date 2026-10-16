// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import { useEffect, useState } from 'react';
import { KeepAlivePanel } from './KeepAlivePanel';
import { useOnTabActivated, useTabActive, useTabActiveRef } from '../hooks/useTabActive';

// タブの作業状態を保つ決まり: 一度開いた画面は捨てずに隠す／初めて開くまでは描かない／
// 隠れている間は全体のキー操作を受けない／戻ったら読み直しとフォーカスの復帰をする。

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

/** 入力欄・表示中かの印・Del キーの受け取り回数を持つ、画面の代わり。 */
function Screen({ onDel, onActivated }: { onDel: () => void; onActivated: () => void }) {
  const [text, setText] = useState('');
  const active = useTabActive();
  const activeRef = useTabActiveRef();
  useOnTabActivated(onActivated);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!activeRef.current) return;
      if (e.key === 'Delete') onDel();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [activeRef, onDel]);
  return (
    <div>
      <span data-testid="active">{String(active)}</span>
      <textarea value={text} onChange={(e) => setText(e.target.value)} />
    </div>
  );
}

const setup = () => {
  const onDel = vi.fn();
  const onActivated = vi.fn();
  const render = (active: boolean) =>
    act(() => {
      root.render(
        <KeepAlivePanel active={active}>
          <Screen onDel={onDel} onActivated={onActivated} />
        </KeepAlivePanel>
      );
    });
  return { onDel, onActivated, render };
};

const panel = () => container.firstElementChild as HTMLElement | null;
const textarea = () => container.querySelector('textarea') as HTMLTextAreaElement;
const pressDelete = () =>
  act(() => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete' }));
  });

describe('KeepAlivePanel', () => {
  it('初めて表示されるまでは描かない', () => {
    const { render } = setup();
    render(false);
    expect(panel()).toBeNull();
    render(true);
    expect(textarea()).not.toBeNull();
  });

  it('隠しても捨てず、戻ると同じ部品のまま（状態が残る）', () => {
    const { render } = setup();
    render(true);
    const before = textarea();
    render(false);
    expect(textarea()).toBe(before);
    expect(panel()?.style.visibility).toBe('hidden');
    expect(panel()?.hasAttribute('inert')).toBe(true);
    expect(container.querySelector('[data-testid="active"]')?.textContent).toBe('false');
    render(true);
    expect(textarea()).toBe(before);
    expect(panel()?.style.visibility).toBe('');
    expect(panel()?.hasAttribute('inert')).toBe(false);
    expect(container.querySelector('[data-testid="active"]')?.textContent).toBe('true');
  });

  it('隠れている間は全体のキー操作を受けない', () => {
    const { render, onDel } = setup();
    render(true);
    pressDelete();
    expect(onDel).toHaveBeenCalledTimes(1);
    render(false);
    pressDelete();
    expect(onDel).toHaveBeenCalledTimes(1);
    render(true);
    pressDelete();
    expect(onDel).toHaveBeenCalledTimes(2);
  });

  it('戻ったときだけ読み直しを呼ぶ（初めて描いたときは呼ばない）', () => {
    const { render, onActivated } = setup();
    render(true);
    expect(onActivated).not.toHaveBeenCalled();
    render(false);
    expect(onActivated).not.toHaveBeenCalled();
    render(true);
    expect(onActivated).toHaveBeenCalledTimes(1);
  });

  it('戻ったら、隠す前にフォーカスしていた欄へフォーカスを戻す', () => {
    const { render } = setup();
    render(true);
    act(() => textarea().focus());
    render(false);
    act(() => (document.activeElement as HTMLElement | null)?.blur());
    expect(document.activeElement).not.toBe(textarea());
    render(true);
    expect(document.activeElement).toBe(textarea());
  });
});
