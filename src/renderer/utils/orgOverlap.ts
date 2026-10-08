// 個人の連絡先と、所属する組織カード（代表電話・FAX・代表メール・所在地）の重なりを見つけ、
// 「組織に登録されています。統合しますか？」で一本化する（利用者の要望 2026-10-03）。
//
// 統合は「個人側の写しをやめて、組織の値を参照して表示する」こと。ただしメールアドレスは
// 消さずに共有（会社のアドレス）の印を付けるだけにする。差出人の照合（知り合い・氏名の解決）は
// 連絡先のアドレスで行っており、組織の代表メールは照合に使われないため、消すとそのアドレスからの
// メールが誰のものか分からなくなる。電話・FAX・住所は照合に使わないので、個人側から外してよい。

import type { ContactInput } from '@bindings/ContactInput';
import type { ContactAddressInput } from '@bindings/ContactAddressInput';
import type { OrgAddress } from '@bindings/OrgAddress';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';

/** 組織と重なっている個人側の値（配列の添字）。 */
export type OrgOverlap = {
  /** 代表メールと同じで、まだ共有の印が無いメール。 */
  emails: number[];
  /** 代表電話・FAX と同じ電話。 */
  phones: number[];
  /** 所在地と同じ住所。 */
  addresses: number[];
};

const norm = (s?: string | null) => (s ?? '').trim().replace(/\s+/g, ' ').toLowerCase();
const digits = (s?: string | null) => (s ?? '').replace(/\D/g, '');

const ADDRESS_KEYS = ['region', 'city', 'street', 'extended', 'country'] as const;

const sameAddress = (a: ContactAddressInput, o: OrgAddress): boolean =>
  digits(a.postal) === digits(o.postal) && ADDRESS_KEYS.every((k) => norm(a[k]) === norm(o[k]));

const isEmptyAddress = (o: OrgAddress): boolean =>
  !digits(o.postal) && ADDRESS_KEYS.every((k) => !norm(o[k]));

/**
 * 個人の連絡先のうち、組織カードと同じ値を探す。
 *
 * @param phoneKey 電話番号を比べられる形へ（呼び出し側で E.164 へ正規化する。表記の違い
 *   `03-1234-5678` と `+81312345678` を同じと見るため）
 */
export function findOrgOverlap(
  draft: Pick<ContactInput, 'emails' | 'phones' | 'addresses'>,
  org: OrganizationSummary,
  phoneKey: (v: string) => string
): OrgOverlap {
  const orgEmail = norm(org.email);
  const orgPhones = [org.phone, org.fax]
    .map((p) => (p ?? '').trim())
    .filter(Boolean)
    .map(phoneKey);
  const indexes = <T>(xs: T[], hit: (x: T) => boolean) => xs.flatMap((x, i) => (hit(x) ? [i] : []));
  return {
    emails: orgEmail
      ? indexes(draft.emails, (e) => !e.is_shared && norm(e.value) === orgEmail)
      : [],
    phones: indexes(
      draft.phones,
      (p) => p.value.trim() !== '' && orgPhones.includes(phoneKey(p.value.trim()))
    ),
    addresses: isEmptyAddress(org.address)
      ? []
      : indexes(draft.addresses, (a) => sameAddress(a, org.address)),
  };
}

/** 重なりが 1 つでもあるか。 */
export const hasOrgOverlap = (o: OrgOverlap): boolean =>
  o.emails.length + o.phones.length + o.addresses.length > 0;

/** 重なりを統合した下書きを返す（電話・住所は外し、メールは共有の印を付ける）。 */
export function mergeOrgOverlap<T extends Pick<ContactInput, 'emails' | 'phones' | 'addresses'>>(
  draft: T,
  overlap: OrgOverlap
): T {
  return {
    ...draft,
    emails: draft.emails.map((e, i) =>
      overlap.emails.includes(i) ? { ...e, is_shared: true } : e
    ),
    phones: draft.phones.filter((_, i) => !overlap.phones.includes(i)),
    addresses: draft.addresses.filter((_, i) => !overlap.addresses.includes(i)),
  };
}
