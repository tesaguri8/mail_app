import { invoke } from '@tauri-apps/api/core';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';

/** 組織一覧（所属件数つき）。query があれば名前で部分一致。組織コンボボックスの候補に使う。 */
export const organizationList = (query?: string) =>
  invoke<OrganizationSummary[]>('organization_list', { query: query ?? null });
