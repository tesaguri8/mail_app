import { invoke } from '@tauri-apps/api/core';

/** ゴミ箱（連絡先・組織の論理削除）の保持日数を取得（既定 7）。 */
export const trashRetentionGet = () => invoke<number>('trash_retention_get');

/** ゴミ箱の保持日数を保存。 */
export const trashRetentionSet = (days: number) => invoke<void>('trash_retention_set', { days });

/** 保持期間を過ぎたゴミ箱を今すぐ完全削除。 */
export const trashPurge = () => invoke<void>('trash_purge');

/** メールのゴミ箱の保持日数を取得（既定 30。0 = 無期限）。 */
export const mailTrashRetentionGet = () => invoke<number>('mail_trash_retention_get');

/** メールのゴミ箱の保持日数を保存（0 = 無期限）。 */
export const mailTrashRetentionSet = (days: number) =>
  invoke<void>('mail_trash_retention_set', { days });

/** 保持期間を過ぎたゴミ箱メールを今すぐ完全削除（0 = 無期限なら何もしない）。 */
export const mailTrashPurge = () => invoke<void>('mail_trash_purge');
