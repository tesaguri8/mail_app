// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { TsgOneStatus } from '@bindings/TsgOneStatus';

/**
 * 設定の「TSG One」: ログイン前・ログイン待ち・ログイン後・枠超え・失敗の出し分け。
 * Rust のコマンドは差し替える（tsg-core へは何も送らない）。
 */
(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (k: string, o?: Record<string, string>) => (o?.provider ? `${k}:${o.provider}` : k),
  }),
}));

let listener: ((e: { payload: TsgOneStatus }) => void) | null = null;
vi.mock('@tauri-apps/api/event', () => ({
  listen: (_name: string, cb: (e: { payload: TsgOneStatus }) => void) => {
    listener = cb;
    return Promise.resolve(() => undefined);
  },
}));

const signedOut: TsgOneStatus = {
  user: null,
  pending: false,
  over_cap: false,
  problem: null,
  problem_detail: null,
};
const signedIn: TsgOneStatus = {
  ...signedOut,
  user: { id: 'u1', email: 'taro@example.com', display_name: 'Taro', provider: 'google' },
};

const calls: string[] = [];
let current: TsgOneStatus = signedOut;
vi.mock('../services/tsgOne', () => ({
  TSG_ONE_CHANGED_EVENT: 'tsg-one:changed',
  tsgOneStatus: () => Promise.resolve(current),
  tsgOneLogin: (p: string) => {
    calls.push(`login:${p}`);
    return Promise.resolve({ ...signedOut, pending: true });
  },
  tsgOneCancelLogin: () => {
    calls.push('cancel');
    return Promise.resolve(signedOut);
  },
  tsgOneLogout: () => {
    calls.push('logout');
    return Promise.resolve(signedOut);
  },
  tsgOneRecheckDevice: () => {
    calls.push('recheck');
    return Promise.resolve(signedIn);
  },
  tsgOneOpenMemberSite: () => {
    calls.push('member');
    return Promise.resolve();
  },
}));

const { TsgOneSettings } = await import('./TsgOneSettings');

let container: HTMLDivElement;
let root: Root;

async function mount(status: TsgOneStatus) {
  current = status;
  await act(async () => {
    root.render(<TsgOneSettings />);
  });
}

function button(label: string): HTMLButtonElement {
  const b = [...container.querySelectorAll('button')].find((x) => x.textContent === label);
  if (!b) throw new Error(`ボタンが無い: ${label}`);
  return b;
}

async function click(label: string) {
  await act(async () => {
    button(label).click();
  });
}

beforeEach(() => {
  calls.length = 0;
  listener = null;
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe('TsgOneSettings', () => {
  it('ログイン前は提供元ごとのログインを出し、押すとログインを始めて待つ', async () => {
    await mount(signedOut);
    expect(container.querySelector('[data-tsg-signed-out]')).not.toBeNull();
    expect(button('tsgOne.loginWith:GitHub')).toBeTruthy();
    await click('tsgOne.loginWith:Google');
    expect(calls).toEqual(['login:google']);
    expect(container.querySelector('[data-tsg-pending]')).not.toBeNull();
    // 待っている間は、ほかのログインを始めない
    expect(button('tsgOne.loginWith:Apple').disabled).toBe(true);
    await click('tsgOne.cancel');
    expect(calls).toEqual(['login:google', 'cancel']);
    expect(container.querySelector('[data-tsg-pending]')).toBeNull();
  });

  it('ブラウザから戻ったイベントでログイン中のアカウントに替わる', async () => {
    await mount({ ...signedOut, pending: true });
    await act(async () => listener?.({ payload: signedIn }));
    expect(container.querySelector('[data-tsg-signed-in]')?.textContent).toContain(
      'taro@example.com'
    );
    expect(container.textContent).toContain('tsgOne.via:Google');
    await click('tsgOne.logout');
    expect(calls).toEqual(['logout']);
    expect(container.querySelector('[data-tsg-signed-out]')).not.toBeNull();
  });

  it('台数の枠を超えたら、会員サイトへの案内と確かめ直しを出す', async () => {
    await mount({ ...signedIn, over_cap: true });
    expect(container.querySelector('[data-tsg-over-cap]')).not.toBeNull();
    await click('tsgOne.openMemberSite');
    await click('tsgOne.recheck');
    expect(calls).toEqual(['member', 'recheck']);
    expect(container.querySelector('[data-tsg-over-cap]')).toBeNull();
  });

  it('失敗は種類ごとの文言と補足で出す', async () => {
    await mount({ ...signedOut, problem: 'relogin', problem_detail: 'HTTP 400 invalid_grant' });
    const box = container.querySelector('[data-tsg-problem]');
    expect(box?.textContent).toContain('tsgOne.problem.relogin');
    expect(box?.textContent).toContain('HTTP 400 invalid_grant');
  });
});
