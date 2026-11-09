import { describe, expect, it } from 'vitest';
import type { EventSummary } from '@bindings/EventSummary';
import { scheduleGroups } from './scheduleGroups';

const TODAY = '2026-11-10';

/** 試験用の予定（必要な欄だけ変える）。 */
function ev(id: number, start_at: string, extra: Partial<EventSummary> = {}): EventSummary {
  return {
    id,
    title: `予定${id}`,
    description: null,
    location: null,
    start_at,
    end_at: null,
    all_day: false,
    color: null,
    recurrence: null,
    reminder_minutes: null,
    related_email_id: null,
    deleted_at: null,
    calendar_id: null,
    availability: 'busy',
    visibility: 'default',
    original_start_at: null,
    exdates: [],
    ...extra,
  };
}

const shape = (g: ReturnType<typeof scheduleGroups>) =>
  g.map(({ day, list }) => [day, list.map((e) => e.id)]);

describe('scheduleGroups（ホームの本日の日程）', () => {
  it('今日が空で明日に予定があっても、今日を空のまま先頭に置く', () => {
    const events = [ev(1, '2026-11-11T09:00'), ev(2, '2026-11-12', { all_day: true })];
    expect(shape(scheduleGroups(events, TODAY, 3))).toEqual([
      [TODAY, []],
      ['2026-11-11', [1]],
      ['2026-11-12', [2]],
    ]);
  });

  it('期間全体に予定が無ければ、今日の空の 1 日だけ', () => {
    expect(shape(scheduleGroups([], TODAY, 7))).toEqual([[TODAY, []]]);
  });

  it('本日のみ（range=1）でも、空なら今日の空の 1 日', () => {
    expect(shape(scheduleGroups([ev(1, '2026-11-11T09:00')], TODAY, 1))).toEqual([[TODAY, []]]);
  });

  it('明日以降の空の日は出さず、今日は終日→時刻順に並べる', () => {
    const events = [
      ev(1, '2026-11-10T15:00'),
      ev(2, '2026-11-10T09:00'),
      ev(3, '2026-11-10', { all_day: true }),
      ev(4, '2026-11-12T10:00'),
    ];
    expect(shape(scheduleGroups(events, TODAY, 3))).toEqual([
      [TODAY, [3, 2, 1]],
      ['2026-11-12', [4]],
    ]);
  });

  it('前日から続く予定は今日に掛かる', () => {
    const events = [ev(1, '2026-11-09T22:00', { end_at: '2026-11-10T02:00' })];
    expect(shape(scheduleGroups(events, TODAY, 2))).toEqual([[TODAY, [1]]]);
  });
});
