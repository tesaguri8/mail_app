// ローカルでの変更の合図（変更したら即 Google へ送る。利用者の判断 2026-10-09）。
//
// 連絡先・組織を変える呼び出し（保存・削除・復元・取り込み・統合・一括統合・片付け・同期先の
// 変更・組織の変更）は、成功したらこの合図を出す。自動同期（hooks/useAutoSync）は合図を
// まとめ待ち（デバウンス）してから Google の同期を起こす。続けて何件も変えても 1 回で送る。

/** ローカルで連絡先などを変えた合図。 */
export const LOCAL_CHANGE_EVENT = 'rondine:local-change';

/** まとめ待ちの長さ（ミリ秒）。最後の変更からこれだけ待って同期を起こす。 */
export const LOCAL_CHANGE_DEBOUNCE_MS = 3000;

/** 合図を出す。 */
export function notifyLocalChange(): void {
  if (typeof window !== 'undefined') window.dispatchEvent(new Event(LOCAL_CHANGE_EVENT));
}

/** 呼び出しが成功したら合図を出して、結果をそのまま返す（失敗したら合図を出さない）。 */
export async function changed<T>(p: Promise<T>): Promise<T> {
  const r = await p;
  notifyLocalChange();
  return r;
}

/** まとめ待ち: `call` が続けて呼ばれても、最後の呼び出しから `ms` 後に `fn` を 1 回だけ呼ぶ。 */
export function debounce(fn: () => void, ms: number): { call: () => void; cancel: () => void } {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const cancel = () => {
    if (timer != null) clearTimeout(timer);
    timer = null;
  };
  return {
    call: () => {
      cancel();
      timer = setTimeout(() => {
        timer = null;
        fn();
      }, ms);
    },
    cancel,
  };
}
