/** タグの配色パレット（新規作成時に巡回して割り当て、チップ表示にも使う）。 */
export const TAG_PALETTE = [
  '#ef4444', // red
  '#f97316', // orange
  '#f59e0b', // amber
  '#10b981', // emerald
  '#14b8a6', // teal
  '#3b82f6', // blue
  '#8b5cf6', // violet
  '#ec4899', // pink
] as const;

/** インデックス（既存タグ数など）から配色を一つ選ぶ。 */
export function pickTagColor(index: number): string {
  return TAG_PALETTE[((index % TAG_PALETTE.length) + TAG_PALETTE.length) % TAG_PALETTE.length];
}

/** 色未設定タグのフォールバック色。 */
export const DEFAULT_TAG_COLOR = '#64748b'; // slate
