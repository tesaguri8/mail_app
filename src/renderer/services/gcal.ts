import { invoke } from '@tauri-apps/api/core';
import type { GcalSyncResult } from '@bindings/GcalSyncResult';

// Google カレンダー双方向同期（docs/CALENDAR_SYNC.md）。
// アカウント連携そのものは services/google.ts（カレンダー・連絡先で共通）。

/** 指定アカウントのカレンダーを双方向同期する。 */
export const gcalSync = (accountId: number) =>
  invoke<GcalSyncResult>('gcal_sync', { accountId });
