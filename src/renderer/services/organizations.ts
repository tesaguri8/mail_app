import { invoke } from '@tauri-apps/api/core';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import type { OrganizationInput } from '@bindings/OrganizationInput';
import type { OrganizationDetail } from '@bindings/OrganizationDetail';
import type { OrgDuplicateGroup } from '@bindings/OrgDuplicateGroup';

/** 組織一覧（所属件数つき）。query があれば名前で部分一致。組織コンボボックスの候補に使う。
 *  includeDeleted=true で論理削除済み（ゴミ箱）も含める。 */
export const organizationList = (query?: string, includeDeleted = false) =>
  invoke<OrganizationSummary[]>('organization_list', { query: query ?? null, includeDeleted });

/** 論理削除した組織を復元。 */
export const organizationRestore = (id: number) =>
  invoke<void>('organization_restore', { id });

/** 単一の組織（組織カード）を取得。連絡先のラベル表示・カード編集ダイアログ用。 */
export const organizationGet = (id: number) =>
  invoke<OrganizationSummary>('organization_get', { id });

/** 組織の詳細（所属連絡先＋共有アドレスを件数つきで）。 */
export const organizationDetail = (id: number) =>
  invoke<OrganizationDetail>('organization_detail', { id });

/** 組織カード（名前・よみ・メモ・代表電話/FAX/代表メール/URL・所在地）を作成/編集。
 *  input.id 指定で更新、無ければ新規。 */
export const organizationUpsert = (input: OrganizationInput) =>
  invoke<OrganizationSummary>('organization_upsert', { input });

/** 組織を削除（所属している連絡先があるときはバックエンド側で拒否される）。 */
export const organizationDelete = (id: number) => invoke<void>('organization_delete', { id });

/** 組織名の重複候補（正規化名で束ねたグループ）を取得。 */
export const organizationFindDuplicates = () =>
  invoke<OrgDuplicateGroup[]>('organization_find_duplicates');

/** 複数の組織を 1 件（keepId）に統一（name が統一名）。 */
export const organizationMerge = (keepId: number, dropIds: number[], name: string) =>
  invoke<OrganizationSummary>('organization_merge', { keepId, dropIds, name });
