import { invoke } from '@tauri-apps/api/core';
import type { MailSummary } from '@bindings/MailSummary';
import type { MailDetail } from '@bindings/MailDetail';
import type { SyncResult } from '@bindings/SyncResult';
import type { AttachmentSummary } from '@bindings/AttachmentSummary';
import type { StorageInfo } from '@bindings/StorageInfo';
import type { RetentionReport } from '@bindings/RetentionReport';

// Tauri v2 は camelCase の引数キーを snake_case の Rust 引数へ自動変換する。
export const mailSync = (accountId: number) => invoke<SyncResult>('mail_sync', { accountId });

export const mailList = (accountId: number, limit: number) =>
  invoke<MailSummary[]>('mail_list', { accountId, limit });

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
