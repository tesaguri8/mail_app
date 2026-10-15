import { useMemo, useSyncExternalStore } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { AttachmentProgress } from '@bindings/AttachmentProgress';
import type { AttachmentStage } from '@bindings/AttachmentStage';

/**
 * 添付の取得（ダウンロード）の状態を、添付ごとに全画面共通で持つ最小ストア。
 *
 * 取得は呼び出した画面（会話のバブル・全文表示）の外で進むので、状態も画面の外に置く。
 * バブルを閉じたり別の会話へ移ったりしても、戻ってきたときに同じ状態が出る。
 * 進み具合は Rust 側の "attachment:progress" イベント（services/attachment_fetch.rs）で受け取る。
 */

/** 取得する添付（どのメールのどの添付か）。 */
export interface AttachmentRef {
  id: number;
  filename: string;
  /** 添付の大きさ（BODYSTRUCTURE の値。進み具合の分母の目安）。 */
  size: number;
  emailId: number;
}

/** 添付 1 件の取得の状態。 */
export interface DownloadState {
  id: number;
  emailId: number;
  filename: string;
  stage: AttachmentStage;
  /** 取れたバイト数（転送エンコード後）。 */
  received: number;
  /** 全体のバイト数（分からなければ 0）。 */
  total: number;
  /** 取得を始めた時刻（経過時間の表示用）。 */
  startedAt: number;
  /** 失敗したときの理由（失敗していなければ undefined）。行に残し、閉じるか押し直すまで消さない。 */
  error?: unknown;
  /** この添付の取得を待っている呼び出しの数（0 になったら終わり）。 */
  callers: number;
}

const PROGRESS_EVENT = 'attachment:progress';

let downloads: ReadonlyMap<number, DownloadState> = new Map();
const listeners = new Set<() => void>();

function put(id: number, next: DownloadState | undefined): void {
  const m = new Map(downloads);
  if (next) m.set(id, next);
  else m.delete(id);
  downloads = m;
  listeners.forEach((l) => l());
}

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

/** 取得中か（失敗して行に残っているだけのものは含めない）。 */
export function isInFlight(s: DownloadState | undefined): boolean {
  return s !== undefined && s.error === undefined && s.callers > 0;
}

/** Rust からの進み具合を反映する（取得中の添付だけ）。 */
export function applyProgress(p: AttachmentProgress): void {
  const cur = downloads.get(p.attachment_id);
  if (!isInFlight(cur) || !cur) return;
  put(p.attachment_id, {
    ...cur,
    stage: p.stage,
    received: p.received,
    total: p.total > 0 ? p.total : cur.total,
  });
}

let listening = false;
function ensureListening(): void {
  if (listening) return;
  listening = true;
  listen<AttachmentProgress>(PROGRESS_EVENT, (e) => applyProgress(e.payload)).catch(() => {
    // Tauri の外（Vitest 等）では購読できない。次の取得でまた試す。
    listening = false;
  });
}

/**
 * 添付を取る処理 `fn` を、取得の状態として記録しながら走らせる。
 * 成功したら状態を消し、失敗したら理由を残して例外をそのまま投げ直す。
 * 同じ添付を続けて押した場合は 1 つの状態にまとめる（Rust 側でも取得は合流する）。
 */
export async function trackAttachment<T>(a: AttachmentRef, fn: () => Promise<T>): Promise<T> {
  ensureListening();
  const cur = downloads.get(a.id);
  put(
    a.id,
    isInFlight(cur) && cur
      ? { ...cur, callers: cur.callers + 1 }
      : {
          id: a.id,
          emailId: a.emailId,
          filename: a.filename,
          stage: 'preparing',
          received: 0,
          total: a.size,
          startedAt: Date.now(),
          callers: 1,
        }
  );
  const finish = (error?: unknown) => {
    const now = downloads.get(a.id);
    if (!now) return;
    if (error !== undefined) put(a.id, { ...now, error, callers: 0 });
    else if (now.callers > 1) put(a.id, { ...now, callers: now.callers - 1 });
    else put(a.id, undefined);
  };
  try {
    const r = await fn();
    finish();
    return r;
  } catch (e) {
    finish(e ?? 'error');
    throw e;
  }
}

/** 行に残った失敗を閉じる。 */
export function dismissDownload(id: number): void {
  if (!isInFlight(downloads.get(id))) put(id, undefined);
}

/** 添付 1 件の取得の状態を購読する（その添付が変わったときだけ描き直す）。 */
export function useAttachmentDownload(id: number): DownloadState | undefined {
  return useSyncExternalStore(subscribe, () => downloads.get(id));
}

/**
 * メール 1 通の添付のうち、状態のあるもの（取得中・失敗）の一覧の鍵（"id:取得中なら1"）。
 * 文字列なので、進み具合だけが変わっても描き直さない（始まり・終わり・失敗でだけ変わる）。
 */
export function emailDownloadKey(m: ReadonlyMap<number, DownloadState>, emailId: number): string {
  return [...m.values()]
    .filter((d) => d.emailId === emailId)
    .map((d) => `${d.id}:${isInFlight(d) ? 1 : 0}`)
    .join(',');
}

/** {@link emailDownloadKey} を解いた、状態のある添付の id と取得中かどうか。 */
export function parseDownloadKey(key: string): { id: number; inFlight: boolean }[] {
  if (key === '') return [];
  return key.split(',').map((part) => {
    const [id, flag] = part.split(':');
    return { id: Number(id), inFlight: flag === '1' };
  });
}

/** メール 1 通の、状態のある添付（取得中・失敗）を購読する。 */
export function useEmailDownloads(emailId: number): { id: number; inFlight: boolean }[] {
  const key = useSyncExternalStore(subscribe, () => emailDownloadKey(downloads, emailId));
  return useMemo(() => parseDownloadKey(key), [key]);
}

/**
 * 進み具合のパーセント（0〜100）。全体の大きさが分からないときは null（動いている帯で出す）。
 * 取得中は 99 で止め、100 は保存の段に入ってから出す（取り終えたように見せない）。
 */
export function downloadPercent(s: DownloadState): number | null {
  if (s.stage === 'saving') return 100;
  if (s.total <= 0 || s.stage !== 'fetching') return null;
  return Math.min(99, Math.floor((s.received / s.total) * 100));
}

/** 経過時間を m:ss で出す。 */
export function formatElapsed(ms: number): string {
  const sec = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(sec / 60)}:${String(sec % 60).padStart(2, '0')}`;
}

/** 試験用: 状態を空に戻す。 */
export function resetDownloadsForTest(): void {
  downloads = new Map();
}

/** 試験用: いまの状態。 */
export function downloadsForTest(): ReadonlyMap<number, DownloadState> {
  return downloads;
}
