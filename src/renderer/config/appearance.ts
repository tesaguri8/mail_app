import type { CSSProperties } from 'react';

// 画面の見た目（背景の濃さ・文字色・背景写真）の組み立て。メインのウィンドウと
// 作成の別ウィンドウで同じ見た目にするため、ここに集める（値は localStorage を共有する）。

/** 背景の濃さ・文字色スライダーの共通最大値（85%）。 */
export const BAR_MAX = 0.85;

const DIM_KEY = 'rondine.dim';
const INK_KEY = 'rondine.ink';

/** 背景のかぶせ（暗さ）の保存値。 */
export function readDim(): number {
  return Number(localStorage.getItem(DIM_KEY) ?? 0);
}

export function writeDim(dim: number): void {
  localStorage.setItem(DIM_KEY, String(dim));
}

/** 文字色（インク）の保存値。0=白, 0.85=黒。 */
export function readInk(): number {
  return Number(localStorage.getItem(INK_KEY) ?? 0);
}

export function writeInk(ink: number): void {
  localStorage.setItem(INK_KEY, String(ink));
}

/** 見た目の保存値のキーか（別ウィンドウで変更を拾うのに使う）。 */
export function isAppearanceKey(key: string | null): boolean {
  return key === DIM_KEY || key === INK_KEY;
}

/** 文字色（インク）の白→黒スライダー(0〜0.85)を実際の色に。0=白, 0.85=黒。
 * これを Tailwind の --color-white に流し込み、UI 全体を白⇄黒でトーン反転させる。 */
export function inkColor(ink: number): string {
  const l = Math.round(255 * (1 - Math.min(ink, BAR_MAX) / BAR_MAX));
  return `rgb(${l}, ${l}, ${l})`;
}

/** 背景（かぶせグラデ＋写真）。既定でも白文字が読める最低限の暗さを土台にし、dim でさらに濃くする。 */
export function backdropCss(dim: number, backgroundUrl: string): string {
  return `linear-gradient(160deg, rgba(10,14,28,${(0.35 + dim * 0.6).toFixed(2)}) 0%, rgba(6,9,20,${(0.55 + dim * 0.5).toFixed(2)}) 100%), url(${backgroundUrl})`;
}

/**
 * 各画面の面（パネル）に敷く土台。写真の上に文字が直接乗ると、明るい写真では沈むため、
 * 背景よりわずかに濃い面を敷いて内容を浮かせる。濃さはスライダー(dim)に追随させ、
 * 0% でも最低 5% は残す（利用者提案 2026-09-01「スライダー指定＋5%」）。
 */
export function panelTintCss(dim: number): string {
  return `rgba(6, 9, 20, ${Math.min(dim + 0.05, 1).toFixed(2)})`;
}

/** ウィンドウの土台（背景・CSS 変数）に当てる style。 */
export function appearanceStyle(backdrop: string, ink: number): CSSProperties {
  return {
    // 背景オーバーレイ: 既定でも白文字が読める最低限の暗さを土台にし、スライダー(dim)で
    // さらに濃くする。0% でも明るい写真＋白文字が真っ白にならないようにする。
    backgroundImage: backdrop,
    // 固定見出しラベルが同じ背景を貼れるよう、同じ文字列を配る（bg-fixed で位置を合わせる）。
    '--app-backdrop': backdrop,
    // 文字色スライダー: Tailwind の白を差し替えて UI 全体を白⇄黒でトーン反転。
    '--color-white': inkColor(ink),
  } as CSSProperties;
}
