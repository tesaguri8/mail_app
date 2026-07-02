import { invoke } from '@tauri-apps/api/core';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import type { OrganizationDetail } from '@bindings/OrganizationDetail';
import type { OrgDuplicateGroup } from '@bindings/OrgDuplicateGroup';

/** 組織一覧（所属件数つき）。query があれば名前で部分一致。組織コンボボックスの候補に使う。 */
export const organizationList = (query?: string) =>
  invoke<OrganizationSummary[]>('organization_list', { query: query ?? null });

/** 組織の詳細（所属連絡先＋共有アドレスを件数つきで）。 */
export const organizationDetail = (id: number) =>
  invoke<OrganizationDetail>('organization_detail', { id });

/** 組織を作成/編集（名前・メモ）。id 指定で更新、無ければ新規。 */
export const organizationUpsert = (
  id: number | null,
  name: string,
  nameKana: string | null,
  note: string | null,
) => invoke<OrganizationSummary>('organization_upsert', { id, name, nameKana, note });

/** 組織を削除（所属している連絡先があるときはバックエンド側で拒否される）。 */
export const organizationDelete = (id: number) => invoke<void>('organization_delete', { id });

/** 組織名の重複候補（正規化名で束ねたグループ）を取得。 */
export const organizationFindDuplicates = () =>
  invoke<OrgDuplicateGroup[]>('organization_find_duplicates');

/** 複数の組織を 1 件（keepId）に統一（name が統一名）。 */
export const organizationMerge = (keepId: number, dropIds: number[], name: string) =>
  invoke<OrganizationSummary>('organization_merge', { keepId, dropIds, name });
