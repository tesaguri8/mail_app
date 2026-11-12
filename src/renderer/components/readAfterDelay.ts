import { useCallback, useEffect, useMemo, useRef } from 'react';
import type { ThreadListItem } from '@bindings/ThreadListItem';

/**
 * 未読のスレッドを開いてから既読にするまでの時間（ミリ秒）。見ている間だけ数える
 * （Outlook の「N 秒表示したら既読」と同じ。ちらっと見ただけのものは既読にしない）。
 */
export const READ_AFTER_MS = 5_000;

/** 開いたスレッドを、決めた時間だけ見たら既読にする時計。 */
export interface ReadTimer {
  /** 数え始める（前の予約は捨てる）。見ていない間は数えない。 */
  start(): void;
  /** 予約を捨てる（まだ時間が来ていなければ、既読にしない）。 */
  cancel(): void;
  /** 見ているか（窓が表に出ていてフォーカスがあり、メールのタブが表示中か）が変わった。 */
  setAttentive(attentive: boolean): void;
  /** 時間が来るのを待っているか。 */
  readonly pending: boolean;
}

/**
 * 見ている間だけ進む時計を作る。止めている間の経過は数えず、戻ったら残りから続ける
 * （裏に回すたびに最初からにはしない）。
 *
 * @param onElapsed 見ていた時間の合計が `delayMs` に達したときに 1 回だけ呼ぶ
 * @param delayMs 既読にするまでの時間（既定は {@link READ_AFTER_MS}）
 * @param attentive 作った時点で見ているか
 */
export function createReadTimer(
  onElapsed: () => void,
  delayMs: number = READ_AFTER_MS,
  attentive = true
): ReadTimer {
  let watching = attentive;
  // 残り時間（null なら待っていない）。
  let remaining: number | null = null;
  let startedAt = 0;
  let handle: ReturnType<typeof setTimeout> | null = null;

  const run = () => {
    if (remaining == null || handle != null) return;
    startedAt = Date.now();
    handle = setTimeout(() => {
      handle = null;
      remaining = null;
      onElapsed();
    }, remaining);
  };
  const pause = () => {
    if (handle == null || remaining == null) return;
    clearTimeout(handle);
    handle = null;
    remaining = Math.max(0, remaining - (Date.now() - startedAt));
  };
  const cancel = () => {
    if (handle != null) clearTimeout(handle);
    handle = null;
    remaining = null;
  };

  return {
    start() {
      cancel();
      remaining = delayMs;
      if (watching) run();
    },
    cancel,
    setAttentive(next) {
      if (next === watching) return;
      watching = next;
      if (next) run();
      else pause();
    },
    get pending() {
      return remaining != null;
    },
  };
}

/** いま利用者がこの窓を見ているか（表に出ていて、フォーカスがある）。 */
export function windowAttentive(doc: Document = document): boolean {
  return doc.visibilityState === 'visible' && doc.hasFocus();
}

/**
 * {@link createReadTimer} を部品で使う。窓が裏に回る・最小化される・フォーカスが外れる・
 * メールのタブが隠れる間は時計を止める。
 *
 * @param onElapsed 時間が来たときに呼ぶ（最新の関数を呼ぶので、毎回作り直してよい）
 * @param tabActive メールのタブが表示中か
 * @returns 数え始める `start` と、予約を捨てる `cancel`（どちらも作り直さない）
 */
export function useReadAfterDelay(
  onElapsed: () => void,
  tabActive: boolean
): Pick<ReadTimer, 'start' | 'cancel'> {
  const cb = useRef(onElapsed);
  cb.current = onElapsed;
  const tabRef = useRef(tabActive);
  tabRef.current = tabActive;
  const timer = useMemo(
    () => createReadTimer(() => cb.current(), READ_AFTER_MS, tabActive && windowAttentive()),
    // 時計は 1 つだけ（見ているかは下の effect が伝える）。
    // eslint-disable-next-line react-hooks/exhaustive-deps
    []
  );
  const update = useCallback(
    () => timer.setAttentive(tabRef.current && windowAttentive()),
    [timer]
  );
  useEffect(update, [tabActive, update]);
  useEffect(() => {
    document.addEventListener('visibilitychange', update);
    window.addEventListener('focus', update);
    window.addEventListener('blur', update);
    return () => {
      document.removeEventListener('visibilitychange', update);
      window.removeEventListener('focus', update);
      window.removeEventListener('blur', update);
      timer.cancel();
    };
  }, [timer, update]);
  return useMemo(() => ({ start: () => timer.start(), cancel: () => timer.cancel() }), [timer]);
}

/** 開いている未読スレッドの予約（`read` は時間が来て既読にしたか）。 */
export interface ActiveRead {
  rowId: number;
  ids: number[];
  read: boolean;
}

/**
 * 絞り込みの述語を、読書中の行には「未読のまま」として当てる。時間が来て既読にしても、
 * 離れるまでは未読の絞り込みから消さない（離れたら普段の述語に戻り、そこで消える）。
 *
 * @param passes 普段の絞り込みの述語
 * @param active 開いている未読スレッドの予約（無ければ `passes` のまま）
 */
export function passesWhileReading(
  passes: (m: ThreadListItem) => boolean,
  active: ActiveRead | null
): (m: ThreadListItem) => boolean {
  if (!active?.read) return passes;
  return (m) =>
    m.id === active.rowId
      ? passes({ ...m, is_read: false, unread_count: Math.max(1, m.unread_count) })
      : passes(m);
}
