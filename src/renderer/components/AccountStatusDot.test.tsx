// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { TFunction } from 'i18next';
import type { AccountSyncStatus } from '@bindings/AccountSyncStatus';
import {
  AccountStatusDot,
  accountStatusLabel,
  formatSyncTime,
  worstStatus,
} from './AccountStatusDot';
import { Dropdown } from './Dropdown';

/**
 * アカウントの接続状況の印（サイドバーのアカウント選択と設定のカードで共有）。
 * 正常は緑・同期中は水色・失敗は赤・まだ同期していないは灰。失敗のときは、種類と最後に成功した時刻を出す。
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

const status = (over: Partial<AccountSyncStatus>): AccountSyncStatus => ({
  account_id: 1,
  phase: 'ok',
  error_kind: null,
  error: null,
  last_success_at: null,
  last_attempt_at: null,
  ...over,
});

/** 試験用の t: キーと差し込みの値を、そのまま見える形にする。 */
const t = ((key: string, opts?: Record<string, unknown>) =>
  opts && 'time' in opts ? `${key}[${String(opts.time)}]` : key) as unknown as TFunction;

const NOW = new Date(2026, 9, 28, 9, 0);
const AT_0223 = new Date(2026, 9, 28, 2, 23).toISOString();

describe('接続状況の文言', () => {
  it('状態が無ければ「まだ同期していない」、同期中・正常はそれぞれの文言', () => {
    expect(accountStatusLabel(t, undefined)).toBe('mailbox.syncStatus.never');
    expect(accountStatusLabel(t, status({ phase: 'syncing' }))).toBe('mailbox.syncStatus.syncing');
    // 軽い到達確認だけ（まだ同期・ログインしていない）
    expect(accountStatusLabel(t, status({ phase: 'ok' }))).toBe('mailbox.syncStatus.reachable');
    expect(accountStatusLabel(t, status({ phase: 'ok', last_success_at: AT_0223 }), NOW)).toBe(
      `mailbox.syncStatus.ok[${formatSyncTime(AT_0223, NOW)}]`
    );
  });

  it('失敗は種類と、最後に成功した時刻（無ければ、この起動ではまだ成功していない）', () => {
    const unreachable = status({
      phase: 'failed',
      error_kind: 'unreachable',
      last_success_at: AT_0223,
    });
    expect(accountStatusLabel(t, unreachable, NOW)).toBe(
      `mailbox.syncStatus.unreachable` +
        `mailbox.syncStatus.lastSuccess[${formatSyncTime(AT_0223, NOW)}]`
    );
    const auth = status({ phase: 'failed', error_kind: 'auth' });
    expect(accountStatusLabel(t, auth)).toBe('mailbox.syncStatus.authmailbox.syncStatus.noSuccess');
  });

  it('今日の時刻は時刻だけ、ほかの日は月日つき', () => {
    expect(formatSyncTime(AT_0223, NOW)).not.toMatch(/10\D+28|28\D+10/);
    const yesterday = new Date(2026, 9, 27, 23, 5).toISOString();
    expect(formatSyncTime(yesterday, NOW)).toMatch(/27/);
  });
});

describe('接続状況の印', () => {
  it('状態ごとに色を変え、ツールチップに文言を出す', () => {
    const cases: [AccountSyncStatus | undefined, string, string][] = [
      [status({ phase: 'ok' }), 'ok', 'bg-emerald-400'],
      [status({ phase: 'syncing' }), 'syncing', 'bg-sky-400'],
      [status({ phase: 'failed', error_kind: 'unreachable' }), 'failed', 'bg-rose-500'],
      [undefined, 'never', 'bg-white/30'],
    ];
    for (const [s, phase, color] of cases) {
      act(() => root.render(<AccountStatusDot status={s} />));
      const dot = container.querySelector('[data-sync-phase]');
      expect(dot?.getAttribute('data-sync-phase')).toBe(phase);
      expect(dot?.className).toContain(color);
      expect(dot?.getAttribute('title')).toBeTruthy();
    }
  });

  it('いくつかのアカウントをまとめるときは、いちばん悪い状態を出す', () => {
    const ok = status({ account_id: 1, phase: 'ok' });
    const failed = status({ account_id: 2, phase: 'failed', error_kind: 'auth' });
    const syncing = status({ account_id: 3, phase: 'syncing' });
    expect(worstStatus([ok, failed, syncing])).toBe(failed);
    expect(worstStatus([ok, syncing])).toBe(syncing);
    expect(worstStatus([ok, undefined])?.phase ?? 'never').toBe('never');
    expect(worstStatus([ok])).toBe(ok);
  });

  it('ドロップダウンの候補の行に印と補足の行を出す', () => {
    const failed = status({ phase: 'failed', error_kind: 'unreachable' });
    act(() =>
      root.render(
        <Dropdown
          value="1"
          onChange={() => undefined}
          options={[
            {
              value: '1',
              label: 'a@example.invalid',
              icon: <AccountStatusDot status={failed} />,
              title: 'サーバーにつながりません',
              detail: 'サーバーにつながりません（最後の成功 2:23）',
            },
            {
              value: '2',
              label: 'b@example.invalid',
              icon: <AccountStatusDot status={undefined} />,
            },
          ]}
        />
      )
    );
    // 閉じているときのボタンにも、選んでいるアカウントの印を出す
    expect(container.querySelector('button [data-sync-phase="failed"]')).not.toBeNull();
    act(() => (container.querySelector('button') as HTMLButtonElement).click());
    const rows = Array.from(document.querySelectorAll('div.fixed button'));
    expect(rows).toHaveLength(2);
    expect(rows[0].getAttribute('title')).toBe('サーバーにつながりません');
    expect(rows[0].textContent).toContain('（最後の成功 2:23）');
    expect(rows[1].querySelector('[data-sync-phase="never"]')).not.toBeNull();
  });
});
