import { useEffect, useRef, useState, type ReactNode } from 'react';

/** 表示までの待ち時間（ms）。押し間違いを防ぐ程度に短く、邪魔にならない程度に長く。 */
const SHOW_DELAY = 300;

/**
 * アイコンボタン用のツールチップ。
 *
 * native の `title` 属性は WebView 側の実装任せで、**下からカーソルを持っていくと出ない
 * ことがある**（利用者報告 2026-08-31）。表示条件をアプリ側に持てば、どの向きから乗っても
 * 同じように出る。文言は `aria-label` にも残すので、読み上げには影響しない。
 *
 * 親の `overflow` に切られないよう `position: fixed` で描く
 * （ContextMenu / DateFilter と同じ作法。ポータルは使わない）。
 */
export function Tooltip({ label, children }: { label: string; children: ReactNode }) {
  const ref = useRef<HTMLSpanElement>(null);
  const timer = useRef<number | undefined>(undefined);
  // 押した後、カーソルが乗ったままでも出し直さないための抑止。押すと何かが起きる
  // （メニューが開く等）ので、その上にツールチップが重なると邪魔になる。
  const suppressed = useRef(false);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  // 表示待ちの最中にアンマウントされてもタイマーを残さない。
  useEffect(() => () => window.clearTimeout(timer.current), []);

  const show = () => {
    if (suppressed.current) return;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      const el = ref.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      // 画面端で見切れないよう、中心を内側へ寄せる（tooltip 側は -translate-x-1/2）。
      const half = 80;
      const left = Math.min(Math.max(r.left + r.width / 2, half), window.innerWidth - half);
      setPos({ left, top: r.bottom + 6 });
    }, SHOW_DELAY);
  };

  const hide = () => {
    window.clearTimeout(timer.current);
    setPos(null);
  };

  /** 押したら消し、カーソルが離れるまで出し直さない。 */
  const hideUntilLeave = () => {
    suppressed.current = true;
    hide();
  };

  /** 離れたら抑止を解く（次に乗せたらまた出る）。 */
  const leave = () => {
    suppressed.current = false;
    hide();
  };

  return (
    <span
      ref={ref}
      className="inline-flex"
      onMouseEnter={show}
      onMouseLeave={leave}
      // キーボード操作でも出す（Tab で辿ったとき）。
      onFocusCapture={show}
      onBlurCapture={leave}
      // 押したら即座に消し、離れるまで出し直さない。
      onPointerDown={hideUntilLeave}
    >
      {children}
      {pos && (
        <span
          role="tooltip"
          style={{ left: pos.left, top: pos.top }}
          className="pointer-events-none fixed z-50 max-w-40 -translate-x-1/2 rounded border border-white/15 bg-neutral-900/95 px-2 py-1 text-xs text-white/90 shadow-xl backdrop-blur"
        >
          {label}
        </span>
      )}
    </span>
  );
}
