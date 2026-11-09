// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';

/**
 * 本文のリンクを差出人の信頼で開かせる／止める（docs/MAIL_SECURITY.md §2.1）。
 * HTML の <a> と、本文の URL を自動でリンクにしたものの両方で、同じ振る舞いになることを確かめる。
 */
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (k: string) => k }) }));

const { HtmlText, AutoLinkText } = await import('./HtmlText');
const { LinkGuard } = await import('./LinkGuard');

let container: HTMLDivElement;
let root: Root;
let opened: string[];

beforeEach(() => {
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
  opened = [];
  vi.spyOn(window, 'open').mockImplementation((url) => {
    opened.push(String(url));
    return null;
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
});

const URL_ = 'https://shop.example.invalid/sale';

/** HTML の <a> と、プレーン本文の自動リンクの 2 通りで描く。 */
const bodies = {
  html: () => <HtmlText html={`<p><a href="${URL_}">セール会場</a></p>`} />,
  plain: () => <AutoLinkText text={`詳しくは ${URL_} をご覧ください`} />,
};

function link(): HTMLAnchorElement {
  const a = container.querySelector('a');
  if (!a) throw new Error('リンクが描かれていない');
  return a;
}

describe.each(Object.entries(bodies))('本文のリンク（%s）', (_name, Body) => {
  it('信頼できる差出人なら左クリックで開く', () => {
    act(() =>
      root.render(
        <LinkGuard trust="open">
          <Body />
        </LinkGuard>
      )
    );
    act(() => link().click());
    expect(opened).toEqual([URL_]);
    expect(link().title).toBe(URL_);
  });

  it('止めた差出人は左クリックで開かず、右クリックで開けると案内する', () => {
    act(() =>
      root.render(
        <LinkGuard trust="guarded">
          <Body />
        </LinkGuard>
      )
    );
    expect(link().className).not.toContain('text-sky-400');
    act(() => link().click());
    expect(opened).toEqual([]);
    expect(document.body.textContent).toContain('link.guardedHint');
  });

  it('右クリックのメニューに行き先を出し、そこから開ける', () => {
    act(() =>
      root.render(
        <LinkGuard trust="guarded">
          <Body />
        </LinkGuard>
      )
    );
    act(() => {
      link().dispatchEvent(
        new MouseEvent('contextmenu', { bubbles: true, clientX: 10, clientY: 10 })
      );
    });
    expect(document.body.textContent).toContain(URL_);
    const openItem = [...document.body.querySelectorAll('button')].find(
      (b) => b.textContent === 'link.open'
    );
    expect(openItem).toBeDefined();
    act(() => openItem?.click());
    expect(opened).toEqual([URL_]);
  });
});

it('LinkGuard で包まれていない本文（印刷など）はいまどおり開く', () => {
  act(() => root.render(<AutoLinkText text={URL_} />));
  act(() => link().click());
  expect(opened).toEqual([URL_]);
});
