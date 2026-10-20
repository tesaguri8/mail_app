// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { UnseenQuote } from '@bindings/UnseenQuote';
import { UnseenQuotes } from './UnseenQuotes';

/**
 * 引用されていたメールの枠（未受信）の本文も、通常のバブルと同じく URL を押せるリンクにする。
 * 枠の本文は外側のメールの body_plain から切り出したもので、改行が \r\r\n のこともある（実測 53359）。
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

const quote = (body: string): UnseenQuote => ({
  from: '前田 太郎 <taro@maeda.invalid>',
  sender: '前田 太郎',
  at: '2026年10月15日 11:21',
  date: '2026-10-15T11:21:00',
  subject: '図面の件',
  body,
});

describe('UnseenQuotes の本文のリンク', () => {
  it('URL は通常のバブルと同じリンクになる（改行が \\r\\r\\n でも末尾に紛れない）', () => {
    const body = 'データ便（IFCデータ）\r\r\nhttps://deliver.example.invalid/d/abc123\r\r\n\r\r\nよろしくお願いします。';
    act(() => root.render(<UnseenQuotes quotes={[quote(body)]} out={false} />));
    const links = [...container.querySelectorAll('a')];
    expect(links.map((a) => a.getAttribute('href'))).toEqual([
      'https://deliver.example.invalid/d/abc123',
    ]);
    expect(links[0].textContent).toBe('https://deliver.example.invalid/d/abc123');
  });

  it('<mailto:…> の中の URL でないアドレスにはリンクを張らない（通常のバブルと同じ）', () => {
    const body = 'Email: taro@maeda.invalid <mailto:taro@maeda.invalid>\r\nwww.maeda.invalid';
    act(() => root.render(<UnseenQuotes quotes={[quote(body)]} out={false} />));
    const hrefs = [...container.querySelectorAll('a')].map((a) => a.getAttribute('href'));
    expect(hrefs).toEqual(['https://www.maeda.invalid']);
    expect(container.textContent).toContain('<mailto:taro@maeda.invalid>');
  });
});
