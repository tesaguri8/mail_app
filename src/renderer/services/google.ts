import { invoke } from '@tauri-apps/api/core';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import type { GoogleDisconnectResult } from '@bindings/GoogleDisconnectResult';
import type { GoogleSyncResult } from '@bindings/GoogleSyncResult';
import type { GoogleCredentialsStatus } from '@bindings/GoogleCredentialsStatus';
import type { GoogleService } from '@bindings/GoogleService';

// Google アカウント連携（OAuth）。カレンダーと連絡先で同じアカウント・同じ認証情報を共有する。
// 資格情報は Rust 側で keyring/app_settings に保存し、フロントは値を保持しない。

/** OAuth クライアント資格情報（Client ID / Secret）を保存する。 */
export const googleSetCredentials = (clientId: string, clientSecret: string) =>
  invoke<void>('google_set_credentials', { clientId, clientSecret });

/** OAuth クライアント資格情報の設定状況（値は返らず、有無とヒントのみ）。 */
export const googleCredentialsStatus = () =>
  invoke<GoogleCredentialsStatus>('google_credentials_status');

/** 連携済み Google アカウント一覧。 */
export const googleAccounts = () => invoke<GoogleAccount[]>('google_accounts');

/**
 * Google アカウントを連携する（ブラウザで同意 → 完了で解決）。
 * オンにしたいサービス（カレンダー・連絡先）の権限だけを要求する。連携済みのアカウントに
 * 後から足す場合も、同じ呼び出しで差分同意できる。`loginHint` はカードのアドレス
 * （アカウント選択で先に選んでおく。利用者が別のアカウントを選べば、そのアドレスのカードに入る）。
 */
export const googleConnect = (calendar: boolean, contacts: boolean, loginHint?: string) =>
  invoke<GoogleAccount>('google_connect', { calendar, contacts, loginHint: loginHint ?? null });

/** サービス（カードの「カレンダー」「連絡先」のスイッチ）を切り替える。権限が無いのにオンに
 *  しようとするとエラー（先に googleConnect でログインし直す）。 */
export const googleSetService = (accountId: number, service: GoogleService, enabled: boolean) =>
  invoke<GoogleAccount>('google_set_service', { accountId, service, enabled });

/** 連携を解除する（取り込んだカレンダー/予定も削除）。 */
/** 連携の解除。purge=false は「解除中」（記録を残して同期を止める）、true は「完全に解除」
 *  （Google 側の許可の取り消し・つながりと写しの削除）。どちらも Google 側の連絡先・予定は消えない。 */
export const googleDisconnect = (accountId: number, purge: boolean) =>
  invoke<GoogleDisconnectResult>('google_disconnect', { accountId, purge });

/** アカウント 1 件を同期する。カレンダーと、contacts=true なら連絡先（push → pull）も同期し、
 *  取り込んだ連絡先を住所録へ反映する（「今すぐ同期」は常に true。自動同期は間隔を空けて true）。 */
export const googleSync = (accountId: number, contacts: boolean) =>
  invoke<GoogleSyncResult>('google_sync', { accountId, contacts });
