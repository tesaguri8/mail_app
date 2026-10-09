import { describe, expect, it } from 'vitest';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import { defaultTargets, selectableAccount } from './ContactSyncTargets';

const account = (id: number, over: Partial<GoogleAccount> = {}): GoogleAccount => ({
  id,
  email: `a${id}@gmail.com`,
  sync_calendar: true,
  sync_contacts: true,
  last_calendar_sync_at: null,
  last_contacts_sync_at: null,
  disconnected_at: null,
  calendar_granted: true,
  contacts_granted: true,
  ...over,
});

describe('同期先に選べるアカウント', () => {
  it('連絡先を同期していて解除中でないものだけ', () => {
    expect(selectableAccount(account(1))).toBe(true);
    expect(selectableAccount(account(2, { sync_contacts: false }))).toBe(false);
    expect(selectableAccount(account(3, { disconnected_at: '2026-10-09 10:00:00' }))).toBe(false);
  });

  it('新規の既定は選べるアカウントすべて（前回外したもの・解除中・連絡先を同期していないものは除く）', () => {
    const accounts = [
      account(1),
      account(2),
      account(3, { disconnected_at: '2026-10-09 10:00:00' }),
      account(4, { sync_contacts: false }),
    ];
    // 未記録はオン。
    expect([...defaultTargets(accounts, () => true)]).toEqual([1, 2]);
    // 前回 2 を外した。
    expect([...defaultTargets(accounts, (id) => id !== 2)]).toEqual([1]);
  });
});
