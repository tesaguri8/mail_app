import { invoke } from '@tauri-apps/api/core';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import type { GcalSyncResult } from '@bindings/GcalSyncResult';
import type { GcalCredentialsStatus } from '@bindings/GcalCredentialsStatus';

// Google カレンダー双方向同期（docs/CALENDAR_SYNC.md）。
// 資格情報は Rust 側で keyring/app_settings に保存し、フロントは値を保持しない。

/** OAuth クライアント資格情報（Client ID / Secret）を保存する。 */
export const gcalSetCredentials = (clientId: string, clientSecret: string) =>
  invoke<void>('gcal_set_credentials', { clientId, clientSecret });

/** OAuth クライアント資格情報の設定状況（値は返らず、有無とヒントのみ）。 */
export const gcalCredentialsStatus = () =>
  invoke<GcalCredentialsStatus>('gcal_credentials_status');

/** 連携済み Google アカウント一覧。 */
export const gcalAccounts = () => invoke<GoogleAccount[]>('gcal_accounts');

/** Google アカウントを連携する（ブラウザで同意 → 完了で解決）。 */
export const gcalConnect = () => invoke<GoogleAccount>('gcal_connect');

/** 連携を解除する（取り込んだカレンダー/予定も削除）。 */
export const gcalDisconnect = (accountId: number) =>
  invoke<void>('gcal_disconnect', { accountId });

/** 指定アカウントのカレンダーを双方向同期する。 */
export const gcalSync = (accountId: number) =>
  invoke<GcalSyncResult>('gcal_sync', { accountId });
