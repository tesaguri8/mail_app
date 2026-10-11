import { invoke } from '@tauri-apps/api/core';
import { changed } from '../utils/localChange';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import type { CompanyOnlyCandidate } from '@bindings/CompanyOnlyCandidate';
import type { OrganizationDetail } from '@bindings/OrganizationDetail';
import type { OrgDuplicateGroup } from '@bindings/OrgDuplicateGroup';
import type { OrgChangeImpact } from '@bindings/OrgChangeImpact';
import type { OrgLinkSuggestion } from '@bindings/OrgLinkSuggestion';
import type { OrgMatch } from '@bindings/OrgMatch';
import type { UnlinkedOrgName } from '@bindings/UnlinkedOrgName';

// 組織は連絡先の 1 件（docs/ORG_CONTACTS.md）。作る・直す・消す・戻すは連絡先と同じ
// （services/contacts.ts）。ここは組織タブ（見る・つなぐ）と整理のための読み出しだけを持つ。

/** 組織一覧（所属件数つき）。query があれば名前で部分一致。組織コンボボックスの候補に使う。
 *  includeDeleted=true でゴミ箱の組織も含める。 */
export const organizationList = (query?: string, includeDeleted = false) =>
  invoke<OrganizationSummary[]>('organization_list', { query: query ?? null, includeDeleted });

/** 単一の組織を取得（代表の値・所属人数）。人の連絡先で組織の情報を出すのに使う。 */
export const organizationGet = (id: number) =>
  invoke<OrganizationSummary>('organization_get', { id });

/** 組織の詳細（所属連絡先＋共有アドレスを件数つきで）。 */
export const organizationDetail = (id: number) =>
  invoke<OrganizationDetail>('organization_detail', { id });

/** 組織名の重複候補（正規化名で束ねたグループ）を取得。 */
export const organizationFindDuplicates = () =>
  invoke<OrgDuplicateGroup[]>('organization_find_duplicates');

/** 複数の組織を 1 件（keepId）に統一（name が統一名）。 */
export const organizationMerge = (keepId: number, dropIds: number[], name: string) =>
  changed(invoke<OrganizationSummary>('organization_merge', { keepId, dropIds, name }));

// ── 整理（docs/CONTACT_MODEL.md §1-5-1）。どれも候補を出すだけで、作る・つなぐは人が選ぶ。

/** 組織カードになっていない会社名（正規化名でまとめ、人数の多い順）。 */
export const organizationUnlinkedNames = () =>
  invoke<UnlinkedOrgName[]>('organization_unlinked_names');

/** 会社名から組織カードを作り、同じ会社名（正規化後）の人を全員つなぐ。 */
export const organizationCreateFromName = (name: string) =>
  changed(invoke<OrganizationSummary>('organization_create_from_name', { name }));

/** 組織カードごとの「つながっていないが同じ組織らしい人」（理由つき）。 */
export const organizationLinkSuggestions = () =>
  invoke<OrgLinkSuggestion[]>('organization_link_suggestions');

/** 会社名・メール・電話から、似ている既存の組織カード（理由つき・多くて 8 件）。 */
export const organizationMatches = (
  name: string,
  emails: string[],
  phones: string[],
  excludeContact: number | null
) => invoke<OrgMatch[]>('organization_matches', { name, emails, phones, excludeContact });

/** 選んだ人を組織カードにつなぐ。 */
export const organizationLinkContacts = (orgId: number, contactIds: number[]) =>
  changed(invoke<OrganizationSummary>('organization_link_contacts', { orgId, contactIds }));

/** 「組織カードにする」の下見（次の同期で送り直しになる人数）。何も書き換えない。 */
export const organizationCreateFromNameImpact = (name: string) =>
  invoke<OrgChangeImpact>('organization_create_from_name_impact', { name });

/** 「つなぐ」の下見（次の同期で送り直しになる人数）。何も書き換えない。 */
export const organizationLinkContactsImpact = (orgId: number, contactIds: number[]) =>
  invoke<OrgChangeImpact>('organization_link_contacts_impact', { orgId, contactIds });

/** 組織の連絡先へ切り替える候補（会社名だけの連絡先）。docs/ORG_CONTACTS.md §4-1。 */
export const organizationCompanyOnlyCandidates = () =>
  invoke<CompanyOnlyCandidate[]>('organization_company_only_candidates');

/** 会社名だけの連絡先を、その場で組織の連絡先に切り替える（同じ会社名の人もつなぐ）。切り替えた数。 */
export const organizationConvertContacts = (contactIds: number[]) =>
  changed(invoke<number>('organization_convert_contacts', { contactIds }));
