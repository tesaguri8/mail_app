// ユーザー設定（クライアント側・localStorage）。サーバー(Rust)は設定を持たず、
// フロントが取得可否などを判断する。変更は 'rondine:prefs' イベントで通知する。

const INLINE_IMAGES_KEY = 'rondine.inlineImages';
const BUBBLE_HTML_KEY = 'rondine.bubbleHtml';
const FLY_ANIMATION_KEY = 'rondine.flyAnimation';
const PHONE_REGION_KEY = 'rondine.phoneRegion';
const PHONE_STYLE_KEY = 'rondine.phoneStyle';
const POSTAL_AUTOFORMAT_KEY = 'rondine.postalAutoformat';
const AUTO_SYNC_ON_KEY = 'rondine.autoSyncOn';
const AUTO_SYNC_SEC_KEY = 'rondine.autoSyncSec';
const HOME_COUNT_SHOW_KEY = 'rondine.homeCountShow';
const HOME_COUNT_FILTER_KEY = 'rondine.homeCountFilter';
const PHONE_AUTOFORMAT_KEY = 'rondine.phoneAutoformat';
const COMPOSE_AUTOSAVE_KEY = 'rondine.composeAutosave';
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
 * 会話バブル（折りたたみ表示）を、プレーンではなく HTML 本文で描画するか。既定: オン。
 * 画像は取得せずプレースホルダのまま（軽量・外部画像のトラッキングも起きない）。
 * 引用のある返信はチャット感を保つため描画側でプレーン（新規部分のみ）にフォールバックする
 * ので、実質「引用のないメール（ニュースレター等）を HTML 描画」する設定。
 */
export function getBubbleHtml(): boolean {
  return localStorage.getItem(BUBBLE_HTML_KEY) !== '0';
}

export function setBubbleHtml(value: boolean): void {
  localStorage.setItem(BUBBLE_HTML_KEY, value ? '1' : '0');
  window.dispatchEvent(new Event(PREFS_EVENT));
}

/**
 * リモート（外部）画像の既定表示モード（docs/MAIL_SECURITY.md §1）。
 * 'hidden'=ブロック（プレースホルダ）/ 'thumb'=サムネイル表示 / 'full'=完全表示。
 * 既定は 'hidden'（トラッキング防止・プライバシー優先）。各メールで一時的に上書きできる。
 */
export type RemoteImageMode = 'hidden' | 'thumb' | 'full';
const REMOTE_IMAGE_MODE_KEY = 'rondine.remoteImageMode';

export function getRemoteImageMode(): RemoteImageMode {
  const v = localStorage.getItem(REMOTE_IMAGE_MODE_KEY);
  return v === 'thumb' || v === 'full' ? v : 'hidden';
}

export function setRemoteImageMode(mode: RemoteImageMode): void {
  localStorage.setItem(REMOTE_IMAGE_MODE_KEY, mode);
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

/** 自動同期（ホーム/メール滞在中の定期同期）を使うか。既定: オン。画面遷移時の同期は常に行う。 */
export function getAutoSyncOn(): boolean {
  return localStorage.getItem(AUTO_SYNC_ON_KEY) !== '0';
}

export function setAutoSyncOn(value: boolean): void {
  localStorage.setItem(AUTO_SYNC_ON_KEY, value ? '1' : '0');
  window.dispatchEvent(new Event(PREFS_EVENT));
}

/** 自動同期の間隔（秒・設定値そのもの）。既定 30 秒、下限 10 秒。 */
export function getAutoSyncSeconds(): number {
  const n = Number(localStorage.getItem(AUTO_SYNC_SEC_KEY));
  if (!Number.isFinite(n) || n < 10) return 30;
  return Math.round(n);
}

export function setAutoSyncSeconds(sec: number): void {
  const v = Number.isFinite(sec) && sec >= 10 ? Math.round(sec) : 30;
  localStorage.setItem(AUTO_SYNC_SEC_KEY, String(v));
  window.dispatchEvent(new Event(PREFS_EVENT));
}

/** 実効の自動同期間隔（秒）。オフなら 0（useAutoSync が参照）。 */
export function getAutoSyncInterval(): number {
  return getAutoSyncOn() ? getAutoSyncSeconds() : 0;
}

/** ホームのアカウント別バッジを表示するか。既定: オン。 */
export function getHomeCountShow(): boolean {
  return localStorage.getItem(HOME_COUNT_SHOW_KEY) !== '0';
}

export function setHomeCountShow(value: boolean): void {
  localStorage.setItem(HOME_COUNT_SHOW_KEY, value ? '1' : '0');
  window.dispatchEvent(new Event(PREFS_EVENT));
}

/** ホームのバッジに出す未読数の対象カテゴリ。
 *  'all'=全体 / 'green'=グリーン / 'known'=住所録 / 'vip'=お気に入り。既定: green。 */
export type HomeCountFilter = 'all' | 'green' | 'known' | 'vip';

export function getHomeCountFilter(): HomeCountFilter {
  const v = localStorage.getItem(HOME_COUNT_FILTER_KEY);
  return v === 'all' || v === 'known' || v === 'vip' ? v : 'green';
}

export function setHomeCountFilter(value: HomeCountFilter): void {
  localStorage.setItem(HOME_COUNT_FILTER_KEY, value);
  window.dispatchEvent(new Event(PREFS_EVENT));
}

/** 電話番号の自動整形（[国]+[国内番号] へ整えて保存・表示）を使うか。既定: オン。 */
export function getPhoneAutoformat(): boolean {
  return localStorage.getItem(PHONE_AUTOFORMAT_KEY) !== '0';
}

export function setPhoneAutoformat(value: boolean): void {
  localStorage.setItem(PHONE_AUTOFORMAT_KEY, value ? '1' : '0');
  window.dispatchEvent(new Event(PREFS_EVENT));
}

/**
 * 新規予定の既定カレンダー（ローカル calendars.id）。最後に新規作成で使ったカレンダーを覚える。
 * 未設定（null）なら EventEditor は既定カレンダーにフォールバックする。
 */
const DEFAULT_CALENDAR_KEY = 'rondine.defaultCalendarId';

export function getDefaultCalendarId(): number | null {
  const n = Number(localStorage.getItem(DEFAULT_CALENDAR_KEY));
  return Number.isFinite(n) && n > 0 ? n : null;
}

export function setDefaultCalendarId(id: number | null): void {
  if (id == null) localStorage.removeItem(DEFAULT_CALENDAR_KEY);
  else localStorage.setItem(DEFAULT_CALENDAR_KEY, String(id));
  window.dispatchEvent(new Event(PREFS_EVENT));
}

/**
 * メール作成中の下書き自動保存を使うか。既定: オン。
 * オフのときは、書きかけを閉じようとしたときに保存を促すダイアログだけを出し、
 * 入力中に自動でローカルの下書きへ保存することはしない（意図しない下書きの量産を防ぐ）。
 */
export function getComposeAutoSave(): boolean {
  return localStorage.getItem(COMPOSE_AUTOSAVE_KEY) !== '0';
}

export function setComposeAutoSave(value: boolean): void {
  localStorage.setItem(COMPOSE_AUTOSAVE_KEY, value ? '1' : '0');
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
