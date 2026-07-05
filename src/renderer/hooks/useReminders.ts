import { useEffect, useRef } from 'react';
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
import { eventList } from '../services/calendar';
import { expandEvents } from '../utils/recurrence';
import type { EventSummary } from '@bindings/EventSummary';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const CHECK_MS = 30_000; // 30秒ごとに点検
const GRACE_MS = 90_000; // 発火の猶予（点検間隔＋α。これより古い期限は起動時に鳴らさない）

const pad = (n: number) => String(n).padStart(2, '0');
const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const addDays = (d: Date, n: number) => {
  const x = new Date(d);
  x.setDate(x.getDate() + n);
  return x;
};

/** 予定の開始 Date（終日は 0 時、時間指定はその時刻。いずれもローカル）。 */
function startDate(e: EventSummary): Date {
  return e.start_at.length > 10 ? new Date(e.start_at) : new Date(`${e.start_at}T00:00`);
}

/**
 * リマインダーのスケジューラ。アプリ起動中、リマインダー付き予定の期限が来たら
 * OS 通知を出す（繰り返しは展開して各出現を対象）。常駐ウィジェットとして開いている
 * 間だけ動作し、期限を過ぎた古い通知は起動時に蒸し返さない（GRACE_MS で足切り）。
 * View に依存しないよう App 直下でマウントする。
 */
export function useReminders() {
  const firedRef = useRef<Set<string>>(new Set());
  const grantedRef = useRef(false);

  useEffect(() => {
    if (!isTauri) return;
    let stopped = false;

    (async () => {
      try {
        grantedRef.current = await isPermissionGranted();
        if (!grantedRef.current) grantedRef.current = (await requestPermission()) === 'granted';
      } catch {
        grantedRef.current = false;
      }
    })();

    const tick = async () => {
      if (stopped || !grantedRef.current) return;
      const now = new Date();
      // 1日前リマインダー＋当日ぶんを拾うため、今日から2日先までを見る。
      const from = ymd(now);
      const to = ymd(addDays(now, 2));
      let rows: EventSummary[];
      try {
        rows = await eventList(from, to);
      } catch {
        return;
      }
      const occurrences = expandEvents(rows, from, to).filter((e) => e.reminder_minutes != null);
      for (const e of occurrences) {
        const fireAt = startDate(e).getTime() - (e.reminder_minutes as number) * 60_000;
        const delta = now.getTime() - fireAt;
        if (delta < 0 || delta >= GRACE_MS) continue; // まだ先 / 古すぎ
        const key = `${e.id}|${e.start_at}`;
        if (firedRef.current.has(key)) continue;
        firedRef.current.add(key);
        const time = e.all_day ? '' : e.start_at.slice(11, 16);
        const body = [time, e.location ?? ''].filter(Boolean).join(' · ');
        try {
          sendNotification(body ? { title: e.title, body } : { title: e.title });
        } catch {
          /* 通知失敗は無視（次の点検で dedup 済み） */
        }
      }
    };

    const id = setInterval(tick, CHECK_MS);
    tick();
    return () => {
      stopped = true;
      clearInterval(id);
    };
  }, []);
}
