// 開いている会話を、同期で新しいメールが入ったときに裏で読み直すための決め（純関数）。
// 送った返信・転送の控えが同期で入ったら、会話を開いたままでもバブルに出す（docs/SYNC.md §5-3）。
import type { ThreadView } from '@bindings/ThreadView';

/** 同期の合図で会話を読み直すか。新しく取り込んだ件数が 1 以上のときだけ（0 なら何も変わらない）。 */
export function shouldRefreshOnSync(stored: number | undefined): boolean {
  return (stored ?? 0) > 0;
}

/**
 * 読み直した会話が前と同じか。同じなら差し替えない（描き直さず、スクロールも検索の位置も動かさない）。
 * 本文・フラグ・件名のどれかが変われば違うとみなす。
 */
export function sameThreadView(prev: ThreadView | null, next: ThreadView): boolean {
  return prev != null && JSON.stringify(prev) === JSON.stringify(next);
}

/** 下端にいるとみなす余り（px）。小数のスクロール量の端数で外れないように。 */
const BOTTOM_SLACK = 4;

/**
 * 読み直したあとのスクロール: 下端にいたなら新しい最後のバブルへ追う（チャットと同じ。送った返信が
 * 下に増えたのが見える）、そうでなければその場に留まる（読んでいる場所を動かさない）。
 */
export function scrollAfterRefresh(el: {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}): 'follow' | 'keep' {
  return el.scrollHeight - el.clientHeight - el.scrollTop <= BOTTOM_SLACK ? 'follow' : 'keep';
}

/**
 * 読み直しで検索の一致の数が変わっても、いま見ている一致の番号（1 始まり）を保つ。
 * はみ出したら最後の一致、一致が無くなれば 0。
 */
export function keepMatch(idx: number, total: number): { idx: number; total: number } {
  if (total <= 0) return { idx: 0, total: 0 };
  return { idx: Math.min(Math.max(idx, 1), total), total };
}
