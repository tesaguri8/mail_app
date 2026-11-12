// 差出人を住所録に登録して「この人だけグリーン」にする（フリーメールの葉っぱ。docs/GREEN_DOMAINS.md）。
//
// 書き込みは住所録の普段の作成と同じ経路（contact_upsert ＋ 同期先の追加）に乗せる。
// 新しい書き込み口は作らない。Google の同期先は、編集画面で新規作成するときの既定
// （defaultTargets）と同じものを選ぶので、普段どおり次の同期で Google にも作られる。

import type { ContactSummary } from '@bindings/ContactSummary';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import {
  contactLookupEmail,
  contactRestore,
  contactSyncTargetAdd,
  contactUpsert,
} from '../services/contacts';
import { googleAccounts } from '../services/google';
import { draftFromPrefill } from '../components/ContactEditor';
import { defaultTargets } from '../components/ContactSyncTargets';

/** 同じアドレスが住所録にあるか。 */
export type SenderInBook =
  /** 無い（新しく作る）。 */
  | { kind: 'none' }
  /** 削除されていない連絡先がある（作らない）。 */
  | { kind: 'alive'; contact: ContactSummary }
  /** ゴミ箱にだけある（作らない。戻せばグリーンになる）。 */
  | { kind: 'trashed'; contact: ContactSummary };

/** 照合の結果（ゴミ箱を含む）を振り分ける。削除されていないものを優先する。 */
export const classifySender = (matches: ContactSummary[]): SenderInBook => {
  const alive = matches.find((c) => c.deleted_at == null);
  if (alive) return { kind: 'alive', contact: alive };
  const trashed = matches[0];
  return trashed ? { kind: 'trashed', contact: trashed } : { kind: 'none' };
};

/** 同じアドレスが住所録（ゴミ箱を含む）にあるかを確かめる。 */
export const findSenderInBook = async (address: string): Promise<SenderInBook> =>
  classifySender(await contactLookupEmail(address, true));

/** 連絡先の表示名（差出人名が無ければアドレス。表示名は必須のため）。 */
export const senderDisplayName = (name: string | null | undefined, address: string) =>
  name?.trim() || address.trim();

/**
 * 差出人の表示名とアドレスで連絡先を作り、新規作成の既定の同期先（Google）を加える。
 * `accounts` は試験用の差し替え口（既定は連携中の Google アカウント）。
 */
export const createSenderContact = async (
  name: string | null | undefined,
  address: string,
  accounts: () => Promise<GoogleAccount[]> = googleAccounts
): Promise<ContactSummary> => {
  const created = await contactUpsert(
    draftFromPrefill({ name: senderDisplayName(name, address), email: address.trim() })
  );
  const list = await accounts().catch(() => [] as GoogleAccount[]);
  for (const accountId of defaultTargets(list)) {
    await contactSyncTargetAdd(created.id, accountId);
  }
  return created;
};

/** ゴミ箱の連絡先を戻す（普段の「復元」と同じ）。 */
export const restoreSenderContact = (contact: ContactSummary) => contactRestore(contact.id);
