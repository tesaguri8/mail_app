// 電話番号の正規化・表示ユーティリティ（libphonenumber-js ベース）。
// 保存は E.164 正準形（例: +819012345678）。UI は [国] + [国内番号] で編集し、
// 表示は設定のスタイル（国内 / 国際）で整形する。

import {
  parsePhoneNumberFromString,
  getCountries,
  getCountryCallingCode,
  type CountryCode,
} from 'libphonenumber-js';

export type PhoneStyle = 'national' | 'international';

/** 選択肢に出す国の一覧（ローカライズ名・国番号つき、名前順）。 */
export function countryOptions(lang: string): { region: CountryCode; calling: string; name: string }[] {
  let display: Intl.DisplayNames | null = null;
  try {
    display = new Intl.DisplayNames([lang], { type: 'region' });
  } catch {
    display = null;
  }
  return getCountries()
    .map((region) => ({
      region,
      calling: getCountryCallingCode(region),
      name: display?.of(region) ?? region,
    }))
    .sort((a, b) => a.name.localeCompare(b.name, lang));
}

/** 生入力を E.164 正準形へ。解釈できなければトリムした元文字列を返す。 */
export function toE164(raw: string, region: CountryCode): string {
  const s = raw.trim();
  if (!s) return '';
  const pn = parsePhoneNumberFromString(s, region);
  return pn && pn.isValid() ? pn.number : s;
}

/** 保存値（E.164 など）を「国」と「国内番号」に分解。解釈不能なら region を既定に、
 *  national に元文字列を返す（編集中の生テキストもここを通る）。 */
export function parseStored(
  stored: string,
  fallbackRegion: CountryCode
): { region: CountryCode; national: string } {
  const s = (stored ?? '').trim();
  if (!s) return { region: fallbackRegion, national: '' };
  const pn = parsePhoneNumberFromString(s, fallbackRegion);
  if (pn && pn.country) {
    return { region: pn.country, national: pn.formatNational() };
  }
  return { region: fallbackRegion, national: s };
}

/** 表示用整形。解釈できなければ元文字列。 */
export function displayPhone(stored: string, style: PhoneStyle, fallbackRegion: CountryCode): string {
  const s = (stored ?? '').trim();
  if (!s) return '';
  const pn = parsePhoneNumberFromString(s, fallbackRegion);
  if (!pn) return s;
  return style === 'international' ? pn.formatInternational() : pn.formatNational();
}
