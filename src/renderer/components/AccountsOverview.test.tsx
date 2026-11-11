// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { AccountSyncStatus } from '@bindings/AccountSyncStatus';

/**
 * ホーム右のアカウント一覧: アドレスの左に接続状況の印（サイドバー・設定と同じ AccountStatusDot）。
 * 状態の知らせ（SYNC_STATUS_EVENT）で変わり、展開しても残り、印を押しても行と同じく展開する。
 */
const mocks = vi.hoisted(() => {
  // isTauri の判定（モジュールを読む時点で見る）。
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  return {
    listeners: new Map<string, (e: { payload: unknown }) => void>(),
    initial: [] as AccountSyncStatus[],
  };
});

vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, cb: (e: { payload: unknown }) => void) => {
    mocks.listeners.set(name, cb);
    return () => mocks.listeners.delete(name);
  },
}));
vi.mock('../services/mail', () => ({
  SYNC_STATUS_EVENT: 'sync:status',
  accountSyncStatuses: async () => mocks.initial,
  homeUnreadCounts: async () => [],
  mailList: async () => [],
}));
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => k, i18n: { language: 'ja' } }),
}));
vi.mock('../hooks/useTabActive', () => ({ useOnTabActivated: () => undefined }));

const { AccountsOverview } = await import('./AccountsOverview');

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
  mocks.listeners.clear();
  mocks.initial = [];
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const account = (id: number): AccountSummary =>
  ({
    id,
    email: `user${id}@example.invalid`,
    display_name: null,
    imap_host: 'imap.example.invalid',
    smtp_host: 'smtp.example.invalid',
    sync_window: 'all',
    full_window: 'all',
    body_window: 'off',
    signature_id: null,
    unread_count: 0,
    total_count: 0,
    server_total_count: 0,
  }) as AccountSummary;

const status = (account_id: number, over: Partial<AccountSyncStatus>): AccountSyncStatus => ({
  account_id,
  phase: 'ok',
  error_kind: null,
  error: null,
  last_success_at: '2026-11-12T01:00:00Z',
  last_attempt_at: null,
  next_retry_at: null,
  ...over,
});

/** 行ごとの印の状態（アドレスの順）。 */
const phases = () =>
  [...container.querySelectorAll('button')]
    .map((b) => b.querySelector('[data-sync-phase]')?.getAttribute('data-sync-phase'))
    .filter((p): p is string => p != null);

const render = async (accounts: AccountSummary[]) => {
  await act(async () => {
    root.render(<AccountsOverview accounts={accounts} onOpenMail={() => undefined} />);
  });
};

describe('ホームのアカウント一覧の接続状況の印', () => {
  it('アカウントごとに印が出る（まだ同期していないものは never）', async () => {
    mocks.initial = [
      status(1, { phase: 'ok' }),
      status(2, { phase: 'failed', error_kind: 'auth' }),
    ];
    await render([account(1), account(2), account(3)]);
    expect(phases()).toEqual(['ok', 'failed', 'never']);
    // 印はアドレスの左（行の中でアドレスより前）。
    const row = container.querySelector('button')!;
    const dot = row.querySelector('[data-sync-phase]')!;
    const addr = row.querySelector('.truncate')!;
    expect(addr.textContent).toBe('user1@example.invalid');
    expect(dot.compareDocumentPosition(addr) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // ツールチップは共通の文言（失敗は種類と最後の成功）。
    const failed = container.querySelector('[data-sync-phase="failed"]')!;
    expect(failed.getAttribute('title')).toContain('mailbox.syncStatus.auth');
  });

  it('状態の知らせで印が変わる', async () => {
    await render([account(1), account(2)]);
    expect(phases()).toEqual(['never', 'never']);
    const emit = mocks.listeners.get('sync:status')!;
    act(() => emit({ payload: status(1, { phase: 'syncing' }) }));
    expect(phases()).toEqual(['syncing', 'never']);
    act(() => emit({ payload: status(1, { phase: 'failed', error_kind: 'unreachable' }) }));
    act(() => emit({ payload: status(2, { phase: 'ok' }) }));
    expect(phases()).toEqual(['failed', 'ok']);
  });

  it('印を押すと行と同じく展開し、展開しても印は残る', async () => {
    mocks.initial = [status(2, { phase: 'ok' })];
    await render([account(1), account(2)]);
    const dot = container.querySelectorAll<HTMLElement>('[data-sync-phase]')[1];
    await act(async () => {
      dot.click();
    });
    // 展開中はそのアカウントだけを出す。
    expect(container.textContent).not.toContain('user1@example.invalid');
    expect(container.textContent).toContain('user2@example.invalid');
    expect(phases()).toEqual(['ok']);
  });
});
