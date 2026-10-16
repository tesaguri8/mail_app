import { createContext, useContext, useEffect, useRef, type MutableRefObject } from 'react';

/**
 * 上部のタブ（ホーム／メール／連絡先／カレンダー／設定）のうち、この部品が載っている画面が
 * いま表示中か。画面は一度開いたら捨てずに隠しておくため（`KeepAlivePanel`）、隠れている画面も
 * 動き続ける。全体のキー操作・タイマーはこれを見て「表示中のときだけ」効かせる。
 *
 * タブの外（タイトルバー等）では常に true。
 */
export const TabActiveContext = createContext(true);

/** この部品が載っているタブが表示中か。 */
export function useTabActive(): boolean {
  return useContext(TabActiveContext);
}

/**
 * 表示中かを ref で返す。`window` のキー操作のように、登録し直さずに最新の値を見たい
 * ハンドラ向け（ハンドラの先頭で `if (!activeRef.current) return;` とする）。
 */
export function useTabActiveRef(): MutableRefObject<boolean> {
  const active = useTabActive();
  const ref = useRef(active);
  ref.current = active;
  return ref;
}

/**
 * 隠れていたタブが表示に戻ったときに `onActivated` を呼ぶ（初めて描いたときは呼ばない）。
 * 開いたときにだけ読んでいたデータを、戻ったときに読み直すのに使う。
 */
export function useOnTabActivated(onActivated: () => void): void {
  const active = useTabActive();
  const wasActive = useRef(active);
  const cb = useRef(onActivated);
  cb.current = onActivated;
  useEffect(() => {
    if (active && !wasActive.current) cb.current();
    wasActive.current = active;
  }, [active]);
}
