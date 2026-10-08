import { invoke } from '@tauri-apps/api/core';
import type { GcontactsMatchResult } from '@bindings/GcontactsMatchResult';
import type { GcontactsSyncResult } from '@bindings/GcontactsSyncResult';

// Google 連絡先（People API）の取り込み。
// アカウント連携そのものは services/google.ts（カレンダー・連絡先で共通）。
//
// 取り込み先は台帳（contact_identities）までで、住所録にはまだ反映しない。
// 既存の住所録と全件重複させないため、照合は後続フェーズで行う。

/** 指定アカウントの Google 連絡先を取り込む。 */
export const gcontactsSync = (accountId: number) =>
  invoke<GcontactsSyncResult>('gcontacts_sync', { accountId });

// 照合フェーズ: 台帳に溜まった未照合分を住所録と突き合わせる。
// 高確信（メール/携帯＋氏名の一致）だけ自動で紐付け、決めきれない分は新規として起こす。
// 起こした分のうち似た相手が居たものは、既存の「重複整理」に候補として出る。

/** 照合の下見。件数だけ返し、住所録は変えない。 */
export const gcontactsMatchPreview = (accountId: number) =>
  invoke<GcontactsMatchResult>('gcontacts_match_preview', { accountId });

/** 照合を適用する（紐付け＋新規作成）。 */
export const gcontactsMatchApply = (accountId: number) =>
  invoke<GcontactsMatchResult>('gcontacts_match_apply', { accountId });

// 片付け: 同期に置き換える前にファイル（CSV／vCard）で取り込んだ Google 連絡先の写しのうち、
// 同期とつながっていないもの。消しても Google 側には何も送られない（台帳に紐付いていないため）。

/** 同期とつながっていない Google の写しの件数。 */
export const gcontactsCopiesCount = () => invoke<number>('gcontacts_copies_count');

/** 同期とつながっていない Google の写しをゴミ箱へ移す（戻せる）。移した件数を返す。 */
export const gcontactsCopiesTrash = () => invoke<number>('gcontacts_copies_trash');

/** 「Rondine で新しく作った連絡先も Google 側に作る」設定を切り替える。 */
export const gcontactsSetPushNew = (accountId: number, enabled: boolean) =>
  invoke<void>('gcontacts_set_push_new', { accountId, enabled });
