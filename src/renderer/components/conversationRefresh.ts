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

/**
 * 会話の References にあって手元に無いメールを、サーバーへ探しに行くか（docs/THREADING.md §5）。
 * まだ探していない（または 30 日後の探し直しの時期が来た）ものがあり、この画面で開いてから
 * まだ探しに行っていない会話だけ。探すのはバックエンドが 1 会話 1 回に絞るが、画面からも
 * 開き直しのたびに呼ばないようにする。
 */
export function shouldFetchMissingRefs(
  view: ThreadView | null,
  tried: ReadonlySet<number>
): boolean {
  return (
    view != null && !tried.has(view.thread.id) && view.missing_refs.some((r) => r.state === 'due')
  );
}

/**
 * 開いている会話で、`ids` のメールを既読にした形にする（未読のスレッドを見て既読にした合図。
 * 読み直さずにバブルの未読の点を消す）。`ids` に無いメールはそのまま（手動で未読に戻したものを
 * 勝手に既読にしない）。変わるものが無ければ同じ値を返す（描き直さない）。
 */
export function markMessagesRead(
  view: ThreadView | null,
  ids: readonly number[]
): ThreadView | null {
  if (!view) return view;
  const set = new Set(ids);
  if (!view.messages.some((m) => set.has(m.id) && !m.is_read)) return view;
  return {
    ...view,
    messages: view.messages.map((m) => (set.has(m.id) && !m.is_read ? { ...m, is_read: true } : m)),
  };
}
