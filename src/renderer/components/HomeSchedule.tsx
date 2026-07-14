import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CalendarDays } from 'lucide-react';
import type { EventSummary } from '@bindings/EventSummary';
import { eventList } from '../services/calendar';
import { expandEvents } from '../utils/recurrence';
import { isHoliday } from '../utils/holidays';
import { CALENDAR_SYNCED_EVENT } from '../hooks/useAutoSync';
import { ymd, addDays, dayOf } from './CalendarView';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 未選択の予定色（CalendarView の既定と揃える）。 */
const DEFAULT_COLOR = '#64b5f6';
/** 表示範囲の選択肢（日数）。既定は本日（1 日）。 */
const RANGES: { days: number; key: string }[] = [
  { days: 1, key: 'home.range_today' },
  { days: 2, key: 'home.range_2' },
  { days: 3, key: 'home.range_3' },
  { days: 7, key: 'home.range_7' },
];
const RANGE_KEY = 'rondine.home.scheduleDays';

/** 時刻部分（HH:MM）を取り出す。日付のみなら空。 */
const timeOf = (iso: string) => (iso.length > 10 ? iso.slice(11, 16) : '');

/** 予定が日付 d（'YYYY-MM-DD'）に掛かるか（開始日〜終了日、終日は最終日を含む）。 */
function coversDay(e: EventSummary, d: string): boolean {
  const start = dayOf(e.start_at);
  const end = e.end_at ? dayOf(e.end_at) : start;
  return start <= d && d <= end;
}

/** その日の予定を、終日→開始時刻順に並べて返す。 */
function eventsOn(events: EventSummary[], d: string): EventSummary[] {
  return events
    .filter((e) => coversDay(e, d))
    .sort((a, b) => {
      if (a.all_day !== b.all_day) return a.all_day ? -1 : 1;
      return a.start_at.localeCompare(b.start_at);
    });
}

/** 日付の文字色（日曜/祝日=赤・土曜=青・平日=既定）。 */
function dayTone(ds: string): string {
  const dow = new Date(`${ds}T00:00`).getDay();
  if (dow === 0 || isHoliday(ds)) return 'text-red-300';
  if (dow === 6) return 'text-blue-300';
  return 'text-white/70';
}

/**
 * ホーム右カラム：メールアカウント下に出す「本日の日程」。
 * 本日 / 2日 / 3日 / 7日 で表示範囲を切り替え（既定は本日）、期間に掛かる予定を
 * カレンダーと同じ展開（繰り返し含む）で一覧する。クリックでカレンダー画面へ。
 */
export function HomeSchedule({ onOpenCalendar }: { onOpenCalendar?: () => void }) {
  const { t, i18n } = useTranslation();
  const [range, setRange] = useState<number>(() => {
    const s = Number(localStorage.getItem(RANGE_KEY));
    return s === 2 || s === 3 || s === 7 ? s : 1;
  });
  useEffect(() => {
    localStorage.setItem(RANGE_KEY, String(range));
  }, [range]);

  const [events, setEvents] = useState<EventSummary[]>([]);
  const todayStr = ymd(new Date());
  const tomorrowStr = ymd(addDays(new Date(`${todayStr}T00:00`), 1));

  // 期間 [今日, 今日+range) に掛かる予定を取得（繰り返しは出現へ展開）。
  const reload = useCallback(() => {
    if (!isTauri) {
      setEvents([]);
      return;
    }
    const now = new Date();
    const from = ymd(now);
    const to = ymd(addDays(now, range));
    eventList(from, to)
      .then((rows) => setEvents(expandEvents(rows, from, to)))
      .catch(() => setEvents([]));
  }, [range]);
  useEffect(() => {
    reload();
  }, [reload]);

  // バックグラウンド自動同期が Google 側の変更を取り込んだら再読み込みする。
  useEffect(() => {
    window.addEventListener(CALENDAR_SYNCED_EVENT, reload);
    return () => window.removeEventListener(CALENDAR_SYNCED_EVENT, reload);
  }, [reload]);

  // 期間内の各日を、予定のある日だけ抽出（本日のみのときは日ヘッダを出さない）。
  const groups = useMemo(() => {
    const start = new Date(`${todayStr}T00:00`);
    return Array.from({ length: range }, (_, i) => ymd(addDays(start, i)))
      .map((day) => ({ day, list: eventsOn(events, day) }))
      .filter((g) => g.list.length > 0);
  }, [events, range, todayStr]);

  const dayLabel = (ds: string): string => {
    if (ds === todayStr) return t('cal.today');
    if (ds === tomorrowStr) return t('home.tomorrow');
    return new Intl.DateTimeFormat(i18n.language, {
      month: 'short',
      day: 'numeric',
      weekday: 'short',
    }).format(new Date(`${ds}T00:00`));
  };

  return (
    <div className="shrink-0 border-t border-white/15 pt-3 drop-shadow">
      <div className="mb-2 flex items-center justify-between gap-2">
        <button
          onClick={onOpenCalendar}
          className="flex items-center gap-1.5 text-sm font-medium text-white/85 hover:text-white"
        >
          <CalendarDays size={14} className="text-white/70" />
          {t('home.scheduleTitle')}
        </button>
        <div className="flex items-center gap-0.5 rounded-lg bg-white/10 p-0.5 text-[11px]">
          {RANGES.map((r) => (
            <button
              key={r.days}
              onClick={() => setRange(r.days)}
              className={`rounded px-1.5 py-0.5 hover:bg-white/15 ${
                range === r.days ? 'bg-white/25 font-medium text-white' : 'text-white/70'
              }`}
            >
              {t(r.key)}
            </button>
          ))}
        </div>
      </div>

      {groups.length === 0 ? (
        <p className="text-xs text-white/45">{t('cal.noEvents')}</p>
      ) : (
        <div className="max-h-52 space-y-2.5 overflow-y-auto pr-1">
          {groups.map((g) => (
            <div key={g.day}>
              {range > 1 && (
                <div className={`mb-1 text-[11px] font-medium ${dayTone(g.day)}`}>
                  {dayLabel(g.day)}
                </div>
              )}
              <div className="space-y-1">
                {g.list.map((e) => (
                  <button
                    key={`${e.id}-${e.start_at}`}
                    onClick={onOpenCalendar}
                    className="flex w-full items-baseline gap-2 text-left text-white/85 hover:text-white"
                  >
                    <span className="w-14 shrink-0 whitespace-nowrap text-[11px] tabular-nums text-white/55">
                      {e.all_day ? t('cal.allDay') : timeOf(e.start_at)}
                    </span>
                    <span className="flex min-w-0 items-center gap-1.5">
                      <span
                        className="h-1.5 w-1.5 shrink-0 rounded-full"
                        style={{ backgroundColor: e.color ?? DEFAULT_COLOR }}
                      />
                      <span className="truncate text-sm">{e.title}</span>
                    </span>
                  </button>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
