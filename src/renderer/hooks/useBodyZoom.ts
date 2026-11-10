import { useEffect, useLayoutEffect, useState } from 'react';
import {
  WHEEL_ACC_INIT,
  ZOOM_FIELDS,
  applyZoomAction,
  fieldOf,
  keyZoomAction,
  readZoom,
  wheelStep,
  writeZoom,
  zoomFieldOfKey,
  zoomLabel,
  zoomVar,
  type ZoomAction,
  type ZoomField,
} from '../utils/bodyZoom';

/** 倍率を変えた瞬間に欄の隅へ出す表示（「120%」）。 */
export interface ZoomFlash {
  label: string;
  /** ビューポート基準の位置（欄の右上。画面外に出ないよう寄せたもの）。 */
  top: number;
  right: number;
  /** 同じ倍率を続けて出したときも出し直すための通し番号。 */
  seq: number;
}

/** 表示を消すまでの時間（ミリ秒）。 */
const FLASH_MS = 900;
const EDGE = 6;

/** 欄の倍率を CSS 変数としてルート要素に置く（欄はこの変数で文字を拡大する）。 */
function applyVar(field: ZoomField, v: number) {
  document.documentElement.style.setProperty(zoomVar(field), String(v));
}

/**
 * 本文の拡大・縮小のキーとホイールを 1 か所で受ける（ウィンドウごとに 1 回だけ使う）。
 * ポインタが乗っている欄（`data-zoom-field`）の倍率だけを変える。欄の外でも Ctrl + "+" 等と
 * Ctrl + ホイールは打ち消し、WebView の画面全体のズームは起こさない。
 * 保存した倍率を起動時に当て、別ウィンドウで変わったら（`storage`）追随する。
 *
 * @returns 変えた直後だけ出す倍率の表示（無ければ null）
 */
export function useBodyZoom(): ZoomFlash | null {
  const [flash, setFlash] = useState<ZoomFlash | null>(null);

  useLayoutEffect(() => {
    ZOOM_FIELDS.forEach((f) => applyVar(f, readZoom(f)));
  }, []);

  useEffect(() => {
    // キーで変えるときの「ポインタが乗っている欄」は、最後のポインタ位置から引く。
    let px = -1;
    let py = -1;
    let wheel = WHEEL_ACC_INIT;
    let seq = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const change = (field: ZoomField, el: Element, action: ZoomAction) => {
      const v = applyZoomAction(readZoom(field), action);
      writeZoom(field, v);
      applyVar(field, v);
      const r = el.getBoundingClientRect();
      seq += 1;
      setFlash({
        label: zoomLabel(v),
        top: Math.min(Math.max(r.top, 0) + EDGE, window.innerHeight - 28),
        right: Math.max(window.innerWidth - Math.min(r.right, window.innerWidth), 0) + EDGE,
        seq,
      });
      clearTimeout(timer);
      timer = setTimeout(() => setFlash(null), FLASH_MS);
    };

    const onPointer = (e: PointerEvent) => {
      px = e.clientX;
      py = e.clientY;
    };
    const onLeave = () => {
      px = -1;
      py = -1;
    };
    const onKey = (e: KeyboardEvent) => {
      const action = keyZoomAction(e);
      if (!action) return;
      e.preventDefault();
      const hit = px < 0 ? null : fieldOf(document.elementFromPoint(px, py));
      if (hit) change(hit.field, hit.el, action);
    };
    const onWheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      e.preventDefault();
      const hit = fieldOf(e.target instanceof Element ? e.target : null);
      if (!hit) return;
      const { step, next } = wheelStep(wheel, e.deltaY, e.deltaMode, e.timeStamp);
      wheel = next;
      if (step !== 0) change(hit.field, hit.el, step > 0 ? 'in' : 'out');
    };
    const onStorage = (e: StorageEvent) => {
      const field = zoomFieldOfKey(e.key);
      if (field) applyVar(field, readZoom(field));
    };

    window.addEventListener('pointermove', onPointer, { passive: true });
    document.documentElement.addEventListener('mouseleave', onLeave);
    // 欄の中の部品がキーを止めても受けられるよう、捕捉段階で受ける。
    window.addEventListener('keydown', onKey, true);
    // preventDefault するので passive にしない。
    window.addEventListener('wheel', onWheel, { capture: true, passive: false });
    window.addEventListener('storage', onStorage);
    return () => {
      clearTimeout(timer);
      window.removeEventListener('pointermove', onPointer);
      document.documentElement.removeEventListener('mouseleave', onLeave);
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('wheel', onWheel, { capture: true });
      window.removeEventListener('storage', onStorage);
    };
  }, []);

  return flash;
}
