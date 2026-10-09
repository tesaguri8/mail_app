import { invoke } from '@tauri-apps/api/core';
import type { MergeRemoteDeletion } from '@bindings/MergeRemoteDeletion';
import type { SureMergePreview } from '@bindings/SureMergePreview';
import type { SureMergeResult } from '@bindings/SureMergeResult';
import type { ContactSummary } from '@bindings/ContactSummary';
import type { ContactListItem } from '@bindings/ContactListItem';
import type { ContactInput } from '@bindings/ContactInput';
import type { ImportReport } from '@bindings/ImportReport';
import type { ContactExportReport } from '@bindings/ContactExportReport';
import type { VcardVersion } from '@bindings/VcardVersion';
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

/** 連絡先を vCard ファイルに書き出す（ids が null ならゴミ箱を除く全員）。 */
export const contactExport = (path: string, ids: number[] | null, version: VcardVersion) =>
  invoke<ContactExportReport>('contact_export', { path, ids, version });

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

/** 統合したら Google 側から消すことになる件数（アカウントごと）。読むだけ（確認画面用）。 */
export const contactMergePreview = (keepId: number, dropIds: number[]) =>
  invoke<MergeRemoteDeletion[]>('contact_merge_preview', { keepId, dropIds });

/** 以前の統合で残った Google の重複（1 人に同じアカウントの ID が 2 つ以上）の件数。読むだけ。 */
export const contactGoogleDuplicates = () =>
  invoke<MergeRemoteDeletion[]>('contact_google_duplicates');

/** 以前の統合で残った Google の重複を、統合と同じ規則で片付ける（次の同期で Google 側から削除）。 */
export const contactGoogleDuplicatesTidy = () => invoke<number>('contact_google_duplicates_tidy');

/** 確実な重複（名前・メールの集合・電話の集合が同じで、食い違う欄が無い組）をまとめて統合したら
 *  どうなるか。読むだけ（組の一覧・件数・Google から消す件数）。 */
export const contactSureMergePreview = () =>
  invoke<SureMergePreview>('contact_sure_merge_preview');

/** 確実な重複をまとめて統合する（実行のときに組を数え直す。Google の余りは次の同期で削除）。 */
export const contactSureMerge = () => invoke<SureMergeResult>('contact_sure_merge');

/** 複数連絡先を 1 件（keepId）に統合。同じ Google アカウントの ID は 1 つ残し、余りは次の同期で削除。 */
export const contactMerge = (keepId: number, dropIds: number[]) =>
  invoke<ContactSummary>('contact_merge', { keepId, dropIds });

/** 連絡先の同期先に Google アカウントを加える（作成待ちを置き、次の同期で作る）。 */
export const contactSyncTargetAdd = (contactId: number, accountId: number) =>
  invoke<void>('contact_sync_target_add', { contactId, accountId });

/** 連絡先とそのアカウントの同期をやめる。deleteRemote なら次の同期で向こうの連絡先も削除する
 *  （偽ならつながりだけ外し、向こうは残る）。Rondine の連絡先はどちらでも残る。 */
export const contactSyncStop = (contactId: number, accountId: number, deleteRemote: boolean) =>
  invoke<void>('contact_sync_stop', { contactId, accountId, deleteRemote });
