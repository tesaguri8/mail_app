// 連絡先の編集用の下書き（ContactInput）を組み立てる小道具。
//
// 連絡先の中身は Rust 側の ContactFields と同じ形で、一覧/詳細（ContactSummary）と
// 入力（ContactInput）が共有している。すべての項目を下書きに写して送るので、画面に
// 出していない項目（畳んでいる項目・送らない属性）も保存で消えない。

import type { ContactInput } from '@bindings/ContactInput';
import type { ContactOrganization } from '@bindings/ContactOrganization';
import type { ContactSummary } from '@bindings/ContactSummary';

/** 空の下書き（新規作成用）。 */
export const emptyContactInput = (): ContactInput => ({
  id: null,
  display_name: '',
  name_prefix: null,
  family_name: null,
  middle_name: null,
  given_name: null,
  name_suffix: null,
  phonetic_family: null,
  phonetic_middle: null,
  phonetic_given: null,
  nickname: null,
  maiden_name: null,
  birthday: null,
  note: null,
  kind: 'person',
  is_favorite: false,
  is_business: false,
  allow_remote_images: false,
  organizations: [],
  emails: [],
  phones: [],
  addresses: [],
  urls: [],
  dates: [],
  relations: [],
  handles: [],
  custom_fields: [],
  tags: [],
});

/** 読み込んだ連絡先を下書きにする（すべての項目を写す＝保存で値を落とさない）。 */
export const contactToInput = (c: ContactSummary): ContactInput => ({
  id: c.id,
  display_name: c.display_name,
  name_prefix: c.name_prefix,
  family_name: c.family_name,
  middle_name: c.middle_name,
  given_name: c.given_name,
  name_suffix: c.name_suffix,
  phonetic_family: c.phonetic_family,
  phonetic_middle: c.phonetic_middle,
  phonetic_given: c.phonetic_given,
  nickname: c.nickname,
  maiden_name: c.maiden_name,
  birthday: c.birthday,
  note: c.note,
  kind: c.kind,
  is_favorite: c.is_favorite,
  is_business: c.is_business,
  allow_remote_images: c.allow_remote_images,
  organizations: c.organizations,
  emails: c.emails,
  phones: c.phones,
  addresses: c.addresses,
  urls: c.urls,
  dates: c.dates,
  relations: c.relations,
  handles: c.handles,
  custom_fields: c.custom_fields,
  tags: c.tags,
});

/** 空の組織の下書き（新規作成用）。名前は自分の会社の行（先頭の会社）に持つ。 */
export const emptyOrganizationInput = (): ContactInput => ({
  ...emptyContactInput(),
  kind: 'organization',
  organizations: [emptyOrganization()],
});

/**
 * 人／組織を切り替えた下書き（docs/ORG_CONTACTS.md §4-2）。組織にするときは、先頭の会社
 * （自分の会社の行）の名前が空なら表示名を入れる。人に戻すときは何も消さない。
 */
export const withKind = (d: ContactInput, organization: boolean): ContactInput => {
  if (!organization) return { ...d, kind: 'person' };
  const first = primaryOrganization(d);
  const name = (first.name ?? '').trim() || d.display_name.trim() || null;
  return {
    ...d,
    kind: 'organization',
    organizations: withPrimaryOrganization(d, { name, org_id: null }),
  };
};

/** 空の会社。 */
export const emptyOrganization = (): ContactOrganization => ({
  org_id: null,
  name: null,
  phonetic_name: null,
  title: null,
  department: null,
});

/** 主の会社（先頭）。無ければ空。 */
export const primaryOrganization = (d: Pick<ContactInput, 'organizations'>): ContactOrganization =>
  d.organizations[0] ?? emptyOrganization();

/** 主の会社だけを書き換えた会社の列（2 社目以降はそのまま保つ）。 */
export const withPrimaryOrganization = (
  d: Pick<ContactInput, 'organizations'>,
  patch: Partial<ContactOrganization>
): ContactOrganization[] => [{ ...primaryOrganization(d), ...patch }, ...d.organizations.slice(1)];

/** 会社の欄がすべて空か（「会社を追加」したまま何も入れていない行）。 */
export const isBlankOrganization = (o: ContactOrganization): boolean =>
  o.org_id === null &&
  [o.name, o.phonetic_name, o.title, o.department].every((v) => (v ?? '').trim() === '');
