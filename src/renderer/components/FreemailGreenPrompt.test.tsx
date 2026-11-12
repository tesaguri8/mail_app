// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { ContactSummary } from '@bindings/ContactSummary';

/** フリーメールの葉っぱ: 住所録に登録して「この人だけ」グリーンにする確認。 */
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (k: string) => k }) }));
const find = vi.fn();
const create = vi.fn();
const restore = vi.fn();
vi.mock('../utils/senderContact', () => ({
  findSenderInBook: (...a: unknown[]) => find(...a),
  createSenderContact: (...a: unknown[]) => create(...a),
  restoreSenderContact: (...a: unknown[]) => restore(...a),
  senderDisplayName: (n: string | null, a: string) => n?.trim() || a,
}));

const { FreemailGreenPrompt } = await import('./FreemailGreenPrompt');

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
  find.mockReset();
  create.mockReset();
  restore.mockReset();
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const flush = () => act(async () => await Promise.resolve());
const button = (label: string) =>
  Array.from(container.querySelectorAll('button')).find((b) => b.textContent === label);
const render = (onGreen = vi.fn(), onClose = vi.fn()) => {
  act(() =>
    root.render(
      <FreemailGreenPrompt
        domain="gmail.com"
        name="山田 太郎"
        address="taro@gmail.invalid"
        onGreen={onGreen}
        onClose={onClose}
      />
    )
  );
  return { onGreen, onClose };
};

describe('FreemailGreenPrompt', () => {
  it('住所録に無ければ「登録する」で作り、グリーンにする', async () => {
    find.mockResolvedValue({ kind: 'none' });
    create.mockResolvedValue({ id: 1 });
    const { onGreen } = render();
    await flush();
    expect(container.textContent).toContain('green.freemailAsk');
    // 狭い幅でも切らずに折り返す（truncate を使わない）。
    expect(container.querySelector('.truncate')).toBeNull();
    expect(container.querySelector('p')?.className).toContain('break-words');
    await act(async () => button('green.freemailRegister')?.click());
    await flush();
    expect(create).toHaveBeenCalledWith('山田 太郎', 'taro@gmail.invalid');
    expect(onGreen).toHaveBeenCalledTimes(1);
    expect(container.textContent).toContain('green.freemailRegistered');
  });

  it('削除されていない連絡先が既にあれば作らない', async () => {
    find.mockResolvedValue({
      kind: 'alive',
      contact: { id: 2, display_name: '山田' } as ContactSummary,
    });
    render();
    await flush();
    expect(container.textContent).toContain('green.freemailAlive');
    expect(button('green.freemailRegister')).toBeUndefined();
    expect(create).not.toHaveBeenCalled();
  });

  it('ゴミ箱にだけあれば作らず、戻すかを訊く', async () => {
    const c = { id: 3, display_name: '山田' } as ContactSummary;
    find.mockResolvedValue({ kind: 'trashed', contact: c });
    restore.mockResolvedValue(undefined);
    const { onGreen } = render();
    await flush();
    expect(container.textContent).toContain('green.freemailTrashed');
    await act(async () => button('green.freemailRestore')?.click());
    await flush();
    expect(restore).toHaveBeenCalledWith(c);
    expect(create).not.toHaveBeenCalled();
    expect(onGreen).toHaveBeenCalledTimes(1);
  });

  it('「やめる」で閉じ、何も作らない', async () => {
    find.mockResolvedValue({ kind: 'none' });
    const { onClose, onGreen } = render();
    await flush();
    act(() => button('green.freemailCancel')?.click());
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(create).not.toHaveBeenCalled();
    expect(onGreen).not.toHaveBeenCalled();
  });
});
