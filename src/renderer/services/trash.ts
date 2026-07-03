import { invoke } from '@tauri-apps/api/core';

/** ゴミ箱（連絡先・組織の論理削除）の保持日数を取得（既定 7）。 */
export const trashRetentionGet = () => invoke<number>('trash_retention_get');

/** ゴミ箱の保持日数を保存。 */
export const trashRetentionSet = (days: number) => invoke<void>('trash_retention_set', { days });

/** 保持期間を過ぎたゴミ箱を今すぐ完全削除。 */
export const trashPurge = () => invoke<void>('trash_purge');
