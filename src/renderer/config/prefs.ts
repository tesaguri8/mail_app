// ユーザー設定（クライアント側・localStorage）。サーバー(Rust)は設定を持たず、
// フロントが取得可否などを判断する。変更は 'rondine:prefs' イベントで通知する。

const INLINE_IMAGES_KEY = 'rondine.inlineImages';
export const PREFS_EVENT = 'rondine:prefs';

/** 本文埋め込み画像（inline asset）を自動取得して表示するか。既定: オン。 */
export function getInlineImages(): boolean {
  return localStorage.getItem(INLINE_IMAGES_KEY) !== '0';
}

export function setInlineImages(value: boolean): void {
  localStorage.setItem(INLINE_IMAGES_KEY, value ? '1' : '0');
  window.dispatchEvent(new Event(PREFS_EVENT));
}
