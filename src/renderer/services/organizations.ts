import { invoke } from '@tauri-apps/api/core';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import type { OrgDuplicateGroup } from '@bindings/OrgDuplicateGroup';

/** 組織一覧（所属件数つき）。query があれば名前で部分一致。組織コンボボックスの候補に使う。 */
export const organizationList = (query?: string) =>
  invoke<OrganizationSummary[]>('organization_list', { query: query ?? null });

/** 組織名の重複候補（正規化名で束ねたグループ）を取得。 */
export const organizationFindDuplicates = () =>
  invoke<OrgDuplicateGroup[]>('organization_find_duplicates');

/** 複数の組織を 1 件（keepId）に統一（name が統一名）。 */
export const organizationMerge = (keepId: number, dropIds: number[], name: string) =>
  invoke<OrganizationSummary>('organization_merge', { keepId, dropIds, name });
