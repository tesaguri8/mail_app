// ユーザー設定（クライアント側・localStorage）。サーバー(Rust)は設定を持たず、
// フロントが取得可否などを判断する。変更は 'rondine:prefs' イベントで通知する。

const INLINE_IMAGES_KEY = 'rondine.inlineImages';
const PHONE_REGION_KEY = 'rondine.phoneRegion';
const PHONE_STYLE_KEY = 'rondine.phoneStyle';
const POSTAL_AUTOFORMAT_KEY = 'rondine.postalAutoformat';
export const PREFS_EVENT = 'rondine:prefs';

/** 本文埋め込み画像（inline asset）を自動取得して表示するか。既定: オン。 */
export function getInlineImages(): boolean {
  return localStorage.getItem(INLINE_IMAGES_KEY) !== '0';
}

export function setInlineImages(value: boolean): void {
  localStorage.setItem(INLINE_IMAGES_KEY, value ? '1' : '0');
  window.dispatchEvent(new Event(PREFS_EVENT));
}

/** 電話番号の既定の国（ISO 3166-1 alpha-2）。国内番号の解釈と新規行の初期値に使う。既定: JP。 */
export function getPhoneRegion(): string {
  return localStorage.getItem(PHONE_REGION_KEY) || 'JP';
}

export function setPhoneRegion(region: string): void {
  localStorage.setItem(PHONE_REGION_KEY, region);
  window.dispatchEvent(new Event(PREFS_EVENT));
}

/** 電話番号の表示スタイル。'national'（国内表記）/ 'international'（国際表記）。既定: national。 */
export function getPhoneStyle(): 'national' | 'international' {
  return localStorage.getItem(PHONE_STYLE_KEY) === 'international' ? 'international' : 'national';
}

export function setPhoneStyle(style: 'national' | 'international'): void {
  localStorage.setItem(PHONE_STYLE_KEY, style);
  window.dispatchEvent(new Event(PREFS_EVENT));
}

/** 郵便番号を自動整形するか（日本: 7桁→NNN-NNNN）。既定: オン。 */
export function getPostalAutoformat(): boolean {
  return localStorage.getItem(POSTAL_AUTOFORMAT_KEY) !== '0';
}

export function setPostalAutoformat(value: boolean): void {
  localStorage.setItem(POSTAL_AUTOFORMAT_KEY, value ? '1' : '0');
  window.dispatchEvent(new Event(PREFS_EVENT));
}
