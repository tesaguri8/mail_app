import { describe, expect, it } from 'vitest';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import { defaultTargets, selectableAccount } from './ContactSyncTargets';

const account = (id: number, over: Partial<GoogleAccount> = {}): GoogleAccount => ({
  id,
  email: `a${id}@gmail.com`,
  sync_calendar: true,
  sync_contacts: true,
  push_new_contacts: false,
  last_calendar_sync_at: null,
  last_contacts_sync_at: null,
  disconnected_at: null,
  ...over,
});

describe('同期先に選べるアカウント', () => {
  it('連絡先を同期していて解除中でないものだけ', () => {
    expect(selectableAccount(account(1))).toBe(true);
    expect(selectableAccount(account(2, { sync_contacts: false }))).toBe(false);
    expect(selectableAccount(account(3, { disconnected_at: '2026-10-09 10:00:00' }))).toBe(false);
  });

  it('新規の既定は「既定で Google にも保存」のアカウントだけ（解除中は除く）', () => {
    const accounts = [
      account(1, { push_new_contacts: true }),
      account(2),
      account(3, { push_new_contacts: true, disconnected_at: '2026-10-09 10:00:00' }),
    ];
    expect([...defaultTargets(accounts)]).toEqual([1]);
  });
});
