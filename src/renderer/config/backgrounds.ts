// 背景写真プール（docs/UI_UX_DESIGN.md 背景写真システム）。
// アプリ同梱のサンプルを Vite の glob で読み込む。将来はユーザー取り込み画像もここへ合流。
const modules = import.meta.glob('../assets/backgrounds/*.{jpg,jpeg,png,svg,webp}', {
  eager: true,
  query: '?url',
  import: 'default',
}) as Record<string, string>;

/** 候補の並び（ファイル名昇順で安定）。 */
export const BACKGROUNDS: string[] = Object.keys(modules)
  .sort()
  .map((k) => modules[k]);

const KEY = 'rondine.bgIndex';

/**
 * 現在の背景インデックス。ユーザーがボタンで選んだ値があればそれ、無ければ日替わり
 * （1日ごとに別の1枚を自動配置）。将来は設定の切替タイミングで自動選択する。
 */
export function getBackgroundIndex(): number {
  if (BACKGROUNDS.length === 0) return 0;
  const saved = localStorage.getItem(KEY);
  if (saved != null && saved !== '') {
    const n = Number(saved);
    if (Number.isInteger(n) && n >= 0 && n < BACKGROUNDS.length) return n;
  }
  // 日替わり: 経過日数 mod 枚数。手動で選ぶと以降はその値を優先する。
  const day = Math.floor(Date.now() / 86_400_000);
  return day % BACKGROUNDS.length;
}

/** ユーザーが選んだ背景を保存する（以降はこれを優先＝自動配置より手動選択を優先）。 */
export function setBackgroundIndex(i: number): void {
  localStorage.setItem(KEY, String(i));
}
