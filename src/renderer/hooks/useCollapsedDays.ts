import { useCallback, useState } from 'react';

/**
 * メール一覧で畳んだ日（`YYYY-MM-DD` の集合）を、選択（アカウント・フォルダ）ごとに覚える。
 * 置き場は localStorage（この端末の表示の好みなので）。読めない・書けないとき（private window・
 * 消された・塞がれた）は、全部開いた状態で動く。設計: docs/MAIL_LIST_TIMELINE.md §6
 */
const KEY_PREFIX = 'rondine.mailListCollapsed';

/** 覚える日の上限（新しい日から）。何年分を畳んでも保存が膨らみすぎないように。 */
const MAX_DAYS = 4000;

function storageKey(scope: string): string {
  return `${KEY_PREFIX}.${scope}`;
}

function read(scope: string | null): Set<string> {
  if (scope == null) return new Set();
  try {
    const raw = localStorage.getItem(storageKey(scope));
    if (!raw) return new Set();
    const v: unknown = JSON.parse(raw);
    return Array.isArray(v)
      ? new Set(v.filter((x): x is string => typeof x === 'string'))
      : new Set();
  } catch {
    return new Set();
  }
}

function write(scope: string | null, days: ReadonlySet<string>): void {
  if (scope == null) return;
  try {
    if (days.size === 0) localStorage.removeItem(storageKey(scope));
    else {
      // 日付の文字列は辞書順＝日付順。新しい日を残す
      const keep = [...days].sort().reverse().slice(0, MAX_DAYS);
      localStorage.setItem(storageKey(scope), JSON.stringify(keep));
    }
  } catch {
    // 書けなくても、この画面の間は畳んだまま（次に開いたときは全部開く）
  }
}

/**
 * scope（例: `"3.inbox"`。null なら覚えない）の畳んだ日と、それを変える関数。
 * scope が変わったら、その選択の覚えを読み直す。
 */
export function useCollapsedDays(
  scope: string | null
): [ReadonlySet<string>, (fn: (prev: ReadonlySet<string>) => Set<string>) => void] {
  const [state, setState] = useState(() => ({ scope, days: read(scope) as ReadonlySet<string> }));
  // 選択が変わったら、描く前に読み直す（前の選択の畳みで 1 回描かない）
  let current = state;
  if (state.scope !== scope) {
    current = { scope, days: read(scope) };
    setState(current);
  }
  const update = useCallback((fn: (prev: ReadonlySet<string>) => Set<string>) => {
    setState((prev) => {
      const days = fn(prev.days);
      write(prev.scope, days);
      return { scope: prev.scope, days };
    });
  }, []);
  return [current.days, update];
}
