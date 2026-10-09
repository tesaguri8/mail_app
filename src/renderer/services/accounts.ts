import { invoke } from '@tauri-apps/api/core';
import type { AutoconfigResult } from '@bindings/AutoconfigResult';
import type { AccountInput } from '@bindings/AccountInput';
import type { AccountProfile } from '@bindings/AccountProfile';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { ServerAccountSummary } from '@bindings/ServerAccountSummary';

export const accountAutoconfig = (email: string) =>
  invoke<AutoconfigResult>('account_autoconfig', { email });

export const accountAdd = (input: AccountInput, password: string) =>
  invoke<AccountSummary>('account_add', { input, password });

export const accountList = () => invoke<AccountSummary[]>('account_list');

/** 設定の「アカウント」のカード一覧（アドレスごと。中身はメール・Google 連携の id で指す）。 */
export const accountProfiles = () => invoke<AccountProfile[]>('account_profiles');

/** カードの並び順を保存（渡した ID 順。メールの一覧の並びもそろう）。 */
export const accountProfileReorder = (ids: number[]) =>
  invoke<void>('account_profile_reorder', { ids });

/** カードの呼び名を変える（null・空ならアドレスで出す）。 */
export const accountProfileRename = (profileId: number, name: string | null) =>
  invoke<void>('account_profile_rename', { profileId, name });

export const serverAccountList = () => invoke<ServerAccountSummary[]>('server_account_list');

export const accountTestConnection = (host: string, port: number) =>
  invoke<void>('account_test_connection', { host, port });

export const accountTestLogin = (
  host: string,
  port: number,
  username: string,
  password: string
) => invoke<void>('account_test_login', { host, port, username, password });

export const accountDelete = (accountId: number) =>
  invoke<void>('account_delete', { accountId });

// アカウントの並び順を保存（渡した ID 順）。ドラッグ＆ドロップ用。
export const accountReorder = (ids: number[]) => invoke<void>('account_reorder', { ids });

export const accountCheck = (accountId: number) =>
  invoke<void>('account_check', { accountId });

// IMAP サーバーへの TCP 到達確認だけ（LOGIN しない・タイムアウトつき）。接続ドットの軽量チェック用。
export const accountPing = (accountId: number) =>
  invoke<void>('account_ping', { accountId });

export const accountUpdate = (
  accountId: number,
  displayName: string | null,
  signatureId: number | null
) => invoke<void>('account_update', { accountId, displayName, signatureId });

/** 既存アカウントのパスワードだけを入れ直す（資格情報が失われたときの復旧用）。 */
export const accountSetPassword = (accountId: number, password: string) =>
  invoke<void>('account_set_password', { accountId, password });
