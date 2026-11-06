import { useCallback, useState } from 'react';
import { ALL_FOLDED, foldAllBut, type FoldState } from '../components/mailListWindow';
import { localDayKey } from '../components/timeline/dayCounts';

/**
 * メール一覧の日の畳み（`FoldState`）を、選択（アカウント・フォルダ）ごとに覚える。
 * 置き場は localStorage（この端末の表示の好みなので）。読めない・書けないとき（private window・
 * 消された・塞がれた）は、起動時の形で動く。設計: docs/MAIL_LIST_TIMELINE.md §6
 *
 * **起動したあと、その選択を初めて開いたときは、覚えた形より起動時の形（`STARTUP_FOLD`）を優先する**
 * （利用者の提案。2026-11-06）。同じ起動の間にその選択へ戻ったときは、覚えた形（その間に開閉した形）に戻す。
 */
const KEY_PREFIX = 'rondine.mailListCollapsed';

/**
 * 起動時の形。`all` は全部畳む、`todayOnly` は今日だけ開いておく（利用者に両方の案を出してある。
 * 返事が無ければ全部畳む）。変えるのはここだけ。
 */
const STARTUP_FOLD: 'all' | 'todayOnly' = 'all';

/** 覚える日の上限（新しい日から）。何年分を開閉しても保存が膨らみすぎないように。 */
const MAX_DAYS = 4000;

/** この起動で、もう開いた選択（ここに無ければ、起動時の形から始める）。 */
const seenThisLaunch = new Set<string>();

function startupFold(): FoldState {
  return STARTUP_FOLD === 'todayOnly'
    ? foldAllBut(localDayKey(new Date().toISOString()))
    : ALL_FOLDED;
}

function storageKey(scope: string): string {
  return `${KEY_PREFIX}.${scope}`;
}

/** 保存の形: `{ "f": 既定で畳むか, "x": 既定と逆にした日 }`。以前の形（畳んだ日の配列）も読む。 */
function parse(raw: string | null): FoldState | null {
  if (!raw) return null;
  const v: unknown = JSON.parse(raw);
  const keys = (xs: unknown): Set<string> =>
    new Set(Array.isArray(xs) ? xs.filter((x): x is string => typeof x === 'string') : []);
  if (Array.isArray(v)) return { allFolded: false, flipped: keys(v) };
  if (v && typeof v === 'object' && 'f' in v && 'x' in v) {
    return { allFolded: v.f === true, flipped: keys(v.x) };
  }
  return null;
}

function read(scope: string | null): FoldState {
  if (scope == null) return startupFold();
  if (!seenThisLaunch.has(scope)) {
    seenThisLaunch.add(scope);
    const fold = startupFold();
    write(scope, fold);
    return fold;
  }
  try {
    return parse(localStorage.getItem(storageKey(scope))) ?? startupFold();
  } catch {
    return startupFold();
  }
}

function write(scope: string | null, fold: FoldState): void {
  if (scope == null) return;
  try {
    // 日付の文字列は辞書順＝日付順。新しい日を残す
    const x = [...fold.flipped].sort().reverse().slice(0, MAX_DAYS);
    localStorage.setItem(storageKey(scope), JSON.stringify({ f: fold.allFolded, x }));
  } catch {
    // 書けなくても、この画面の間は今の形のまま
  }
}

/**
 * scope（例: `"3.inbox"`。null なら覚えない）の日の畳みと、それを変える関数。
 * scope が変わったら、その選択の形を読み直す。
 */
export function useCollapsedDays(
  scope: string | null
): [FoldState, (fn: (prev: FoldState) => FoldState) => void] {
  const [state, setState] = useState(() => ({ scope, fold: read(scope) }));
  // 選択が変わったら、描く前に読み直す（前の選択の畳みで 1 回描かない）
  let current = state;
  if (state.scope !== scope) {
    current = { scope, fold: read(scope) };
    setState(current);
  }
  const update = useCallback((fn: (prev: FoldState) => FoldState) => {
    setState((prev) => {
      const fold = fn(prev.fold);
      write(prev.scope, fold);
      return { scope: prev.scope, fold };
    });
  }, []);
  return [current.fold, update];
}
