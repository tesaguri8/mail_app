import { useSyncExternalStore } from 'react';

/**
 * バックグラウンド作業（メール更新・添付ダウンロード等）を全画面共通で「フッターに進捗表示」する
 * ための最小ストア。React 外（フック内の非同期処理）からも push できるよう、useSyncExternalStore
 * ベースの素朴な外部ストアで実装する（依存追加なし）。
 *
 * - progress を持たない作業はスピナー（不確定）として、持つ作業はバーとして表示する。
 * - 同時に複数走っても配列で保持し、フッターは先頭 1 件を代表表示する。
 */
export interface Activity {
  /** 一意 ID（開始時に採番）。 */
  id: number;
  /** 表示ラベル（例:「メールを更新中…」）。 */
  label: string;
  /** 進捗（分かる場合のみ）。無ければ不確定スピナー扱い。 */
  current?: number;
  total?: number;
}

let activities: Activity[] = [];
let seq = 0;
const listeners = new Set<() => void>();

function emit(): void {
  // 参照を作り替えて useSyncExternalStore に変更を伝える。
  activities = activities.slice();
  listeners.forEach((l) => l());
}

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

/** 作業を開始し、識別 ID を返す（完了時に activityStop へ渡す）。 */
export function activityStart(label: string): number {
  const id = ++seq;
  activities.push({ id, label });
  emit();
  return id;
}

/** 作業の進捗（current/total）やラベルを更新する。 */
export function activityUpdate(id: number, patch: Partial<Omit<Activity, 'id'>>): void {
  const i = activities.findIndex((a) => a.id === id);
  if (i < 0) return;
  activities[i] = { ...activities[i], ...patch };
  emit();
}

/** 作業を終了して一覧から取り除く。 */
export function activityStop(id: number): void {
  const next = activities.filter((a) => a.id !== id);
  if (next.length === activities.length) return;
  activities = next;
  emit();
}

/** 非同期処理の実行前後で作業表示を自動開始/終了するラッパー。 */
export async function withActivity<T>(label: string, fn: () => Promise<T>): Promise<T> {
  const id = activityStart(label);
  try {
    return await fn();
  } finally {
    activityStop(id);
  }
}

/** フッター等で現在のバックグラウンド作業一覧を購読する。 */
export function useActivities(): Activity[] {
  return useSyncExternalStore(subscribe, () => activities);
}
