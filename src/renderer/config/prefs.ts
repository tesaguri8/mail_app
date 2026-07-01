// ユーザー設定（クライアント側・localStorage）。サーバー(Rust)は設定を持たず、
// フロントが取得可否などを判断する。変更は 'rondine:prefs' イベントで通知する。

const INLINE_IMAGES_KEY = 'rondine.inlineImages';
const FLY_ANIMATION_KEY = 'rondine.flyAnimation';
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
