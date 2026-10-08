// 連絡先の編集用の下書き（ContactInput）を組み立てる小道具。
//
// 連絡先の中身は Rust 側の ContactFields と同じ形で、一覧/詳細（ContactSummary）と
// 入力（ContactInput）が共有している。編集画面が扱わない項目（ミドルネーム・URL・
// 記念日など）も、読み込んだ値をそのまま下書きに写して送れば保存で消えない。

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
  show_as_company: false,
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
  show_as_company: c.show_as_company,
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

/** 空の会社。 */
const emptyOrganization = (): ContactOrganization => ({
  org_id: null,
  name: null,
  phonetic_name: null,
  title: null,
  department: null,
});

/** 主の会社（先頭）。無ければ空。いまの編集画面は主の 1 社だけを扱う。 */
export const primaryOrganization = (d: Pick<ContactInput, 'organizations'>): ContactOrganization =>
  d.organizations[0] ?? emptyOrganization();

/** 主の会社だけを書き換えた会社の列（2 社目以降はそのまま保つ）。 */
export const withPrimaryOrganization = (
  d: Pick<ContactInput, 'organizations'>,
  patch: Partial<ContactOrganization>
): ContactOrganization[] => [{ ...primaryOrganization(d), ...patch }, ...d.organizations.slice(1)];
