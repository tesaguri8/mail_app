import { invoke } from '@tauri-apps/api/core';
import type { GcontactsSyncResult } from '@bindings/GcontactsSyncResult';

// Google 連絡先（People API）の取り込み。
// アカウント連携そのものは services/google.ts（カレンダー・連絡先で共通）。
//
// 取り込み先は台帳（contact_identities）までで、住所録にはまだ反映しない。
// 既存の住所録と全件重複させないため、照合は後続フェーズで行う。

/** 指定アカウントの Google 連絡先を取り込む。 */
export const gcontactsSync = (accountId: number) =>
  invoke<GcontactsSyncResult>('gcontacts_sync', { accountId });
