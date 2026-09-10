// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import { AutoLinkText, HtmlText } from './HtmlText';

/**
 * 引用の表示（縦線＋段ごとの色）を固定する。段の深さは「入れ子の数」で、色は
 * 3 色の循環（4 段目は 1 段目と同じ色）。プレーンの「>」と HTML の blockquote で
 * 同じ見た目になることも確かめる。
 */
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

/** 引用ブロックを外側から順に返す（DOM 順＝浅い段が先）。 */
const quotes = (): HTMLQuoteElement[] => [...container.querySelectorAll('blockquote')];

describe('AutoLinkText の引用表示', () => {
  it('引用の無い本文は引用ブロックを作らない', () => {
    act(() => root.render(<AutoLinkText text={'こんにちは\n本文です'} />));
    expect(quotes()).toHaveLength(0);
    expect(container.textContent).toContain('本文です');
  });

  it('「>」の連なりが段の入れ子になり、記号は本文に残らない', () => {
    act(() => root.render(<AutoLinkText text={'地の文\n> 一段目\n>> 二段目'} />));
    const qs = quotes();
    expect(qs).toHaveLength(2);
    // 二段目は一段目の内側にある。
    expect(qs[0].contains(qs[1])).toBe(true);
    expect(container.textContent).toContain('一段目');
    expect(container.textContent).not.toContain('>');
  });

  it('段ごとに縦線の色が変わり、4 段目で 1 段目に戻る', () => {
    act(() => root.render(<AutoLinkText text={'> 1\n>> 2\n>>> 3\n>>>> 4'} />));
    const cls = quotes().map((q) => q.className);
    expect(cls).toHaveLength(4);
    expect(cls[0]).toContain('border-sky-400/50');
    expect(cls[1]).toContain('border-emerald-400/50');
    expect(cls[2]).toContain('border-amber-400/50');
    // 4 段目は循環して 1 段目と同じ色。
    expect(cls[3]).toContain('border-sky-400/50');
  });

  it('引用の後に地の文が戻ると、引用ブロックの外へ出る', () => {
    act(() => root.render(<AutoLinkText text={'上\n> 引用\n下'} />));
    const q = quotes()[0];
    expect(q.textContent).toContain('引用');
    expect(q.textContent).not.toContain('下');
  });

  it('引用符の前に軽い字下げがあっても段として数える', () => {
    act(() => root.render(<AutoLinkText text={'  > 字下げ引用'} />));
    expect(quotes()).toHaveLength(1);
  });
});

describe('HtmlText の引用表示', () => {
  it('入れ子の blockquote が段ごとの色で描かれる', () => {
    act(() =>
      root.render(<HtmlText html={'<blockquote>一段目<blockquote>二段目</blockquote></blockquote>'} />),
    );
    const cls = quotes().map((q) => q.className);
    expect(cls).toHaveLength(2);
    expect(cls[0]).toContain('border-sky-400/50');
    expect(cls[1]).toContain('border-emerald-400/50');
  });

  it('引用でないブロック要素は引用ブロックにしない', () => {
    act(() => root.render(<HtmlText html={'<div>本文</div><p>段落</p>'} />));
    expect(quotes()).toHaveLength(0);
  });
});
