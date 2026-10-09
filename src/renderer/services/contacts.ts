import { invoke } from '@tauri-apps/api/core';
import type { ContactSummary } from '@bindings/ContactSummary';
import type { ContactListItem } from '@bindings/ContactListItem';
import type { ContactInput } from '@bindings/ContactInput';
import type { ImportReport } from '@bindings/ImportReport';
import type { DuplicateGroup } from '@bindings/DuplicateGroup';
import type { ContactMatch } from '@bindings/ContactMatch';

// Tauri v2 は camelCase の引数キーを snake_case の Rust 引数へ自動変換する。
/** 連絡先一覧（一覧に出す分だけの軽い形）。開いたら contactGet で全項目を取る。 */
export const contactList = (query?: string, groups?: number[], includeDeleted = false) =>
  invoke<ContactListItem[]>('contact_list', {
    query: query ?? null,
    groups: groups && groups.length > 0 ? groups : null,
    includeDeleted,
  });

export const contactGet = (id: number) => invoke<ContactSummary>('contact_get', { id });

/** 指定メールアドレスを持つ連絡先（非削除）を返す。メールの ＋/編集 切替・重複数表示に使う。 */
export const contactLookupEmail = (email: string) =>
  invoke<ContactSummary[]>('contact_lookup_email', { email });

export const contactUpsert = (input: ContactInput) =>
  invoke<ContactSummary>('contact_upsert', { input });

/** 連絡先を論理削除（ゴミ箱へ。保持期間後に完全削除）。 */
export const contactDelete = (id: number) => invoke<void>('contact_delete', { id });

/** 論理削除した連絡先を復元。 */
export const contactRestore = (id: number) => invoke<void>('contact_restore', { id });

/** 連絡先ファイルをインポート（.vcf = vCard / .csv = Google CSV）。 */
export const contactImport = (path: string) => invoke<ImportReport>('contact_import', { path });

/** 重複候補（正規化表示名でグループ化）を取得。 */
export const contactFindDuplicates = () => invoke<DuplicateGroup[]>('contact_find_duplicates');

/** 入力（メール/電話/FAX/氏名）に一致する既存連絡先を返す（共有指定の値は除外）。
 *  新規登録前チェック・編集中の赤字警告・メールからの＋追加で使う。 */
export const contactFindMatches = (
  emails: string[],
  phones: string[],
  displayName: string | null,
  excludeId: number | null,
) =>
  invoke<ContactMatch[]>('contact_find_matches', {
    emails,
    phones,
    displayName,
    excludeId,
  });

/** 複数連絡先を 1 件（keepId）に統合。 */
export const contactMerge = (keepId: number, dropIds: number[]) =>
  invoke<ContactSummary>('contact_merge', { keepId, dropIds });

/** 連絡先の同期先に Google アカウントを加える（作成待ちを置き、次の同期で作る）。 */
export const contactSyncTargetAdd = (contactId: number, accountId: number) =>
  invoke<void>('contact_sync_target_add', { contactId, accountId });

/** 連絡先とそのアカウントの同期をやめる。deleteRemote なら次の同期で向こうの連絡先も削除する
 *  （偽ならつながりだけ外し、向こうは残る）。Rondine の連絡先はどちらでも残る。 */
export const contactSyncStop = (contactId: number, accountId: number, deleteRemote: boolean) =>
  invoke<void>('contact_sync_stop', { contactId, accountId, deleteRemote });
