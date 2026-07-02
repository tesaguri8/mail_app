// ユーザー設定（クライアント側・localStorage）。サーバー(Rust)は設定を持たず、
// フロントが取得可否などを判断する。変更は 'rondine:prefs' イベントで通知する。

const INLINE_IMAGES_KEY = 'rondine.inlineImages';
const FLY_ANIMATION_KEY = 'rondine.flyAnimation';
const PHONE_REGION_KEY = 'rondine.phoneRegion';
const PHONE_STYLE_KEY = 'rondine.phoneStyle';
const POSTAL_AUTOFORMAT_KEY = 'rondine.postalAutoformat';
const AUTO_SYNC_SEC_KEY = 'rondine.autoSyncSec';
const HOME_COUNT_MODE_KEY = 'rondine.homeCountMode';
export const PREFS_EVENT = 'rondine:prefs';

/** 本文埋め込み画像（inline asset）を自動取得して表示するか。既定: オン。 */
export function getInlineImages(): boolean {
  return localStorage.getItem(INLINE_IMAGES_KEY) !== '0';
}

export function setInlineImages(value: boolean): void {
  localStorage.setItem(INLINE_IMAGES_KEY, value ? '1' : '0');
  window.dispatchEvent(new Event(PREFS_EVENT));
}

/**
 * 送信時の「つばめが飛ぶ」演出（Fly）を使うか。既定: オン（docs/FLY_SEND.md）。
 * オフ時は送信ボタンを通常の「送信」ボタンにする。
 */
export function getFlyAnimation(): boolean {
  return localStorage.getItem(FLY_ANIMATION_KEY) !== '0';
}

export function setFlyAnimation(value: boolean): void {
  localStorage.setItem(FLY_ANIMATION_KEY, value ? '1' : '0');
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

/**
 * 自動同期の間隔（秒）。ホーム/メールモード滞在中にこの間隔で同期する。
 * 0 = 自動同期オフ（画面遷移時の同期は常に行う）。既定: 30 秒、下限 10 秒。
 */
export function getAutoSyncInterval(): number {
  const stored = localStorage.getItem(AUTO_SYNC_SEC_KEY);
  if (stored === null) return 30; // 未設定は既定 30 秒
  const n = Number(stored);
  if (!Number.isFinite(n) || n <= 0) return 0; // 0 や不正値はオフ
  return Math.max(10, Math.round(n));
}

export function setAutoSyncInterval(sec: number): void {
  const v = Number.isFinite(sec) && sec > 0 ? Math.max(10, Math.round(sec)) : 0;
  localStorage.setItem(AUTO_SYNC_SEC_KEY, String(v));
  window.dispatchEvent(new Event(PREFS_EVENT));
}

/** ホームのアカウント別バッジに出す件数。'unread'=未読数 / 'total'=全数 / 'hidden'=非表示。既定: unread。 */
export type HomeCountMode = 'unread' | 'total' | 'hidden';

export function getHomeCountMode(): HomeCountMode {
  const v = localStorage.getItem(HOME_COUNT_MODE_KEY);
  return v === 'total' || v === 'hidden' ? v : 'unread';
}

export function setHomeCountMode(mode: HomeCountMode): void {
  localStorage.setItem(HOME_COUNT_MODE_KEY, mode);
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
