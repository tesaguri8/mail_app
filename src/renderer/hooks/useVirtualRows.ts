import { useCallback, useEffect, useState } from 'react';

/** 見えている範囲の前後に余分に描く行数（速いスクロールで空白が見えないように）。 */
const OVERSCAN = 8;

export interface VirtualRows<T extends HTMLElement> {
  /** スクロールする要素に付ける ref（要素が作り直されても追随するようコールバック ref）。 */
  ref: (el: T | null) => void;
  /** スクロールする要素の onScroll に渡す。 */
  onScroll: () => void;
  /** 描く行の範囲 [start, end)。 */
  start: number;
  end: number;
  /** 描かない行の高さの合計（上・下の詰め物に使う）。 */
  padTop: number;
  padBottom: number;
}

/**
 * 行の高さが一定の長いリストを、見えている行だけ描く（仮想スクロール）。
 *
 * 連絡先の一覧のように数千行あるリストを全部描くと、レイアウトと描画だけで 1 秒を超える。
 * 行の高さを固定にすれば、スクロール位置と窓の高さから描く範囲が計算だけで決まるので、
 * 依存を足さずに済む（高さが行ごとに変わるリストには使えない）。
 */
export function useVirtualRows<T extends HTMLElement>(count: number, rowHeight: number): VirtualRows<T> {
  const [el, setEl] = useState<T | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewport, setViewport] = useState(0);

  const onScroll = useCallback(() => {
    if (el) setScrollTop(el.scrollTop);
  }, [el]);

  // 窓の高さ（リサイズ・パネルの開閉）に追随する。要素が替わったら位置も読み直す。
  useEffect(() => {
    if (!el) return;
    const ro = new ResizeObserver(() => setViewport(el.clientHeight));
    ro.observe(el);
    setViewport(el.clientHeight);
    setScrollTop(el.scrollTop);
    return () => ro.disconnect();
  }, [el]);

  const first = Math.floor(scrollTop / rowHeight);
  const start = Math.max(0, Math.min(count, first - OVERSCAN));
  const end = Math.min(count, first + Math.ceil(viewport / rowHeight) + OVERSCAN);
  return {
    ref: setEl,
    onScroll,
    start,
    end,
    padTop: start * rowHeight,
    padBottom: Math.max(0, count - end) * rowHeight,
  };
}
