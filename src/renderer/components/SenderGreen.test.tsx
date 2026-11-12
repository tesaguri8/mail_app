// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { ContactSummary } from '@bindings/ContactSummary';

/**
 * 葉っぱの付け外し（useSenderGreen）。住所録の本人は葉っぱでは外せない:
 * - 本人でなければドメインを警告に回して外す
 * - 本人でドメインもグリーンなら、ドメインだけ警告に回してグリーンのまま（そう伝える）
 * - 本人でドメインはグリーンでなければ、何も書かずに理由を出す
 * どの場合も、見た目は結果どおりにしか変えない（一瞬消えて戻る、をしない）。
 */
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (k: string) => k }) }));
const add = vi.fn();
const warn = vi.fn();
vi.mock('../services/green', () => ({
  greenDomainAdd: (...a: unknown[]) => add(...a),
  greenDomainWarn: (...a: unknown[]) => warn(...a),
}));
const kept = vi.fn();
const domainGreen = vi.fn();
vi.mock('../utils/senderContact', () => ({
  findKeptContact: (...a: unknown[]) => kept(...a),
  isDomainGreen: (...a: unknown[]) => domainGreen(...a),
  findSenderInBook: vi.fn(async () => ({ kind: 'none' })),
  createSenderContact: vi.fn(),
  restoreSenderContact: vi.fn(),
  senderDisplayName: (n: string | null, a: string) => n || a,
}));

const { useSenderGreen } = await import('./SenderGreen');

let container: HTMLDivElement;
let root: Root;
let api: ReturnType<typeof useSenderGreen>;
const changed: boolean[] = [];
const opened: number[] = [];

function Probe({ domain, baseGreen }: { domain: string; baseGreen: boolean }) {
  api = useSenderGreen({
    domain,
    address: `taro@${domain}`,
    name: '山田 太郎',
    baseGreen,
    resetKey: 1,
    onChanged: (g) => changed.push(g),
    onOpenContact: (id) => opened.push(id),
  });
  return (
    <div>
      <span data-green={api.isGreen ? '1' : '0'} />
      {api.panel}
    </div>
  );
}

beforeEach(() => {
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
  [add, warn, kept, domainGreen].forEach((f) => f.mockReset());
  changed.length = 0;
  opened.length = 0;
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const green = () => container.querySelector('[data-green]')?.getAttribute('data-green');
const render = (domain: string, baseGreen: boolean) =>
  act(() => root.render(<Probe domain={domain} baseGreen={baseGreen} />));
const press = () => act(async () => await api.toggle());
const person = { id: 9, display_name: '山田 太郎' } as unknown as ContactSummary;

describe('useSenderGreen', () => {
  it('本人でなければ、ドメインを警告に回して外す', async () => {
    kept.mockResolvedValue(null);
    render('corp.invalid', true);
    await press();
    expect(warn).toHaveBeenCalledWith('corp.invalid');
    expect(green()).toBe('0');
    expect(changed).toEqual([false]);
  });

  it('本人でドメインはグリーンでない（フリーメール等）: 何も書かず、外せない理由を出す', async () => {
    kept.mockResolvedValue(person);
    domainGreen.mockResolvedValue(false);
    render('gmail.com', true);
    await press();
    expect(warn).not.toHaveBeenCalled();
    expect(green()).toBe('1');
    expect(changed).toEqual([]);
    expect(container.textContent).toContain('green.keptPerson');
    // 「住所録で開く」でその人を開く。
    const open = Array.from(container.querySelectorAll('button')).find(
      (b) => b.textContent === 'green.openContact'
    ) as HTMLButtonElement;
    act(() => open.click());
    expect(opened).toEqual([9]);
    expect(container.querySelector('[data-testid="green-kept-notice"]')).toBeNull();
  });

  it('本人でドメインもグリーン: ドメインだけ警告に回し、グリーンのまま（そう伝える）', async () => {
    kept.mockResolvedValue(person);
    domainGreen.mockResolvedValue(true);
    render('corp.invalid', true);
    await press();
    expect(warn).toHaveBeenCalledWith('corp.invalid');
    expect(green()).toBe('1');
    expect(changed).toEqual([true]);
    expect(container.textContent).toContain('green.keptDomainUnset');
  });

  it('付けるとき: フリーメールなら確認を出し、ほかは付ける', async () => {
    add.mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    render('gmail.com', false);
    await press();
    expect(green()).toBe('0');
    expect(container.querySelector('[data-testid="freemail-green-prompt"]')).toBeTruthy();
    render('corp.invalid', false);
    await press();
    expect(green()).toBe('1');
    expect(changed).toEqual([true]);
  });
});
