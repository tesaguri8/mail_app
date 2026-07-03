import { invoke } from '@tauri-apps/api/core';
import type { MailSummary } from '@bindings/MailSummary';
import type { MailDetail } from '@bindings/MailDetail';
import type { SyncResult } from '@bindings/SyncResult';
import type { AttachmentSummary } from '@bindings/AttachmentSummary';
import type { StorageInfo } from '@bindings/StorageInfo';
import type { RetentionReport } from '@bindings/RetentionReport';
import type { SendInput } from '@bindings/SendInput';
import type { RemoteImage } from '@bindings/RemoteImage';

// Tauri v2 は camelCase の引数キーを snake_case の Rust 引数へ自動変換する。
export const mailSync = (accountId: number) => invoke<SyncResult>('mail_sync', { accountId });

// 実行中の同期/再取り込みを中断する（次のチャンク境界で反映）。動作中なら true。
export const mailSyncCancel = (accountId: number) =>
  invoke<boolean>('mail_sync_cancel', { accountId });

// メールを送信する（SMTP）。input は差出人アカウント・宛先・件名・本文など。
export const mailSend = (input: SendInput) => invoke<void>('mail_send', { input });

// 指定フォルダ（'inbox' | 'sent' | 'drafts' | 'trash' | 'spam'）のメール一覧（新しい順）。
// accountId が null なら全アカウント横断（「全て」表示）。offset でページング（無限スクロール用）。
export const mailList = (
  accountId: number | null,
  folder: string,
  limit: number,
  offset = 0,
) => invoke<MailSummary[]>('mail_list', { accountId: accountId ?? null, folder, limit, offset });

// 件名・差出人・本文の全文検索（FTS5）。指定アカウント/フォルダ内を絞り込む。
// accountId が null なら全アカウント横断で検索。
export const mailSearch = (
  accountId: number | null,
  folder: string,
  query: string,
  limit: number,
) => invoke<MailSummary[]>('mail_search', { accountId: accountId ?? null, folder, query, limit });

export const mailGet = (id: number) => invoke<MailDetail>('mail_get', { id });

// 1通の全文をサーバーから再取得して本文キャッシュを復元（要約保存の解除）。復元後の本文を返す。
export const mailRefetch = (id: number) => invoke<MailDetail>('mail_refetch', { id });

// 添付メタ一覧（本体未取得のものは is_downloaded=false）。
export const mailAttachments = (emailId: number) =>
  invoke<AttachmentSummary[]>('mail_attachments', { emailId });

// 添付をオンデマンドで取得・保存（取得済みなら即返る）。
export const attachmentDownload = (attachmentId: number) =>
  invoke<AttachmentSummary>('attachment_download', { attachmentId });

// 画像の添付/インラインを web 表示用 data URL に変換して取得（HEIC は JPEG 化）。
export const attachmentView = (attachmentId: number, thumb = false) =>
  invoke<string>('attachment_view', { attachmentId, thumb });

// ダウンロード済みの添付を OS の関連アプリで開く。
export const attachmentOpen = (attachmentId: number) =>
  invoke<void>('attachment_open', { attachmentId });

// 添付を指定の場所へ保存（ダウンロード）。dest は保存先フルパス。
export const attachmentExport = (attachmentId: number, dest: string) =>
  invoke<void>('attachment_export', { attachmentId, dest });

export const mailSetRead = (ids: number[], read: boolean) =>
  invoke<void>('mail_set_read', { ids, read });

export const mailSetStarred = (ids: number[], value: boolean) =>
  invoke<void>('mail_set_starred', { ids, value });

export const mailSetBookmarked = (ids: number[], value: boolean) =>
  invoke<void>('mail_set_bookmarked', { ids, value });

export const mailDelete = (ids: number[]) => invoke<void>('mail_delete', { ids });

// 迷惑としてマーク（学習＋隔離）／非迷惑に戻す（学習＋復帰）。docs/SPAM.md §7.5
export const mailMarkSpam = (ids: number[]) => invoke<void>('mail_mark_spam', { ids });

export const mailMarkNotSpam = (ids: number[]) => invoke<void>('mail_mark_not_spam', { ids });

export const accountSetSyncWindow = (accountId: number, window: string) =>
  invoke<void>('account_set_sync_window', { accountId, window });

// フルデータ保持期間を設定（'7d'/'30d'/…/'all'）。適用結果（保持レポート）を返す。
export const accountSetFullWindow = (accountId: number, window: string) =>
  invoke<RetentionReport>('account_set_full_window', { accountId, window });

// 本文の全文保持期間を設定（'off'/'3m'/…/'2y'）。適用結果（保持レポート）を返す。
export const accountSetBodyWindow = (accountId: number, window: string) =>
  invoke<RetentionReport>('account_set_body_window', { accountId, window });

// 点検つき再取り込み（フル再取得＋既存メールへ uid/添付メタを埋め戻し）。
export const mailResync = (accountId: number) => invoke<SyncResult>('mail_resync', { accountId });

// アカウントのローカル保存容量（使用量・上限）。
export const accountStorageInfo = (accountId: number) =>
  invoke<StorageInfo>('account_storage_info', { accountId });

// 容量上限を設定（バイト）。
export const accountSetStorageLimit = (accountId: number, bytes: number) =>
  invoke<void>('account_set_storage_limit', { accountId, bytes });

// ストレージ最適化（保持ポリシー適用: 古い添付削除＋本文の要約保存＋容量保険）。
export const storageOptimize = (accountId: number) =>
  invoke<RetentionReport>('storage_optimize', { accountId });

// 明示許可された外部画像を取得し、サニタイズ済み data URL を返す（docs/MAIL_SECURITY.md §1）。
// sender（差出人アドレス）が許可済みならバックエンドでディスクキャッシュする（初回だけ取得）。
export const mailLoadRemote = (urls: string[], sender: string | null) =>
  invoke<RemoteImage[]>('mail_load_remote', { urls, sender });

// 差出人アドレスの外部画像を常に許可するか（住所録の信頼設定も反映）。
export const senderRemoteAllowed = (address: string) =>
  invoke<boolean>('sender_remote_allowed', { address });

// 差出人アドレスの外部画像許可（常に許可/解除）を保存。
export const senderSetRemotePolicy = (address: string, allow: boolean) =>
  invoke<void>('sender_set_remote_policy', { address, allow });
