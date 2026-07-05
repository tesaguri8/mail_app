import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronLeft, ChevronRight, Plus, Trash2, X, Clock, MapPin, RotateCcw } from 'lucide-react';
import type { EventSummary } from '@bindings/EventSummary';
import type { EventInput } from '@bindings/EventInput';
import { eventList, eventListTrashed, eventUpsert, eventDelete, eventRestore } from '../services/calendar';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 予定の色パレット（未選択＝既定の青）。CSS 色をそのまま color 列に保存する。 */
const COLORS = ['#64b5f6', '#e57373', '#f6bf50', '#81c784', '#ba91e0', '#4db6ac', '#f06292'];
const DEFAULT_COLOR = COLORS[0];

// ── 日付ヘルパー（保存は端末ローカルの素の ISO 文字列。UTC 変換はしない） ──
const pad = (n: number) => String(n).padStart(2, '0');
/** Date → 'YYYY-MM-DD'（ローカル）。 */
const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
/** 'YYYY-MM-DD' の n 日後（ローカル）。 */
const addDays = (date: Date, n: number) => {
  const d = new Date(date);
  d.setDate(d.getDate() + n);
  return d;
};
/** 日付部分（先頭10文字）を取り出す。'2026-07-06T10:00' → '2026-07-06'。 */
const dayOf = (iso: string) => iso.slice(0, 10);
/** 時刻部分（HH:MM）を取り出す。日付のみなら空。 */
const timeOf = (iso: string) => (iso.length > 10 ? iso.slice(11, 16) : '');

/** 予定が日付 d（'YYYY-MM-DD'）に掛かるか。開始日〜終了日（終日は最終日を含む）で判定。 */
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

