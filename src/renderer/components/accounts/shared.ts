// 設定の「アカウント」の部品で共有する小物（docs/ACCOUNTS.md）。

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const inputCls =
  'w-full rounded-md bg-white/10 px-3 py-2 text-sm text-white placeholder-white/40 outline-none focus:bg-white/20';
export const btnCls =
  'rounded-md bg-white/15 px-3 py-2 text-sm hover:bg-white/25 disabled:opacity-40';

/** SQLite の CURRENT_TIMESTAMP（'YYYY-MM-DD HH:MM:SS'・UTC）を手元の時刻表記へ。 */
export const localTime = (utc: string) => new Date(utc.replace(' ', 'T') + 'Z').toLocaleString();

/** 大文字小文字を無視してアドレスを比べる。 */
export const sameAddress = (a: string, b: string) =>
  a.trim().toLowerCase() === b.trim().toLowerCase();

/** メールアカウントの接続の確かめ（接続の点の色）。 */
export type ConnState = { state: 'checking' | 'ok' | 'error'; msg?: string };
