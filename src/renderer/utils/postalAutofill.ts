import type { PostalAddress } from '@bindings/PostalAddress';

// 住所欄の郵便番号の自動入力（日本）。どの欄をいつ引くか・何を入れるかを決める純粋な関数。
// 実際の検索は services/postal.ts（同梱の郵便番号表）。

/** 住所欄のうち、自動入力が扱う項目（連絡先の住所・組織の所在地で共通）。 */
export interface PostalFields {
  postal: string | null;
  region: string | null;
  city: string | null;
  street: string | null;
  extended: string | null;
  country: string | null;
}

const filled = (s: string | null | undefined) => !!s && s.trim() !== '';

/** 国が空か日本のときだけ働かせる（国コードがあればそれも見る）。 */
export function isJapanese(country: string | null, countryCode?: string | null): boolean {
  if (countryCode && countryCode.trim() !== '' && countryCode.trim().toUpperCase() !== 'JP') return false;
  if (!filled(country)) return true;
  return /^(日本|日本国|japan|jp)$/i.test((country ?? '').trim());
}

/** 郵便番号の数字だけ（全角数字・ハイフン・空白を吸収）。 */
export function postalDigits(raw: string | null): string {
  return (raw ?? '')
    .replace(/[０-９]/g, (c) => String.fromCharCode(c.charCodeAt(0) - 0xfee0))
    .replace(/[^0-9]/g, '');
}

/** 郵便番号から住所を引くか: 7 桁が入り、都道府県・市区町村・町域がどれも空。 */
export function wantsAddressFromCode(a: PostalFields, countryCode?: string | null): boolean {
  return (
    isJapanese(a.country, countryCode) &&
    postalDigits(a.postal).length === 7 &&
    !filled(a.region) &&
    !filled(a.city) &&
    !filled(a.street)
  );
}

/** 住所から郵便番号を引くか: 郵便番号が空で、都道府県・市区町村・町域がそろっている。 */
export function wantsCodeFromAddress(a: PostalFields, countryCode?: string | null): boolean {
  return (
    isJapanese(a.country, countryCode) &&
    !filled(a.postal) &&
    filled(a.region) &&
    filled(a.city) &&
    filled(a.street)
  );
}

/** 引いた住所を、まだ空の欄にだけ入れる差分（入っている値は上書きしない）。 */
export function patchFromAddress(current: PostalFields, found: PostalAddress): Partial<PostalFields> {
  const patch: Partial<PostalFields> = {};
  if (!filled(current.region) && found.region) patch.region = found.region;
  if (!filled(current.city) && found.city) patch.city = found.city;
  if (!filled(current.street) && found.town) patch.street = found.town;
  return patch;
}

/** 引いた郵便番号を、郵便番号が空のときだけ入れる差分。 */
export function patchFromCode(current: PostalFields, found: PostalAddress): Partial<PostalFields> {
  return filled(current.postal) ? {} : { postal: found.postal };
}
