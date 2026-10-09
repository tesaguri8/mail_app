// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { GoogleAccount } from '@bindings/GoogleAccount';

// 自動同期の決まり（利用者の判断 2026-10-09）: どの画面にいても動く／連絡先も毎回同期する／
// 連絡先を変えたら、まとめ待ちのあと Google だけを同期する。

const mocks = vi.hoisted(() => {
  // isTauri の判定（モジュールを読む時点で見る）。
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  return {
    mailSync: vi.fn(async () => ({ stored: 0 })),
    googleSync: vi.fn(async () => ({
      calendar: null,
      calendar_error: null,
      contacts: null,
      matched: null,
      contacts_error: null,
    })),
    googleAccounts: vi.fn(async () => [] as GoogleAccount[]),
    interval: { sec: 30 },
  };
});

vi.mock('../services/mail', () => ({ mailSync: mocks.mailSync }));
vi.mock('../services/google', () => ({
  googleAccounts: mocks.googleAccounts,
  googleSync: mocks.googleSync,
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => undefined }));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (k: string) => k }) }));
vi.mock('../config/prefs', () => ({
  getAutoSyncInterval: () => mocks.interval.sec,
  PREFS_EVENT: 'rondine:prefs',
}));

import { useAutoSync } from './useAutoSync';
import { notifyLocalChange, LOCAL_CHANGE_DEBOUNCE_MS } from '../utils/localChange';

const google = (over: Partial<GoogleAccount> = {}): GoogleAccount => ({
  id: 1,
  email: 'a@gmail.com',
  sync_calendar: true,
  sync_contacts: true,
  push_new_contacts: false,
  last_calendar_sync_at: null,
  last_contacts_sync_at: null,
  disconnected_at: null,
  calendar_granted: true,
  contacts_granted: true,
  ...over,
});

const mailAccount = { id: 10, email: 'm@x.jp' } as AccountSummary;

function Harness({ accounts }: { accounts: AccountSummary[] }) {
  useAutoSync(accounts);
  return null;
}

let container: HTMLDivElement;
let root: Root;

/** 待ち時間を進め、その間に積まれた同期（Promise）も流しきる。 */
const advance = (ms: number) =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });

beforeEach(() => {
  vi.useFakeTimers();
  mocks.mailSync.mockClear();
  mocks.googleSync.mockClear();
  mocks.googleAccounts.mockReset();
  mocks.googleAccounts.mockResolvedValue([google()]);
  mocks.interval.sec = 30;
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

const mount = async (accounts: AccountSummary[] = [mailAccount]) => {
  await act(async () => {
    root.render(<Harness accounts={accounts} />);
  });
  await advance(0);
};

describe('useAutoSync', () => {
  it('起動直後に同期し、連絡先も毎回同期する（10 分の間隔は無い）', async () => {
    await mount();
    expect(mocks.mailSync).toHaveBeenCalledWith(10);
    expect(mocks.googleSync).toHaveBeenLastCalledWith(1, true);

    // 次の巡回（30 秒後）でも連絡先を同期する。
    await advance(30_000);
    expect(mocks.googleSync).toHaveBeenCalledTimes(2);
    expect(mocks.googleSync).toHaveBeenLastCalledWith(1, true);
  });

  it('画面の指定は無く、設定の間隔でずっと回る（どの画面にいても動く）', async () => {
    await mount();
    await advance(90_000);
    expect(mocks.mailSync).toHaveBeenCalledTimes(4);
  });

  it('連絡先を変えたら、まとめ待ちのあと Google だけを 1 回同期する', async () => {
    await mount();
    mocks.mailSync.mockClear();
    mocks.googleSync.mockClear();

    // 続けて 3 回変えても、最後の変更から待って 1 回。
    notifyLocalChange();
    await advance(1000);
    notifyLocalChange();
    notifyLocalChange();
    await advance(LOCAL_CHANGE_DEBOUNCE_MS - 1);
    expect(mocks.googleSync).not.toHaveBeenCalled();
    await advance(1);
    expect(mocks.googleSync).toHaveBeenCalledTimes(1);
    expect(mocks.googleSync).toHaveBeenLastCalledWith(1, true);
    // メールのサーバーは叩かない。
    expect(mocks.mailSync).not.toHaveBeenCalled();
  });

  it('自動同期をオフ（0）にしていても、変更は送る', async () => {
    mocks.interval.sec = 0;
    await mount();
    mocks.googleSync.mockClear();
    await advance(60_000);
    expect(mocks.googleSync).not.toHaveBeenCalled();
    notifyLocalChange();
    await advance(LOCAL_CHANGE_DEBOUNCE_MS);
    expect(mocks.googleSync).toHaveBeenCalledTimes(1);
  });

  it('連絡先の同期をしていないアカウントは、連絡先を同期しない', async () => {
    mocks.googleAccounts.mockResolvedValue([google({ sync_contacts: false })]);
    await mount();
    expect(mocks.googleSync).toHaveBeenLastCalledWith(1, false);
  });
});