export function CalendarView() {
  const { t, i18n } = useTranslation();
  // 表示中の月（その月の1日を基準に保持）。
  const [cursor, setCursor] = useState<Date>(() => {
    const now = new Date();
    return new Date(now.getFullYear(), now.getMonth(), 1);
  });
  const [selected, setSelected] = useState<string>(() => ymd(new Date()));
  const [events, setEvents] = useState<EventSummary[]>([]);
  const [trashed, setTrashed] = useState<EventSummary[]>([]);
  const [showTrash, setShowTrash] = useState(false);
  // 編集/新規モーダル。null=閉じている。'new' は新規、それ以外は編集対象の予定。
  const [editing, setEditing] = useState<EventSummary | 'new' | null>(null);
  const todayStr = ymd(new Date());

  // 月グリッドは日曜始まりの6週(42日)。範囲 [gridStart, gridEnd) を DB から引く。
  const grid = useMemo(() => {
    const first = new Date(cursor.getFullYear(), cursor.getMonth(), 1);
    const gridStart = addDays(first, -first.getDay()); // その週の日曜まで戻す
    return Array.from({ length: 42 }, (_, i) => addDays(gridStart, i));
  }, [cursor]);

  const reload = useCallback(() => {
    if (!isTauri) return;
    if (showTrash) {
      eventListTrashed().then(setTrashed).catch(() => setTrashed([]));
      return;
    }
    const from = ymd(grid[0]);
    const to = ymd(addDays(grid[41], 1)); // 末尾セルの翌日（[from,to) の to は排他）
    eventList(from, to).then(setEvents).catch(() => setEvents([]));
  }, [grid, showTrash]);
  useEffect(reload, [reload]);

  const monthLabel = new Intl.DateTimeFormat(i18n.language, {
    year: 'numeric',
    month: 'long',
  }).format(cursor);

  // 曜日見出しは Intl から動的生成（既知の日曜=2024-01-07 を起点に7日）。
  const weekdayLabels = useMemo(() => {
    const fmt = new Intl.DateTimeFormat(i18n.language, { weekday: 'short' });
    return Array.from({ length: 7 }, (_, i) => fmt.format(new Date(2024, 0, 7 + i)));
  }, [i18n.language]);

  const gotoMonth = (delta: number) =>
    setCursor((c) => new Date(c.getFullYear(), c.getMonth() + delta, 1));
  const gotoToday = () => {
    const now = new Date();
    setCursor(new Date(now.getFullYear(), now.getMonth(), 1));
    setSelected(ymd(now));
  };

  const onSaved = () => {
    setEditing(null);
    reload();
  };

  const selectedList = eventsOn(events, selected);

  return (
    <div className="flex h-full min-h-0 flex-col px-4 pb-3 pt-1 text-white">
      {/* ツールバー */}
      <div className="flex items-center gap-2 py-2">
        <h2 className="min-w-[9rem] text-lg font-semibold">{monthLabel}</h2>
        {!showTrash && (
          <div className="flex items-center gap-1">
            <button
              onClick={() => gotoMonth(-1)}
              title={t('cal.prev')}
              className="rounded p-1.5 hover:bg-white/20"
            >
              <ChevronLeft size={16} />
            </button>
            <button
              onClick={gotoToday}
              className="rounded px-2.5 py-1 text-xs hover:bg-white/20"
            >
              {t('cal.today')}
            </button>
            <button
              onClick={() => gotoMonth(1)}
              title={t('cal.next')}
              className="rounded p-1.5 hover:bg-white/20"
            >
              <ChevronRight size={16} />
            </button>
          </div>
        )}
        <div className="flex-1" />
        <button
          onClick={() => setShowTrash((v) => !v)}
          className={`flex items-center gap-1.5 rounded px-2.5 py-1 text-xs hover:bg-white/20 ${showTrash ? 'bg-white/25' : ''}`}
          title={t('cal.trash')}
        >
          <Trash2 size={14} />
          {t('cal.trash')}
        </button>
        {!showTrash && (
          <button
            onClick={() => setEditing('new')}
            className="flex items-center gap-1.5 rounded bg-white/20 px-3 py-1.5 text-sm font-medium hover:bg-white/30"
          >
            <Plus size={15} />
            {t('cal.newEvent')}
          </button>
        )}
      </div>

      {showTrash ? (
        <TrashList
          items={trashed}
          onRestore={(id) => eventRestore(id).then(reload)}
          i18nLang={i18n.language}
        />
      ) : (
        <div className="flex min-h-0 flex-1 gap-3">
          {/* 月グリッド */}
          <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl bg-white/5 ring-1 ring-white/10">
            <div className="grid grid-cols-7 border-b border-white/10 text-center text-xs text-white/60">
              {weekdayLabels.map((w, i) => (
                <div key={w} className={`py-1.5 ${i === 0 ? 'text-red-300/80' : ''} ${i === 6 ? 'text-blue-300/80' : ''}`}>
                  {w}
                </div>
              ))}
            </div>
            <div className="grid min-h-0 flex-1 grid-cols-7 grid-rows-6">
              {grid.map((day) => {
                const ds = ymd(day);
                const inMonth = day.getMonth() === cursor.getMonth();
                const isToday = ds === todayStr;
                const isSel = ds === selected;
                const dayEvents = eventsOn(events, ds);
                return (
                  <button
                    key={ds}
                    onClick={() => setSelected(ds)}
                    onDoubleClick={() => {
                      setSelected(ds);
                      setEditing('new');
                    }}
                    className={`flex min-h-0 flex-col items-stretch gap-0.5 border-b border-r border-white/5 p-1 text-left transition-colors hover:bg-white/10 ${
                      isSel ? 'bg-white/15' : ''
                    } ${inMonth ? '' : 'opacity-40'}`}
                  >
                    <div className="flex justify-end">
                      <span
                        className={`flex h-5 w-5 items-center justify-center rounded-full text-xs ${
                          isToday ? 'bg-blue-500 font-semibold text-white' : 'text-white/80'
                        }`}
                      >
                        {day.getDate()}
                      </span>
                    </div>
                    <div className="flex min-h-0 flex-col gap-0.5 overflow-hidden">
                      {dayEvents.slice(0, 3).map((e) => (
                        <span
                          key={e.id}
                          className="flex items-center gap-1 truncate rounded px-1 py-0.5 text-[11px] leading-tight"
                          style={{ backgroundColor: `${e.color ?? DEFAULT_COLOR}33` }}
                        >
                          <span
                            className="h-1.5 w-1.5 shrink-0 rounded-full"
                            style={{ backgroundColor: e.color ?? DEFAULT_COLOR }}
                          />
                          <span className="truncate">
                            {!e.all_day && timeOf(e.start_at) ? `${timeOf(e.start_at)} ` : ''}
                            {e.title}
                          </span>
                        </span>
                      ))}
                      {dayEvents.length > 3 && (
                        <span className="px-1 text-[10px] text-white/50">＋{dayEvents.length - 3}</span>
                      )}
                    </div>
                  </button>
                );
              })}
            </div>
          </div>

          {/* 選択日のアジェンダ */}
          <aside className="flex w-72 shrink-0 flex-col overflow-hidden rounded-xl bg-white/5 ring-1 ring-white/10">
            <div className="border-b border-white/10 px-3 py-2 text-sm font-medium">
              {new Intl.DateTimeFormat(i18n.language, {
                month: 'long',
                day: 'numeric',
                weekday: 'short',
              }).format(new Date(`${selected}T00:00`))}
            </div>
            <div className="min-h-0 flex-1 space-y-1.5 overflow-y-auto p-2">
              {selectedList.length === 0 ? (
                <p className="px-1 py-6 text-center text-sm text-white/45">{t('cal.noEvents')}</p>
              ) : (
                selectedList.map((e) => (
                  <button
                    key={e.id}
                    onClick={() => setEditing(e)}
                    className="flex w-full items-start gap-2 rounded-lg p-2 text-left hover:bg-white/10"
                  >
                    <span
                      className="mt-1 h-3 w-1 shrink-0 rounded-full"
                      style={{ backgroundColor: e.color ?? DEFAULT_COLOR }}
                    />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium">{e.title}</span>
                      <span className="mt-0.5 flex items-center gap-1 text-xs text-white/55">
                        <Clock size={11} />
                        {e.all_day ? t('cal.allDay') : timeRange(e)}
                      </span>
                      {e.location && (
                        <span className="mt-0.5 flex items-center gap-1 truncate text-xs text-white/55">
                          <MapPin size={11} />
                          {e.location}
                        </span>
                      )}
                    </span>
                  </button>
                ))
              )}
            </div>
            <button
              onClick={() => setEditing('new')}
              className="m-2 flex items-center justify-center gap-1.5 rounded-lg border border-dashed border-white/20 py-2 text-sm text-white/70 hover:bg-white/10"
            >
              <Plus size={14} />
              {t('cal.newEvent')}
            </button>
          </aside>
        </div>
      )}

      {editing && (
        <EventModal
          event={editing === 'new' ? null : editing}
          defaultDay={selected}
          onClose={() => setEditing(null)}
          onSaved={onSaved}
          onDeleted={onSaved}
        />
      )}
    </div>
  );
}

