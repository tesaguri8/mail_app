import { useCallback, useState } from 'react';
import { ALL_OPEN, type FoldState } from '../components/mailListWindow';

/**
 * メール一覧の日の畳み（`FoldState`）。設計: docs/MAIL_LIST_TIMELINE.md §6
 *
 * **起動したときも、アカウント・フォルダを切り替えたときも、全部開いた状態から始める**
 * （利用者の返事「全展開でお願いします」。2026-11-06）。畳みは見ている間だけの見方で、覚えない
 * （以前は localStorage に覚えていたが、起動時に戻さないので切り替えで戻すのもやめた）。
 * scope（例: `"3.inbox"`）が変わったら全部開いた状態に戻す。
 */
export function useDayFold(
  scope: string | null
): [FoldState, (fn: (prev: FoldState) => FoldState) => void] {
  const [state, setState] = useState<{ scope: string | null; fold: FoldState }>({
    scope,
    fold: ALL_OPEN,
  });
  // 選択が変わったら、描く前に戻す（前の選択の畳みで 1 回描かない）
  let current = state;
  if (state.scope !== scope) {
    current = { scope, fold: ALL_OPEN };
    setState(current);
  }
  const update = useCallback((fn: (prev: FoldState) => FoldState) => {
    setState((prev) => ({ scope: prev.scope, fold: fn(prev.fold) }));
  }, []);
  return [current.fold, update];
}
