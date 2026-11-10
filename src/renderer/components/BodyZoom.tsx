import type { CSSProperties, ReactNode } from 'react';
import { useBodyZoom } from '../hooks/useBodyZoom';
import { ZOOM_FIELD_ATTR, zoomVar, type ZoomField } from '../utils/bodyZoom';

/**
 * 本文の拡大・縮小を受け、変えた直後に欄の隅へ倍率を出す（ウィンドウの土台に 1 つだけ置く）。
 */
export function BodyZoomLayer() {
  const flash = useBodyZoom();
  if (!flash) return null;
  return (
    <div
      key={flash.seq}
      className="pointer-events-none fixed z-50 rounded-md bg-neutral-900/85 px-2 py-0.5 text-xs font-medium tabular-nums text-white/90 shadow-lg"
      style={{ top: flash.top, right: flash.right }}
      aria-live="polite"
    >
      {flash.label}
    </div>
  );
}

/** 作成の本文欄（textarea）に付ける印と文字の大きさ。基準は text-sm（0.875rem）。 */
export const composeZoomProps = {
  [ZOOM_FIELD_ATTR]: 'compose',
  style: { fontSize: `calc(0.875rem * var(${zoomVar('compose')}, 1))` } as CSSProperties,
};

/**
 * 読む本文（元メール・会話のバブル）の欄。中身を CSS の `zoom` で拡大する。
 * font-size の倍率では HTML メールの固定 px 指定（`font-size:12px` や表の幅）が変わらないため、
 * 字も行間も字下げも画像も同じ比率で大きくなる `zoom` を使う。倍率の位置合わせ
 * （getBoundingClientRect）が狂わないよう、印は外側に付け、`zoom` は内側だけに当てる。
 */
export function ZoomArea({
  field,
  className,
  children,
}: {
  field: Exclude<ZoomField, 'compose'>;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div {...{ [ZOOM_FIELD_ATTR]: field }} className={className}>
      <div style={{ zoom: `var(${zoomVar(field)}, 1)` }}>{children}</div>
    </div>
  );
}