/** 時間指定予定の時刻レンジ表示（'10:00 – 11:00' / 終了なしは開始のみ）。 */
function timeRange(e: EventSummary): string {
  const s = timeOf(e.start_at);
  const en = e.end_at ? timeOf(e.end_at) : '';
  return en && dayOf(e.end_at!) === dayOf(e.start_at) ? `${s} – ${en}` : s;
}

/** ゴミ箱の一覧（復元のみ。完全削除は保持期間で自動）。 */
function TrashList({
  items,
  onRestore,
  i18nLang,
}: {
  items: EventSummary[];
  onRestore: (id: number) => void;
  i18nLang: string;
}) {
  const { t } = useTranslation();
  if (items.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-white/45">
        {t('cal.trashEmpty')}
      </div>
    );
  }
  const fmt = new Intl.DateTimeFormat(i18nLang, { year: 'numeric', month: 'short', day: 'numeric' });
  return (
    <div className="min-h-0 flex-1 space-y-1.5 overflow-y-auto rounded-xl bg-white/5 p-2 ring-1 ring-white/10">
      {items.map((e) => (
        <div key={e.id} className="flex items-center gap-2 rounded-lg p-2 hover:bg-white/10">
          <span
            className="h-3 w-1 shrink-0 rounded-full"
            style={{ backgroundColor: e.color ?? DEFAULT_COLOR }}
          />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-sm font-medium">{e.title}</span>
            <span className="text-xs text-white/50">{fmt.format(new Date(`${dayOf(e.start_at)}T00:00`))}</span>
          </span>
          <button
            onClick={() => onRestore(e.id)}
            className="flex items-center gap-1 rounded px-2 py-1 text-xs hover:bg-white/20"
          >
            <RotateCcw size={13} />
            {t('cal.restore')}
          </button>
        </div>
      ))}
    </div>
  );
}

