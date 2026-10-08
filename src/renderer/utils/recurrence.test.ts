import { describe, it, expect } from 'vitest';
import type { EventSummary } from '@bindings/EventSummary';
import { expandEvents } from './recurrence';

const base: EventSummary = {
  id: 1,
  title: '定例MTG',
  description: null,
  location: null,
  start_at: '2026-09-04T10:30',
  end_at: '2026-09-04T12:00',
  all_day: false,
  color: null,
  recurrence: 'FREQ=WEEKLY;WKST=MO',
  reminder_minutes: null,
  related_email_id: null,
  deleted_at: null,
  calendar_id: 6,
  availability: 'busy',
  visibility: 'default',
  original_start_at: null,
  exdates: [],
};

const starts = (rows: EventSummary[]) => rows.map((e) => e.start_at);

describe('expandEvents（繰り返しの例外）', () => {
  it('例外が無ければ毎週出る', () => {
    expect(starts(expandEvents([base], '2026-10-05', '2026-10-19'))).toEqual([
      '2026-10-09T10:30',
      '2026-10-16T10:30',
    ]);
  });

  it('1 回だけ動かした回は本体の分を出さず、例外の方を出す', () => {
    const master = { ...base, exdates: ['2026-10-09T10:30'] };
    const moved: EventSummary = {
      ...base,
      id: 2,
      recurrence: null,
      start_at: '2026-10-08T15:00',
      end_at: '2026-10-08T16:30',
      original_start_at: '2026-10-09T10:30',
    };
    expect(starts(expandEvents([master, moved], '2026-10-05', '2026-10-12'))).toEqual([
      '2026-10-08T15:00',
    ]);
  });

  it('1 回だけ削除した回は出さない（終日も日付で突き合わせる）', () => {
    const allDay = {
      ...base,
      all_day: true,
      start_at: '2026-09-04',
      end_at: null,
      exdates: ['2026-10-16'],
    };
    expect(starts(expandEvents([allDay], '2026-10-05', '2026-10-24'))).toEqual([
      '2026-10-09',
      '2026-10-23',
    ]);
  });
});
