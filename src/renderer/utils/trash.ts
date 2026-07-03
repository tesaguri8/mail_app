/**
 * 論理削除された項目の「完全削除までの残り日数」を求める。
 * deleted_at は SQLite の UTC 文字列（"YYYY-MM-DD HH:MM:SS"）。
 * 残り 0（=まもなく削除）を下限にクランプする。
 */
export function trashDaysLeft(deletedAt: string | null, retentionDays: number): number {
  if (!deletedAt) return 0;
  const deleted = new Date(deletedAt.replace(' ', 'T') + 'Z').getTime();
  if (Number.isNaN(deleted)) return 0;
  const purgeAt = deleted + retentionDays * 86_400_000;
  const ms = purgeAt - Date.now();
  return Math.max(0, Math.ceil(ms / 86_400_000));
}