/** 予定の作成/編集モーダル。 */
function EventModal({
  event,
  defaultDay,
  onClose,
  onSaved,
  onDeleted,
}: {
  event: EventSummary | null;
  defaultDay: string;
  onClose: () => void;
  onSaved: () => void;
  onDeleted: () => void;
}) {
  const { t } = useTranslation();
  const [title, setTitle] = useState(event?.title ?? '');
  const [allDay, setAllDay] = useState(event?.all_day ?? false);
  const [startDate, setStartDate] = useState(event ? dayOf(event.start_at) : defaultDay);
  const [startTime, setStartTime] = useState(event ? timeOf(event.start_at) || '09:00' : '09:00');
  const [endDate, setEndDate] = useState(event?.end_at ? dayOf(event.end_at) : event ? dayOf(event.start_at) : defaultDay);
  const [endTime, setEndTime] = useState(event?.end_at ? timeOf(event.end_at) || '10:00' : '10:00');
  const [location, setLocation] = useState(event?.location ?? '');
  const [description, setDescription] = useState(event?.description ?? '');
  const [color, setColor] = useState(event?.color ?? DEFAULT_COLOR);
  const [busy, setBusy] = useState(false);

  // Esc で閉じる。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const canSave = title.trim().length > 0 && !!startDate && !busy;

  const save = async () => {
    if (!canSave) return;
    setBusy(true);
    // 終日は日付のみ、時間指定は 'YYYY-MM-DDTHH:MM'。終了は範囲があるときだけ送る。
    const start_at = allDay ? startDate : `${startDate}T${startTime}`;
    let end_at: string | null = null;
    if (allDay) {
      end_at = endDate && endDate > startDate ? endDate : null;
    } else {
      const ed = endDate || startDate;
      end_at = endTime ? `${ed}T${endTime}` : null;
    }
    const input: EventInput = {
      id: event?.id ?? null,
      title: title.trim(),
      description: description.trim() || null,
      location: location.trim() || null,
      start_at,
      end_at,
      all_day: allDay,
      color,
      recurrence: null,
      reminder_minutes: null,
      related_email_id: null,
    };
    try {
      await eventUpsert(input);
      onSaved();
    } catch {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!event) return;
    if (!window.confirm(t('cal.deleteConfirm'))) return;
    setBusy(true);
    try {
      await eventDelete(event.id);
      onDeleted();
    } catch {
      setBusy(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/50 p-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-2xl bg-neutral-900/95 p-5 text-white shadow-2xl ring-1 ring-white/15"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-3 flex items-center justify-between">
          <h3 className="text-base font-semibold">{event ? t('cal.editEvent') : t('cal.addEvent')}</h3>
          <button onClick={onClose} className="rounded p-1 hover:bg-white/15" title={t('cal.cancel')}>
            <X size={16} />
          </button>
        </div>

        <input
          autoFocus
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder={t('cal.fTitle')}
          className="mb-3 w-full rounded-lg bg-white/10 px-3 py-2 text-sm outline-none ring-1 ring-white/10 placeholder:text-white/40 focus:ring-white/30"
        />

        <label className="mb-3 flex items-center gap-2 text-sm">
          <input type="checkbox" checked={allDay} onChange={(e) => setAllDay(e.target.checked)} />
          {t('cal.allDay')}
        </label>

        <div className="mb-3 space-y-2">
          <div className="flex items-center gap-2">
            <span className="w-10 text-xs text-white/55">{t('cal.fStart')}</span>
            <input
              type="date"
              value={startDate}
              onChange={(e) => setStartDate(e.target.value)}
              className="flex-1 rounded-lg bg-white/10 px-2 py-1.5 text-sm outline-none ring-1 ring-white/10 [color-scheme:dark] focus:ring-white/30"
            />
            {!allDay && (
              <input
                type="time"
                value={startTime}
                onChange={(e) => setStartTime(e.target.value)}
                className="w-28 rounded-lg bg-white/10 px-2 py-1.5 text-sm outline-none ring-1 ring-white/10 [color-scheme:dark] focus:ring-white/30"
              />
            )}
          </div>
          <div className="flex items-center gap-2">
            <span className="w-10 text-xs text-white/55">{t('cal.fEnd')}</span>
            <input
              type="date"
              value={endDate}
              min={startDate}
              onChange={(e) => setEndDate(e.target.value)}
              className="flex-1 rounded-lg bg-white/10 px-2 py-1.5 text-sm outline-none ring-1 ring-white/10 [color-scheme:dark] focus:ring-white/30"
            />
            {!allDay && (
              <input
                type="time"
                value={endTime}
                onChange={(e) => setEndTime(e.target.value)}
                className="w-28 rounded-lg bg-white/10 px-2 py-1.5 text-sm outline-none ring-1 ring-white/10 [color-scheme:dark] focus:ring-white/30"
              />
            )}
          </div>
        </div>

        <input
          value={location}
          onChange={(e) => setLocation(e.target.value)}
          placeholder={t('cal.fLocation')}
          className="mb-3 w-full rounded-lg bg-white/10 px-3 py-2 text-sm outline-none ring-1 ring-white/10 placeholder:text-white/40 focus:ring-white/30"
        />

        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder={t('cal.fDescription')}
          rows={2}
          className="mb-3 w-full resize-none rounded-lg bg-white/10 px-3 py-2 text-sm outline-none ring-1 ring-white/10 placeholder:text-white/40 focus:ring-white/30"
        />

        <div className="mb-4 flex items-center gap-2">
          <span className="text-xs text-white/55">{t('cal.fColor')}</span>
          {COLORS.map((c) => (
            <button
              key={c}
              onClick={() => setColor(c)}
              className={`h-5 w-5 rounded-full transition-transform ${color === c ? 'scale-110 ring-2 ring-white' : ''}`}
              style={{ backgroundColor: c }}
            />
          ))}
        </div>

        <div className="flex items-center gap-2">
          {event && (
            <button
              onClick={remove}
              disabled={busy}
              className="flex items-center gap-1.5 rounded-lg px-3 py-2 text-sm text-red-300 hover:bg-red-500/20 disabled:opacity-50"
            >
              <Trash2 size={14} />
              {t('cal.delete')}
            </button>
          )}
          <div className="flex-1" />
          <button onClick={onClose} className="rounded-lg px-3 py-2 text-sm hover:bg-white/10">
            {t('cal.cancel')}
          </button>
          <button
            onClick={save}
            disabled={!canSave}
            className="rounded-lg bg-blue-500 px-4 py-2 text-sm font-medium hover:bg-blue-400 disabled:opacity-40"
          >
            {t('cal.save')}
          </button>
        </div>
      </div>
    </div>
  );
}
