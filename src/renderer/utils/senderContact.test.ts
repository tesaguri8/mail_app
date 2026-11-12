import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ContactSummary } from '@bindings/ContactSummary';
import type { GoogleAccount } from '@bindings/GoogleAccount';

// 住所録の書き込みは既存のサービス（contact_upsert ＋ 同期先の追加）を通ることを確かめる。
const upsert = vi.fn();
const targetAdd = vi.fn();
const lookup = vi.fn();
vi.mock('../services/contacts', () => ({
  contactUpsert: (...a: unknown[]) => upsert(...a),
  contactSyncTargetAdd: (...a: unknown[]) => targetAdd(...a),
  contactLookupEmail: (...a: unknown[]) => lookup(...a),
  contactRestore: vi.fn(),
}));
vi.mock('../services/google', () => ({ googleAccounts: vi.fn(async () => []) }));
vi.mock('../config/prefs', () => ({
  getNewContactTarget: () => true,
  setNewContactTarget: vi.fn(),
}));

import {
  classifySender,
  createSenderContact,
  findSenderInBook,
  senderDisplayName,
} from './senderContact';

const contact = (id: number, deleted: string | null) =>
  ({ id, display_name: `人${id}`, deleted_at: deleted }) as unknown as ContactSummary;
const account = (id: number, sync: boolean, disconnected: string | null = null) =>
  ({ id, sync_contacts: sync, disconnected_at: disconnected }) as unknown as GoogleAccount;

beforeEach(() => {
  upsert.mockReset();
  targetAdd.mockReset();
  lookup.mockReset();
});

describe('classifySender', () => {
  it('無ければ none、削除されていないものを優先、ゴミ箱だけなら trashed', () => {
    expect(classifySender([])).toEqual({ kind: 'none' });
    const t = contact(1, '2026-11-01');
    const a = contact(2, null);
    expect(classifySender([t, a])).toEqual({ kind: 'alive', contact: a });
    expect(classifySender([t])).toEqual({ kind: 'trashed', contact: t });
  });

  it('照合はゴミ箱を含めて引く', async () => {
    lookup.mockResolvedValue([]);
    await findSenderInBook('taro@gmail.invalid');
    expect(lookup).toHaveBeenCalledWith('taro@gmail.invalid', true);
  });
});

describe('senderDisplayName', () => {
  it('差出人名が無ければアドレス', () => {
    expect(senderDisplayName('  山田 太郎 ', 'taro@gmail.invalid')).toBe('山田 太郎');
    expect(senderDisplayName(null, ' taro@gmail.invalid ')).toBe('taro@gmail.invalid');
    expect(senderDisplayName('  ', 'taro@gmail.invalid')).toBe('taro@gmail.invalid');
  });
});

describe('createSenderContact', () => {
  it('表示名とアドレスで作り、新規作成の既定の同期先（連携中で連絡先を同期するもの）を加える', async () => {
    upsert.mockResolvedValue(contact(7, null));
    const c = await createSenderContact('山田 太郎', ' taro@gmail.invalid ', async () => [
      account(1, true),
      account(2, false),
      account(3, true, '2026-10-01'),
    ]);
    expect(c.id).toBe(7);
    const input = upsert.mock.calls[0][0];
    expect(input.id).toBeNull();
    expect(input.display_name).toBe('山田 太郎');
    expect(input.family_name).toBe('山田');
    expect(input.given_name).toBe('太郎');
    expect(input.emails).toEqual([{ label: null, value: 'taro@gmail.invalid', is_shared: false }]);
    expect(targetAdd.mock.calls).toEqual([[7, 1]]);
  });

  it('Google を引けなくても連絡先は作る（同期先は加えない）', async () => {
    upsert.mockResolvedValue(contact(8, null));
    await createSenderContact(null, 'x@gmail.invalid', async () => {
      throw new Error('offline');
    });
    expect(upsert.mock.calls[0][0].display_name).toBe('x@gmail.invalid');
    expect(targetAdd).not.toHaveBeenCalled();
  });
});
