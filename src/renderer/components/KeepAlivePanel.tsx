import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { TabActiveContext } from '../hooks/useTabActive';

/**
 * 上部のタブの 1 画面分の入れ物。一度開いた画面は捨てずに隠しておき、戻ったときに
 * 最後の作業状態（開いていたメール・検索語・スクロール位置・書きかけの入力）のまま見せる。
 *
 * - 初めて表示されるまでは描かない（起動時に全画面を読み込まない）
 * - 隠すのは `visibility: hidden`（`display: none` にすると大きさが 0 になり、一覧の
 *   スクロール位置や会話のつまみの測り直しが崩れるため。docs/UI_UX_DESIGN.md「タブの作業状態」）
 * - 隠れている間は `inert` にして、フォーカス・クリック・読み上げの対象から外す
 * - 中の部品には `TabActiveContext` で表示中かを配る（全体のキー操作・タイマーを止める）
 * - 戻ったときは、隠す前にフォーカスしていた欄へフォーカスを戻す（書きかけの続きをすぐ打てる）
 */
export function KeepAlivePanel({ active, children }: { active: boolean; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [seen, setSeen] = useState(active);
  // 描画中の setState は、初めて表示されたこの 1 回だけ（React の「前回の props から状態を
  // 導く」書き方）。
  if (active && !seen) setSeen(true);
  // この画面の中で最後にフォーカスした要素（戻ったときに戻す）。
  const lastFocus = useRef<HTMLElement | null>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.toggleAttribute('inert', !active);
    if (!active) return;
    const f = lastFocus.current;
    if (f && f.isConnected && el.contains(f)) f.focus({ preventScroll: true });
  }, [active, seen]);

  if (!seen) return null;
  return (
    <div
      ref={ref}
      className="absolute inset-0"
      style={active ? undefined : { visibility: 'hidden' }}
      aria-hidden={active ? undefined : true}
      onFocus={(e) => {
        if (e.target instanceof HTMLElement) lastFocus.current = e.target;
      }}
    >
      <TabActiveContext.Provider value={active}>{children}</TabActiveContext.Provider>
    </div>
  );
}
