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
import { greenDomainList } from '../services/green';
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

/** 氏名の突き合わせ用の正規化（全半角統一・空白除去・小文字化）。 */
const foldName = (s?: string | null) =>
  (s ?? '').normalize('NFKC').replace(/\s+/g, '').toLowerCase();

/**
 * 同じアドレスの連絡先のうち、差出人名に「関連する」もの（完全一致・空白差のほか、姓のみ/名のみ等の
 * 部分一致）。同じ代表アドレスが別人（前任）に紐づくときの取り違えを防ぐ。差出人名が無ければ null。
 */
export const contactNamed = (
  matches: ContactSummary[],
  name: string | null | undefined
): ContactSummary | null => {
  const want = foldName(name);
  if (!want) return null;
  return (
    matches.find((m) => {
      const dn = foldName(m.display_name);
      return dn === want || dn.includes(want) || want.includes(dn);
    }) ?? null
  );
};

/**
 * 差出人を「住所録の本人」としてグリーンにしている連絡先（削除されていないもの）。無ければ null。
 * 何人もいれば、差出人名に関連する人を優先する（住所録で開く先）。
 */
export const findKeptContact = async (
  address: string,
  name: string | null | undefined
): Promise<ContactSummary | null> => {
  const matches = await contactLookupEmail(address);
  return contactNamed(matches, name) ?? matches[0] ?? null;
};

/** ドメインがグリーン（手動認定 or 住所録由来。警告・フリーメールは除く）か。 */
export const isDomainGreen = async (domain: string): Promise<boolean> => {
  const d = domain.trim().toLowerCase();
  return (await greenDomainList()).some((e) => e.kind === 'green' && e.domain === d);
};
