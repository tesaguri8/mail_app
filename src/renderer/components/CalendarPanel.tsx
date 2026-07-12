import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import type { EventSummary } from '@bindings/EventSummary';
import type { CalendarSummary } from '@bindings/CalendarSummary';
import { calendarList, eventList } from '../services/calendar';
import { expandEvents } from '../utils/recurrence';
import { CALENDAR_SYNCED_EVENT } from '../hooks/useAutoSync';
import {
  MiniMonth,
  AgendaPanel,
  EventEditor,
  coveredDays,
  weekdayLabels,
  ymd,
  type EditTarget,
} from './CalendarView';

/** メール本文からカレンダーパネルを開くときのプレフィル。 */
export type CalendarPanelInitial = {
  /** 'YYYY-MM-DD'。 */
  day: string;
  /** 'HH:MM'。無ければ終日。 */
  time?: string;
  allDay?: boolean;
  /** 件名の初期値（メールの件名など）。 */
  title?: string;
  /** 作成元メールの id（emails.id）。 */
  relatedEmailId?: number;
};

/**
 * メール読書中に右ペインへ出すカレンダー入力パネル。
 * 上＝ミニ月カレンダー（前後移動・日選択）、下＝選択日のアジェンダ or 予定入力フォーム。
 * `initial` があればマウント時にその日時で新規入力フォームを開く（本文日付の＋から）。
 * 既存のカレンダー実装（MiniMonth / AgendaPanel / EventEditor）をそのまま再利用する。
 */
export function CalendarPanel({ initial }: { initial?: CalendarPanelInitial }) {
  const { i18n } = useTranslation();
  const locale = i18n.language;
  const todayStr = ymd(new Date());

  const [calendars, setCalendars] = useState<CalendarSummary[]>([]);
  const [events, setEvents] = useState<EventSummary[]>([]);
  const [selectedDay, setSelectedDay] = useState<string>(initial?.day ?? todayStr);
  const [anchor, setAnchor] = useState<Date>(() =>
    initial?.day ? new Date(`${initial.day}T00:00`) : new Date(),
  );
  const [editing, setEditing] = useState<EditTarget | null>(
    initial
      ? {
          mode: 'new',
          day: initial.day,
          time: initial.time,
          allDay: initial.allDay ?? !initial.time,
          title: initial.title,
          relatedEmailId: initial.relatedEmailId,
        }
      : null,
  );

  const year = anchor.getFullYear();
  const month = anchor.getMonth();
  const weekdayNarrow = useMemo(() => weekdayLabels(locale, 'narrow'), [locale]);

  useEffect(() => {
    calendarList()
      .then(setCalendars)
      .catch(() => undefined);
  }, []);

  // 表示中の月に掛かる予定を読み込む（繰り返しは occurrences へ展開）。
  const reload = useCallback(() => {
    const from = ymd(new Date(year, month, 1));
    const to = ymd(new Date(year, month + 1, 1));
    eventList(from, to)
      .then((rows) => setEvents(expandEvents(rows, from, to)))
      .catch(() => undefined);
  }, [year, month]);

  useEffect(() => {
    reload();
  }, [reload]);

  // バックグラウンド自動同期が Google 側の変更を取り込んだら、一覧と予定を再読み込みする。
  useEffect(() => {
    const onSynced = () => {
      calendarList().then(setCalendars).catch(() => undefined);
      reload();
    };
    window.addEventListener(CALENDAR_SYNCED_EVENT, onSynced);
    return () => window.removeEventListener(CALENDAR_SYNCED_EVENT, onSynced);
  }, [reload]);

  // パネルを開いたまま別の日付＋を押したとき（initial が差し替わる）に、その日時で開き直す。
  // initial の参照はパネルを開くたびに新しくなるので、開く操作ごとに一度だけ発火する。
  useEffect(() => {
    if (!initial) return;
    setSelectedDay(initial.day);
    setAnchor(new Date(`${initial.day}T00:00`));
    setEditing({
      mode: 'new',
      day: initial.day,
      time: initial.time,
      allDay: initial.allDay ?? !initial.time,
      title: initial.title,
      relatedEmailId: initial.relatedEmailId,
    });
  }, [initial]);

  const eventDays = useMemo(() => new Set(events.flatMap(coveredDays)), [events]);
  const dayEvents = useMemo(
    () =>
      events
        .filter((e) => coveredDays(e).includes(selectedDay))
        .sort((a, b) => a.start_at.localeCompare(b.start_at)),
    [events, selectedDay],
  );

  const newOn = (day: string) => setEditing({ mode: 'new', day, allDay: false });

  const afterSave = () => {
    reload();
    setEditing(null);
  };

  return (
    <div className="flex h-full min-h-0 flex-col gap-2 p-2">
      {/* 月ナビ + ミニ月カレンダー */}
      <div className="shrink-0">
        <div className="mb-1 flex items-center justify-between px-1">
          <button
            onClick={() => setAnchor(new Date(year, month - 1, 1))}
            className="rounded p-1 text-white/60 hover:bg-white/10 hover:text-white"
            aria-label="prev-month"
          >
            <ChevronLeft size={16} />
          </button>
          <span className="text-sm font-medium">
            {new Intl.DateTimeFormat(locale, { year: 'numeric', month: 'long' }).format(
              new Date(year, month, 1),
            )}
          </span>
          <button
            onClick={() => setAnchor(new Date(year, month + 1, 1))}
            className="rounded p-1 text-white/60 hover:bg-white/10 hover:text-white"
            aria-label="next-month"
          >
            <ChevronRight size={16} />
          </button>
        </div>
        <MiniMonth
          year={year}
          month={month}
          weekdayNarrow={weekdayNarrow}
          eventDays={eventDays}
          selected={selectedDay}
          todayStr={todayStr}
          locale={locale}
          onSelectDay={(ds) => setSelectedDay(ds)}
          onOpenDay={(ds) => {
            setSelectedDay(ds);
            newOn(ds);
          }}
          onOpenMonth={() => undefined}
        />
      </div>

      {/* 下段: 入力フォーム（編集中）または選択日のアジェンダ */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {editing ? (
          <EventEditor
            target={editing}
            calendars={calendars}
            onClose={() => setEditing(null)}
            onSaved={afterSave}
            onDeleted={afterSave}
          />
        ) : (
          <AgendaPanel
            selected={selectedDay}
            list={dayEvents}
            locale={locale}
            showHolidays
            onOpen={(e) => setEditing({ mode: 'edit', event: e })}
            onNew={() => newOn(selectedDay)}
            className="h-full w-full"
          />
        )}
      </div>
    </div>
  );
}
