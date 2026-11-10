// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import { BodyZoomLayer, ZoomArea, composeZoomProps } from './BodyZoom';

/**
 * 本文の拡大・縮小（docs/UI_UX_DESIGN.md §8）。キーとホイールを 1 か所で受け、
 * ポインタが乗っている欄の倍率だけを変え、欄の外でも画面全体のズームを起こさないこと。
 */
let container: HTMLDivElement;
let root: Root;
let under: Element | null;

const zoomOf = (field: string) =>
  document.documentElement.style.getPropertyValue(`--body-zoom-${field}`);

function pointAt(el: Element | null) {
  under = el;
  window.dispatchEvent(new PointerEvent('pointermove', { clientX: 10, clientY: 10 }));
}

function press(key: string): KeyboardEvent {
  const e = new KeyboardEvent('keydown', { key, ctrlKey: true, cancelable: true, bubbles: true });
  act(() => {
    window.dispatchEvent(e);
  });
  return e;
}

beforeEach(() => {
  vi.useFakeTimers();
  localStorage.clear();
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
  under = null;
  // happy-dom は位置で要素を引けないので、最後に指した要素を返す。
  document.elementFromPoint = () => under;
  act(() =>
    root.render(
      <>
        <BodyZoomLayer />
        <ZoomArea field="bubble">
          <p id="bubble">本文</p>
        </ZoomArea>
        <textarea id="compose" {...composeZoomProps} />
        <button id="outside">返信</button>
      </>
    )
  );
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

const $ = (id: string) => document.getElementById(id);

describe('BodyZoomLayer', () => {
  it('ポインタが乗っている欄だけを拡大し、倍率を短く出して保存する', () => {
    pointAt($('bubble'));
    const e = press('+');
    expect(e.defaultPrevented).toBe(true);
    expect(zoomOf('bubble')).toBe('1.1');
    expect(zoomOf('compose')).toBe('1');
    expect(localStorage.getItem('rondine.bodyZoom.bubble')).toBe('1.1');
    expect(document.body.textContent).toContain('110%');
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(document.body.textContent).not.toContain('110%');
  });

  it('作成の本文欄は別の倍率。Ctrl + 0 で 100% に戻る', () => {
    pointAt($('compose'));
    press('-');
    expect(zoomOf('compose')).toBe('0.9');
    press('0');
    expect(zoomOf('compose')).toBe('1');
  });

  it('欄の外では倍率を変えないが、画面全体のズームは打ち消す', () => {
    pointAt($('outside'));
    const e = press('=');
    expect(e.defaultPrevented).toBe(true);
    expect(zoomOf('bubble')).toBe('1');
  });

  it('Ctrl + ホイールは欄の上で 1 段ずつ、欄の外では打ち消すだけ', () => {
    const wheel = (el: Element | null, deltaY: number) => {
      const e = new WheelEvent('wheel', { deltaY, cancelable: true, bubbles: true });
      // happy-dom の WheelEvent は ctrlKey を受け取らないので、後から立てる。
      Object.defineProperty(e, 'ctrlKey', { value: true });
      act(() => {
        el?.dispatchEvent(e);
      });
      return e;
    };
    expect(wheel($('bubble'), -100).defaultPrevented).toBe(true);
    expect(zoomOf('bubble')).toBe('1.1');
    expect(wheel($('outside'), -100).defaultPrevented).toBe(true);
    expect(zoomOf('bubble')).toBe('1.1');
  });

  it('Ctrl の無いホイールは止めない（ふつうのスクロール）', () => {
    const e = new WheelEvent('wheel', { deltaY: 100, cancelable: true, bubbles: true });
    $('bubble')?.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(false);
  });

  it('別ウィンドウで変わった倍率に追随する', () => {
    localStorage.setItem('rondine.bodyZoom.source', '1.5');
    act(() => {
      window.dispatchEvent(new StorageEvent('storage', { key: 'rondine.bodyZoom.source' }));
    });
    expect(zoomOf('source')).toBe('1.5');
  });
});
