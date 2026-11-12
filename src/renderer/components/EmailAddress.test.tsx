// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { ContactSummary } from '@bindings/ContactSummary';

/**
 * メール詳細のアドレスの＋/編集と右クリックメニュー。
 * - 差出人の＋は省略（truncate）の外に置く（中に置くと overflow:hidden で丸が切れる）
 * - 右クリックのメニューは＋と同じ判定（willEdit）・同じ文言
 */
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (k: string, o?: Record<string, unknown>) => (o ? `${k}:${JSON.stringify(o)}` : k),
  }),
}));
const lookup = vi.fn();
vi.mock('../services/contacts', () => ({ contactLookupEmail: (...a: unknown[]) => lookup(...a) }));
const copied: string[] = [];
vi.mock('../utils/clipboard', () => ({
  copyText: async (s: string) => {
    copied.push(s);
  },
}));

const { SenderAddressLine, LinkifyEmails, clearEmailLookupCache } = await import('./EmailAddress');

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
  lookup.mockReset();
  copied.length = 0;
  clearEmailLookupCache();
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const flush = () => act(async () => await Promise.resolve());
const person = (id: number, name: string) =>
  ({ id, display_name: name }) as unknown as ContactSummary;

describe('SenderAddressLine', () => {
  it('＋は省略される塊（truncate）の外に、幅を取って置く', async () => {
    lookup.mockResolvedValue([]);
    const onAdd = vi.fn();
    act(() =>
      root.render(
        <SenderAddressLine
          name="山田 太郎"
          address="taro@gmail.invalid"
          prefix="差出人: "
          onAdd={onAdd}
        />
      )
    );
    await flush();
    const add = container.querySelector(
      'button[aria-label="mailbox.addContact"]'
    ) as HTMLButtonElement;
    expect(add).toBeTruthy();
    expect(add.closest('.truncate')).toBeNull();
    expect(add.className).not.toContain('w-0');
    // 写す文字は 1 つの inline の塊（名前・アドレス・「>」の間に改行が入らない）。
    expect(container.querySelector('.truncate')?.textContent).toBe(
      '差出人: 山田 太郎 <taro@gmail.invalid>'
    );
    act(() => add.click());
    expect(onAdd).toHaveBeenCalledWith('山田 太郎', 'taro@gmail.invalid');
  });

  it('右クリックで＋と同じ操作・コピー・新規メールを出す（名前の合う既存があれば編集）', async () => {
    lookup.mockResolvedValue([person(5, '山田太郎')]);
    const onAdd = vi.fn();
    const onEdit = vi.fn();
    const onCompose = vi.fn();
    act(() =>
      root.render(
        <SenderAddressLine
          name="山田 太郎"
          address="taro@gmail.invalid"
          prefix=""
          onAdd={onAdd}
          onEdit={onEdit}
          onCompose={onCompose}
        />
      )
    );
    await flush();
    expect(container.querySelector('button[aria-label="mailbox.editContact"]')).toBeTruthy();
    const text = Array.from(container.querySelectorAll('button')).find(
      (b) => b.textContent === 'taro@gmail.invalid'
    ) as HTMLButtonElement;
    act(() => {
      text.dispatchEvent(
        new MouseEvent('contextmenu', { bubbles: true, clientX: 10, clientY: 10 })
      );
    });
    const labels = () =>
      Array.from(document.body.querySelectorAll('button')).map((b) => b.textContent);
    expect(labels()).toContain('mailbox.editContact');
    expect(labels()).toContain('compose.recipientCopyAddress');
    expect(labels()).toContain('mailbox.composeTo:{"email":"taro@gmail.invalid"}');
    const edit = Array.from(document.body.querySelectorAll('button')).find(
      (b) => b.textContent === 'mailbox.editContact' && !b.getAttribute('aria-label')
    ) as HTMLButtonElement;
    act(() => edit.click());
    expect(onEdit).toHaveBeenCalledWith(5);
    expect(onAdd).not.toHaveBeenCalled();
  });
});

describe('LinkifyEmails', () => {
  it('宛先の並びでも右クリックで住所録に追加・コピーできる', async () => {
    lookup.mockResolvedValue([]);
    const onAdd = vi.fn();
    act(() => root.render(<LinkifyEmails text="a@x.invalid, b@y.invalid" onAdd={onAdd} />));
    await flush();
    const b = Array.from(container.querySelectorAll('span')).find(
      (s) => s.textContent === 'b@y.invalid' && s.children.length === 0
    ) as HTMLSpanElement;
    act(() => {
      b.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 5, clientY: 5 }));
    });
    const items = Array.from(document.body.querySelectorAll('button'));
    const copy = items.find(
      (x) => x.textContent === 'compose.recipientCopyAddress'
    ) as HTMLButtonElement;
    await act(async () => copy.click());
    expect(copied).toEqual(['b@y.invalid']);
    // 新規メールの口が無ければ出さない。
    expect(items.some((x) => (x.textContent ?? '').startsWith('mailbox.composeTo'))).toBe(false);
  });
});
