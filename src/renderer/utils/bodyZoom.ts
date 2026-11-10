// 本文の文字の拡大・縮小（docs/UI_UX_DESIGN.md §8）。
// 欄（作成の本文・作成画面の元メール・会話のバブル）ごとに倍率を持ち、ポインタが乗っている欄だけを
// Ctrl + "+" / "-" / "0" と Ctrl + ホイールで変える。ここは計算と保存だけ（DOM の操作はフック側）。

/** 倍率を持つ欄。 */
export type ZoomField = 'compose' | 'source' | 'bubble';

export const ZOOM_FIELDS: readonly ZoomField[] = ['compose', 'source', 'bubble'];

/** 欄に付ける印（`data-zoom-field="bubble"` など）。 */
export const ZOOM_FIELD_ATTR = 'data-zoom-field';

export const ZOOM_MIN = 0.7;
export const ZOOM_MAX = 2;
export const ZOOM_STEP = 0.1;

/** ホイールを何ピクセル分回したら 1 段とするか（トラックパッドの細かい delta を貯める）。 */
export const WHEEL_STEP_PX = 40;
/** この間隔（ミリ秒）より空いたら、貯めた delta を捨てる。 */
const WHEEL_IDLE_MS = 250;
/** deltaMode が行・ページのときのピクセル換算。 */
const LINE_PX = 16;
const PAGE_PX = 800;

/** 倍率を 10% 刻みに丸め、上下限に収める。数でない値は 100%。 */
export function clampZoom(v: number): number {
  if (!Number.isFinite(v)) return 1;
  const r = Math.round(v * 10) / 10;
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, r));
}

/** 1 段（+1 で拡大・-1 で縮小）動かした倍率。 */
export function stepZoom(v: number, dir: 1 | -1): number {
  return clampZoom(clampZoom(v) + dir * ZOOM_STEP);
}

/** 倍率の表示（「120%」）。 */
export function zoomLabel(v: number): string {
  return `${Math.round(clampZoom(v) * 100)}%`;
}

export type ZoomAction = 'in' | 'out' | 'reset';

/** キー操作の判定に要る欄だけ（KeyboardEvent をそのまま渡せる）。 */
export interface ZoomKey {
  key: string;
  code: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  isComposing?: boolean;
}

/**
 * キー操作を拡大・縮小・元に戻すに読み替える（該当しなければ null）。
 * Ctrl（mac は ⌘）＋ "+" / "=" / テンキー + で拡大、"-" / テンキー - で縮小、"0" で 100%。
 * JIS 配列の "+" は Shift + ";" なので、Chrome と同じく Ctrl + ";" も拡大に数える。
 */
export function keyZoomAction(e: ZoomKey): ZoomAction | null {
  if (!(e.ctrlKey || e.metaKey) || e.altKey || e.isComposing) return null;
  if (e.key === '+' || e.key === '=' || e.key === ';' || e.code === 'NumpadAdd') return 'in';
  if (e.key === '-' || e.key === '_' || e.code === 'NumpadSubtract') return 'out';
  if (e.key === '0' || e.code === 'Numpad0') return 'reset';
  return null;
}

/** 倍率に操作を当てた結果。 */
export function applyZoomAction(v: number, action: ZoomAction): number {
  if (action === 'reset') return 1;
  return stepZoom(v, action === 'in' ? 1 : -1);
}

/** ホイールの貯め（前回からの続き）。 */
export interface WheelAcc {
  acc: number;
  at: number;
}

export const WHEEL_ACC_INIT: WheelAcc = { acc: 0, at: 0 };

/**
 * Ctrl + ホイール 1 回分を、拡大（1）・縮小（-1）・まだ動かさない（0）に丸める。
 * 1 回のイベントで動かすのは高々 1 段。細かい delta（トラックパッド）は WHEEL_STEP_PX まで貯め、
 * 向きが変わったり間が空いたりしたら貯めを捨てる。上へ回す（deltaY < 0）と拡大。
 */
export function wheelStep(
  prev: WheelAcc,
  deltaY: number,
  deltaMode: number,
  now: number
): { step: -1 | 0 | 1; next: WheelAcc } {
  const px = deltaY * (deltaMode === 1 ? LINE_PX : deltaMode === 2 ? PAGE_PX : 1);
  const stale = now - prev.at > WHEEL_IDLE_MS || Math.sign(prev.acc) !== Math.sign(px);
  const acc = (stale ? 0 : prev.acc) + px;
  if (Math.abs(acc) < WHEEL_STEP_PX) return { step: 0, next: { acc, at: now } };
  return { step: acc < 0 ? 1 : -1, next: { acc: 0, at: now } };
}

const isZoomField = (s: string | null): s is ZoomField =>
  s !== null && (ZOOM_FIELDS as readonly string[]).includes(s);

/** その要素が載っている欄（欄の外なら null）。 */
export function fieldOf(el: Element | null): { field: ZoomField; el: Element } | null {
  const host = el?.closest(`[${ZOOM_FIELD_ATTR}]`) ?? null;
  const field = host?.getAttribute(ZOOM_FIELD_ATTR) ?? null;
  return host && isZoomField(field) ? { field, el: host } : null;
}

/** 欄の倍率を当てる CSS 変数の名前（ルート要素に置く）。 */
export const zoomVar = (field: ZoomField) => `--body-zoom-${field}`;

// 保存は見た目の設定（config/appearance.ts）と同じく localStorage。作成の別ウィンドウとも共有され、
// 片方で変えると `storage` イベントでもう片方が追随する。
const storageKey = (field: ZoomField) => `rondine.bodyZoom.${field}`;

/** 保存してある倍率（無ければ・壊れていれば 100%）。 */
export function readZoom(field: ZoomField): number {
  try {
    const s = localStorage.getItem(storageKey(field));
    return s === null ? 1 : clampZoom(Number(s));
  } catch {
    return 1;
  }
}

export function writeZoom(field: ZoomField, v: number): void {
  try {
    localStorage.setItem(storageKey(field), String(clampZoom(v)));
  } catch {
    // 保存できなくても、この画面の倍率は変わる（次の起動で 100% に戻るだけ）。
  }
}

/** localStorage のキーがどの欄の倍率か（別ウィンドウの変更を拾うのに使う）。 */
export function zoomFieldOfKey(key: string | null): ZoomField | null {
  return ZOOM_FIELDS.find((f) => storageKey(f) === key) ?? null;
}
