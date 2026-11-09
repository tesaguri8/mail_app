import type { EventSummary } from '@bindings/EventSummary';
import { ymd, addDays, dayOf } from './CalendarView';

/** ホームの「本日の日程」の 1 日分（予定が無ければ list は空）。 */
export interface ScheduleGroup {
  day: string;
  list: EventSummary[];
}

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

/**
 * 期間 [today, today+range) を日ごとに分ける。
 * 今日は予定が無くても必ず先頭に置く（空なら「なし」と出し、明日の予定を今日のものと
 * 見誤らせない）。明日以降は予定のある日だけ。
 */
export function scheduleGroups(
  events: EventSummary[],
  today: string,
  range: number
): ScheduleGroup[] {
  const start = new Date(`${today}T00:00`);
  return Array.from({ length: range }, (_, i) => ymd(addDays(start, i)))
    .map((day) => ({ day, list: eventsOn(events, day) }))
    .filter((g) => g.day === today || g.list.length > 0);
}
