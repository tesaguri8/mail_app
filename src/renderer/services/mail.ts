import { invoke } from '@tauri-apps/api/core';
import type { MailSummary } from '@bindings/MailSummary';
import type { MailDetail } from '@bindings/MailDetail';
import type { SyncResult } from '@bindings/SyncResult';

// Tauri v2 は camelCase の引数キーを snake_case の Rust 引数へ自動変換する。
export const mailSync = (accountId: number) => invoke<SyncResult>('mail_sync', { accountId });

export const mailList = (accountId: number, limit: number) =>
  invoke<MailSummary[]>('mail_list', { accountId, limit });

export const mailGet = (id: number) => invoke<MailDetail>('mail_get', { id });

export const mailSetRead = (ids: number[], read: boolean) =>
  invoke<void>('mail_set_read', { ids, read });

export const mailSetStarred = (ids: number[], value: boolean) =>
  invoke<void>('mail_set_starred', { ids, value });

export const mailSetBookmarked = (ids: number[], value: boolean) =>
  invoke<void>('mail_set_bookmarked', { ids, value });

export const mailDelete = (ids: number[]) => invoke<void>('mail_delete', { ids });

export const accountSetSyncWindow = (accountId: number, window: string) =>
  invoke<void>('account_set_sync_window', { accountId, window });
