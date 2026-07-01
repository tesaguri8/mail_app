import { invoke } from '@tauri-apps/api/core';
import type { ContactSummary } from '@bindings/ContactSummary';
import type { ContactInput } from '@bindings/ContactInput';
import type { ContactGroupSummary } from '@bindings/ContactGroupSummary';
import type { ImportReport } from '@bindings/ImportReport';
import type { DuplicateGroup } from '@bindings/DuplicateGroup';

// Tauri v2 は camelCase の引数キーを snake_case の Rust 引数へ自動変換する。
export const contactList = (query?: string) =>
  invoke<ContactSummary[]>('contact_list', { query: query ?? null });

export const contactGet = (id: number) => invoke<ContactSummary>('contact_get', { id });

export const contactUpsert = (input: ContactInput) =>
  invoke<ContactSummary>('contact_upsert', { input });

export const contactDelete = (id: number) => invoke<void>('contact_delete', { id });

export const contactGroupList = () => invoke<ContactGroupSummary[]>('contact_group_list');

/** 連絡先ファイルをインポート（.vcf = vCard / .csv = Google CSV）。 */
export const contactImport = (path: string) => invoke<ImportReport>('contact_import', { path });

/** 重複候補（正規化表示名でグループ化）を取得。 */
export const contactFindDuplicates = () => invoke<DuplicateGroup[]>('contact_find_duplicates');

/** 複数連絡先を 1 件（keepId）に統合。 */
export const contactMerge = (keepId: number, dropIds: number[]) =>
  invoke<ContactSummary>('contact_merge', { keepId, dropIds });
