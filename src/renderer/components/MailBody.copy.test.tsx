// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import { AutoLinkText } from './HtmlText';
import { makeRenderDate } from './MailBody';

/**
 * 本文の日付の＋（DateAdd）を、選んで写したときに行が割れない作りに固定する。
 * flex の中身はブロック扱いになり、Chromium / WebView2 では写すと前後に改行が入るので、
 * 日付の部品とその中に inline-flex を使わない（2026-10-20 に「12:50」「:57 に …」と割れた件）。
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

describe('本文の日付の＋', () => {
  const line = '2026/10/14 12:50:57 に yamada@example.jp さんが書きました:';
  const renderDate = makeRenderDate(() => {}, { baseISO: '2026-10-20T00:00:00+09:00' });

  it('秒まで含めて 1 つの部品にし、文字は 1 行のまま', () => {
    act(() => root.render(<AutoLinkText text={line} renderDate={renderDate} />));
    const pre = container.querySelector('pre');
    expect(pre?.textContent).toBe(line);
    const button = container.querySelector('button');
    expect(button?.previousElementSibling?.textContent).toBe('2026/10/14 12:50:57');
  });

  it('部品にも＋ボタンにも inline-flex を使わない（写すと改行が入るため）', () => {
    act(() => root.render(<AutoLinkText text={line} renderDate={renderDate} />));
    expect(container.querySelector('button')).not.toBeNull();
    expect(container.querySelectorAll('.inline-flex, .flex')).toHaveLength(0);
  });

  it('＋ボタンは幅 0（見えない間も、現れても、文字の位置を動かさない）', () => {
    act(() => root.render(<AutoLinkText text={line} renderDate={renderDate} />));
    expect(container.querySelector('button')?.className).toContain('w-0');
  });
});
