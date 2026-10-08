import { invoke } from '@tauri-apps/api/core';

// Google 連絡先（People API）の設定。同期そのものは services/google.ts の googleSync
// （カレンダー・連絡先・住所録への反映をまとめて行う）。

/** 「Rondine で新しく作った連絡先も Google 側に作る」設定を切り替える。 */
export const gcontactsSetPushNew = (accountId: number, enabled: boolean) =>
  invoke<void>('gcontacts_set_push_new', { accountId, enabled });
