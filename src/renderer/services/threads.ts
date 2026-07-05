import { invoke } from '@tauri-apps/api/core';
import type { ThreadView } from '@bindings/ThreadView';
import type { ThreadListItem } from '@bindings/ThreadListItem';

// スレッド単位のメール一覧（代表＝フォルダ内最新＋「N通」件数）。docs/THREADING.md §5。
// accountId が null なら全アカウント横断。offset でページング。
export const threadList = (
  accountId: number | null,
  folder: string,
  limit: number,
  offset = 0,
) =>
  invoke<ThreadListItem[]>('thread_list', {
    accountId: accountId ?? null,
    folder,
    limit,
    offset,
  });

// フォルダ内のスレッド総数（一覧の「表示 X / 全 Y」表示用）。accountId が null なら全アカウント。
export const threadCount = (accountId: number | null, folder: string) =>
  invoke<number>('thread_count', { accountId: accountId ?? null, folder });

// 指定メールが属する論理スレッドの会話（時系列・古い順）を取得する。
// 未割当の旧データはバックエンドが遅延割当する（docs/THREADING.md §5）。
export const threadView = (emailId: number) => invoke<ThreadView>('thread_view', { emailId });

// スレッドにアプリ独自タイトルを付ける（再件名）。title=null で既定へ戻す。
export const threadRename = (threadId: number, title: string | null) =>
  invoke<void>('thread_rename', { threadId, title });

// メールを別スレッドへ切り出す（手動分割）。mode: 'this'（この1通）| 'below'（このメール以降）。
// 新スレッド id を返す。
export const threadSplit = (emailId: number, mode: 'this' | 'below') =>
  invoke<number>('thread_split', { emailId, mode });

// 2 つの論理スレッドを結合する（source を target へ）。
export const threadMerge = (sourceThread: number, targetThread: number) =>
  invoke<void>('thread_merge', { sourceThread, targetThread });

// メール1通を指定スレッドへ付け替える（手動）。
export const messageReassign = (emailId: number, targetThread: number) =>
  invoke<void>('message_reassign', { emailId, targetThread });

// アカウントの auto スレッド割当を作り直す（manual は保持）。
export const threadRebuild = (accountId: number) => invoke<void>('thread_rebuild', { accountId });
