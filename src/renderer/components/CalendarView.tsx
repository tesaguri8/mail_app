import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronLeft, ChevronRight, Plus, Trash2, X, Clock, MapPin, RotateCcw, Repeat } from 'lucide-react';
import type { EventSummary } from '@bindings/EventSummary';
import type { EventInput } from '@bindings/EventInput';
import { eventList, eventListTrashed, eventUpsert, eventDelete, eventRestore, eventGet } from '../services/calendar';
import { expandEvents, presetToRule, ruleToPreset, RECUR_PRESETS, type RecurPreset } from '../utils/recurrence';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 予定の色パレット（未選択＝既定の青）。CSS 色をそのまま color 列に保存する。 */
const COLORS = ['#64b5f6', '#e57373', '#f6bf50', '#81c784', '#ba91e0', '#4db6ac', '#f06292'];
const DEFAULT_COLOR = COLORS[0];

/** 表示単位。年＝12ミニ月、月＝6週、2週＝14日（いずれも日セル）、週/日＝タイムグリッド。 */
type ViewMode = 'year' | 'month' | 'fortnight' | 'week' | 'day';
const MODE_KEY = 'rondine.cal.mode';
const MODES: { m: ViewMode; key: string }[] = [
  { m: 'year', key: 'cal.vYear' },
  { m: 'month', key: 'cal.vMonth' },
  { m: 'fortnight', key: 'cal.vFortnight' },
  { m: 'week', key: 'cal.vWeek' },
  { m: 'day', key: 'cal.vDay' },
];

/** 編集対象。新規はプレフィル（日・時刻・終日）を運ぶ。 */
type EditTarget =
  | { mode: 'edit'; event: EventSummary }
  | { mode: 'new'; day: string; time?: string; allDay?: boolean };

// ── 日付ヘルパー（保存は端末ローカルの素の ISO 文字列。UTC 変換はしない） ──
const pad = (n: number) => String(n).padStart(2, '0');
/** Date → 'YYYY-MM-DD'（ローカル）。 */
const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
/** date の n 日後（ローカル）。 */
const addDays = (date: Date, n: number) => {
  const d = new Date(date);
  d.setDate(d.getDate() + n);
  return d;
};
/** その週の日曜0時。週/2週グリッドの起点。 */
const startOfWeek = (d: Date) => {
  const x = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  return addDays(x, -x.getDay());
};
/** 日付部分（先頭10文字）を取り出す。'2026-07-06T10:00' → '2026-07-06'。 */
const dayOf = (iso: string) => iso.slice(0, 10);
/** 時刻部分（HH:MM）を取り出す。日付のみなら空。 */
const timeOf = (iso: string) => (iso.length > 10 ? iso.slice(11, 16) : '');
/** ISO の時刻を「その日の 0 時からの分」に。日付のみは 0。 */
const minutesOf = (iso: string) => {
  const t = timeOf(iso);
  if (!t) return 0;
  const [h, m] = t.split(':').map(Number);
  return h * 60 + m;
};

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

/** 予定が掛かる全日付（'YYYY-MM-DD'）。年表示のドット表示・存在判定に使う。 */
function coveredDays(e: EventSummary): string[] {
  const start = dayOf(e.start_at);
  const end = e.end_at ? dayOf(e.end_at) : start;
  const out: string[] = [];
  let d = new Date(`${start}T00:00`);
  let guard = 0;
  while (ymd(d) <= end && guard < 400) {
    out.push(ymd(d));
    d = addDays(d, 1);
    guard++;
  }
  return out;
}

/** 月/週/2週グリッド、年（12ミニ月）、タイムグリッド（週/日）の導出値。 */
type Period =
  | { kind: 'year'; year: number; from: string; to: string; label: string }
  | { kind: 'grid'; rows: number; days: Date[]; from: string; to: string; label: string }
  | { kind: 'time'; days: Date[]; from: string; to: string; label: string };

function computePeriod(mode: ViewMode, anchor: Date, locale: string): Period {
  const y = anchor.getFullYear();
  if (mode === 'year') {
    return {
      kind: 'year',
      year: y,
      from: `${y}-01-01`,
      to: `${y + 1}-01-01`,
      label: new Intl.DateTimeFormat(locale, { year: 'numeric' }).format(anchor),
    };
  }
  if (mode === 'month') {
    const first = new Date(y, anchor.getMonth(), 1);
    const gridStart = addDays(first, -first.getDay());
    const days = Array.from({ length: 42 }, (_, i) => addDays(gridStart, i));
    return {
      kind: 'grid',
      rows: 6,
      days,
      from: ymd(days[0]),
      to: ymd(addDays(days[41], 1)),
      label: new Intl.DateTimeFormat(locale, { year: 'numeric', month: 'long' }).format(anchor),
    };
  }
  if (mode === 'day') {
    const day = new Date(y, anchor.getMonth(), anchor.getDate());
    return {
      kind: 'time',
      days: [day],
      from: ymd(day),
      to: ymd(addDays(day, 1)),
      label: new Intl.DateTimeFormat(locale, { year: 'numeric', month: 'long', day: 'numeric', weekday: 'short' }).format(day),
    };
  }
  // 週（タイムグリッド） / 2週（日セル）。いずれも日曜始まり。
  const start = startOfWeek(anchor);
  const len = mode === 'fortnight' ? 14 : 7;
  const days = Array.from({ length: len }, (_, i) => addDays(start, i));
  const f = new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric' });
  const common = { from: ymd(days[0]), to: ymd(addDays(days[len - 1], 1)), label: `${f.format(days[0])} – ${f.format(days[len - 1])}` };
  return mode === 'week' ? { kind: 'time', days, ...common } : { kind: 'grid', rows: 2, days, ...common };
}

/** 前/次ボタンで anchor をモードの単位（年/月/2週/週/日）だけ動かす。 */
function stepAnchor(mode: ViewMode, a: Date, dir: number): Date {
  const x = new Date(a);
  if (mode === 'year') x.setFullYear(x.getFullYear() + dir);
  else if (mode === 'month') x.setMonth(x.getMonth() + dir);
  else if (mode === 'day') x.setDate(x.getDate() + dir);
  else x.setDate(x.getDate() + (mode === 'fortnight' ? 14 : 7) * dir);
  return x;
}

export function CalendarView() {
  const { t, i18n } = useTranslation();
  const [mode, setMode] = useState<ViewMode>(() => {
    const s = localStorage.getItem(MODE_KEY);
    return s === 'year' || s === 'fortnight' || s === 'week' || s === 'day' ? s : 'month';
  });
  useEffect(() => {
    localStorage.setItem(MODE_KEY, mode);
  }, [mode]);
  // 表示中の基準日（この日を含む年/月/週/日を表示する）。
  const [anchor, setAnchor] = useState<Date>(() => new Date());
  const [selected, setSelected] = useState<string>(() => ymd(new Date()));
  const [events, setEvents] = useState<EventSummary[]>([]);
  const [trashed, setTrashed] = useState<EventSummary[]>([]);
  const [showTrash, setShowTrash] = useState(false);
  const [editing, setEditing] = useState<EditTarget | null>(null);
  const todayStr = ymd(new Date());

  const period = useMemo(() => computePeriod(mode, anchor, i18n.language), [mode, anchor, i18n.language]);

  const reload = useCallback(() => {
    if (!isTauri) return;
    if (showTrash) {
      eventListTrashed().then(setTrashed).catch(() => setTrashed([]));
      return;
    }
    eventList(period.from, period.to)
      .then((rows) => setEvents(expandEvents(rows, period.from, period.to)))
      .catch(() => setEvents([]));
  }, [period.from, period.to, showTrash]);
  useEffect(reload, [reload]);

  const eventDays = useMemo(() => new Set(events.flatMap(coveredDays)), [events]);
  const weekdayShort = useMemo(() => weekdayLabels(i18n.language, 'short'), [i18n.language]);
  const weekdayNarrow = useMemo(() => weekdayLabels(i18n.language, 'narrow'), [i18n.language]);

  const gotoToday = () => {
    const now = new Date();
    setAnchor(now);
    setSelected(ymd(now));
  };
  const onSaved = () => {
    setEditing(null);
    reload();
  };
  const newAt = (day: string, time?: string, allDay?: boolean) => setEditing({ mode: 'new', day, time, allDay });
  // 繰り返しの出現をクリックした場合は「元（シリーズ）」を取り直して編集する
  // （出現の日付で元を上書きしないため）。編集はシリーズ全体に適用。
  const openEvent = (event: EventSummary) => {
    if (event.recurrence) {
      eventGet(event.id)
        .then((master) => setEditing({ mode: 'edit', event: master }))
        .catch(() => setEditing({ mode: 'edit', event }));
    } else {
      setEditing({ mode: 'edit', event });
    }
  };
  const selectedList = eventsOn(events, selected);
  const maxVisible = mode === 'fortnight' ? 5 : 3;

  return (
    <div className="flex h-full min-h-0 flex-col px-4 pb-3 pt-1 text-white">
      {/* ツールバー */}
      <div className="flex flex-wrap items-center gap-2 py-2">
        <h2 className="min-w-[9rem] text-lg font-semibold">{period.label}</h2>
        {!showTrash && (
          <>
            <div className="flex items-center gap-1">
              <button onClick={() => setAnchor((a) => stepAnchor(mode, a, -1))} title={t('cal.prev')} className="rounded p-1.5 hover:bg-white/20">
                <ChevronLeft size={16} />
              </button>
              <button onClick={gotoToday} className="rounded px-2.5 py-1 text-xs hover:bg-white/20">
                {t('cal.today')}
              </button>
              <button onClick={() => setAnchor((a) => stepAnchor(mode, a, 1))} title={t('cal.next')} className="rounded p-1.5 hover:bg-white/20">
                <ChevronRight size={16} />
              </button>
            </div>
            <div className="flex items-center gap-0.5 rounded-lg bg-white/10 p-0.5 text-xs">
              {MODES.map(({ m, key }) => (
                <button
                  key={m}
                  onClick={() => setMode(m)}
                  className={`rounded px-2 py-1 hover:bg-white/15 ${mode === m ? 'bg-white/25 font-medium' : ''}`}
                >
                  {t(key)}
                </button>
              ))}
            </div>
          </>
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
            onClick={() => newAt(selected)}
            className="flex items-center gap-1.5 rounded bg-white/20 px-3 py-1.5 text-sm font-medium hover:bg-white/30"
          >
            <Plus size={15} />
            {t('cal.newEvent')}
          </button>
        )}
      </div>

      {showTrash ? (
        <TrashList items={trashed} onRestore={(id) => eventRestore(id).then(reload)} i18nLang={i18n.language} />
      ) : period.kind === 'time' ? (
        <TimeGrid
          days={period.days}
          events={events}
          todayStr={todayStr}
          locale={i18n.language}
          onOpenDay={(ds) => {
            setAnchor(new Date(`${ds}T00:00`));
            setMode('day');
          }}
          onNewAt={newAt}
          onOpenEvent={openEvent}
        />
      ) : period.kind === 'year' ? (
        <div className="flex min-h-0 flex-1 gap-3">
          <div className="min-h-0 flex-1 overflow-y-auto rounded-xl bg-white/5 p-3 ring-1 ring-white/10">
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
              {Array.from({ length: 12 }, (_, m) => (
                <MiniMonth
                  key={m}
                  year={period.year}
                  month={m}
                  weekdayNarrow={weekdayNarrow}
                  eventDays={eventDays}
                  selected={selected}
                  todayStr={todayStr}
                  locale={i18n.language}
                  onSelectDay={setSelected}
                  onOpenDay={(ds) => newAt(ds)}
                  onOpenMonth={(mm) => {
                    setAnchor(new Date(period.year, mm, 1));
                    setMode('month');
                  }}
                />
              ))}
            </div>
          </div>
          <AgendaPanel selected={selected} list={selectedList} locale={i18n.language} onOpen={openEvent} onNew={() => newAt(selected)} />
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 gap-3">
          <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl bg-white/5 ring-1 ring-white/10">
            <div className="grid grid-cols-7 border-b border-white/10 text-center text-xs text-white/60">
              {weekdayShort.map((w, i) => (
                <div key={w} className={`py-1.5 ${i === 0 ? 'text-red-300/80' : ''} ${i === 6 ? 'text-blue-300/80' : ''}`}>
                  {w}
                </div>
              ))}
            </div>
            <div className="grid min-h-0 flex-1 grid-cols-7" style={{ gridTemplateRows: `repeat(${period.rows}, minmax(0, 1fr))` }}>
              {period.days.map((day) => (
                <DayCell
                  key={ymd(day)}
                  date={day}
                  events={events}
                  selected={selected}
                  todayStr={todayStr}
                  dim={mode === 'month' && day.getMonth() !== anchor.getMonth()}
                  maxVisible={maxVisible}
                  onSelect={setSelected}
                  onOpenNew={(ds) => newAt(ds)}
                />
              ))}
            </div>
          </div>
          <AgendaPanel selected={selected} list={selectedList} locale={i18n.language} onOpen={openEvent} onNew={() => newAt(selected)} />
        </div>
      )}

      {editing && (
        <EventModal target={editing} onClose={() => setEditing(null)} onSaved={onSaved} onDeleted={onSaved} />
      )}
    </div>
  );
}

/** 曜日ラベルを Intl で生成（既知の日曜=2024-01-07 起点に7日）。 */
function weekdayLabels(locale: string, width: 'short' | 'narrow'): string[] {
  const fmt = new Intl.DateTimeFormat(locale, { weekday: width });
  return Array.from({ length: 7 }, (_, i) => fmt.format(new Date(2024, 0, 7 + i)));
}

/** 予定チップ（色ドット＋時刻＋タイトル）。 */
function EventChip({ e }: { e: EventSummary }) {
  return (
    <span
      className="flex items-center gap-1 truncate rounded px-1 py-0.5 text-[11px] leading-tight"
      style={{ backgroundColor: `${e.color ?? DEFAULT_COLOR}33` }}
    >
      <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ backgroundColor: e.color ?? DEFAULT_COLOR }} />
      <span className="truncate">
        {!e.all_day && timeOf(e.start_at) ? `${timeOf(e.start_at)} ` : ''}
        {e.title}
      </span>
    </span>
  );
}

/** 月/週/2週グリッドの1日セル。 */
function DayCell({
  date,
  events,
  selected,
  todayStr,
  dim,
  maxVisible,
  onSelect,
  onOpenNew,
}: {
  date: Date;
  events: EventSummary[];
  selected: string;
  todayStr: string;
  dim: boolean;
  maxVisible: number;
  onSelect: (ds: string) => void;
  onOpenNew: (ds: string) => void;
}) {
  const ds = ymd(date);
  const isToday = ds === todayStr;
  const isSel = ds === selected;
  const dayEvents = eventsOn(events, ds);
  return (
    <button
      onClick={() => onSelect(ds)}
      onDoubleClick={() => onOpenNew(ds)}
      className={`flex min-h-0 flex-col items-stretch gap-0.5 border-b border-r border-white/5 p-1 text-left transition-colors hover:bg-white/10 ${
        isSel ? 'bg-white/15' : ''
      } ${dim ? 'opacity-40' : ''}`}
    >
      <div className="flex justify-end">
        <span
          className={`flex h-5 w-5 items-center justify-center rounded-full text-xs ${
            isToday ? 'bg-blue-500 font-semibold text-white' : 'text-white/80'
          }`}
        >
          {date.getDate()}
        </span>
      </div>
      <div className="flex min-h-0 flex-col gap-0.5 overflow-hidden">
        {dayEvents.slice(0, maxVisible).map((e) => (
          <EventChip key={e.id} e={e} />
        ))}
        {dayEvents.length > maxVisible && (
          <span className="px-1 text-[10px] text-white/50">＋{dayEvents.length - maxVisible}</span>
        )}
      </div>
    </button>
  );
}

const ROW_H = 44; // タイムグリッド1時間の高さ(px)

/** 週/日のタイムグリッド（時刻の行 × 日の列。終日バンド＋時間指定を重なり配置）。 */
function TimeGrid({
  days,
  events,
  todayStr,
  locale,
  onOpenDay,
  onNewAt,
  onOpenEvent,
}: {
  days: Date[];
  events: EventSummary[];
  todayStr: string;
  locale: string;
  onOpenDay: (ds: string) => void;
  onNewAt: (day: string, time?: string, allDay?: boolean) => void;
  onOpenEvent: (e: EventSummary) => void;
}) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  // 起動時は 7:00 あたりへスクロール。
  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = 7 * ROW_H;
  }, []);
  // 現在時刻ラインを1分ごとに更新（今日を含むときだけ意味を持つ）。
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick((n) => n + 1), 60_000);
    return () => clearInterval(id);
  }, []);

  const hours = Array.from({ length: 24 }, (_, h) => h);
  const dowFmt = new Intl.DateTimeFormat(locale, { weekday: 'short' });
  const now = new Date();
  const nowMin = now.getHours() * 60 + now.getMinutes();

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl bg-white/5 ring-1 ring-white/10">
      {/* 日ヘッダ */}
      <div className="flex border-b border-white/10">
        <div className="w-12 shrink-0" />
        {days.map((d) => {
          const ds = ymd(d);
          const isToday = ds === todayStr;
          return (
            <button
              key={ds}
              onClick={() => onOpenDay(ds)}
              className="flex flex-1 flex-col items-center gap-0.5 border-l border-white/5 py-1.5 hover:bg-white/10"
              title={t('cal.vDay')}
            >
              <span className="text-[11px] text-white/55">{dowFmt.format(d)}</span>
              <span
                className={`flex h-6 w-6 items-center justify-center rounded-full text-sm ${
                  isToday ? 'bg-blue-500 font-semibold text-white' : 'text-white/85'
                }`}
              >
                {d.getDate()}
              </span>
            </button>
          );
        })}
      </div>

      {/* 終日バンド */}
      <div className="flex border-b border-white/10">
        <div className="flex w-12 shrink-0 items-center justify-end pr-1 text-[10px] text-white/40">{t('cal.allDay')}</div>
        {days.map((d) => {
          const ds = ymd(d);
          const list = eventsOn(events, ds).filter((e) => e.all_day);
          return (
            <div
              key={ds}
              onClick={() => onNewAt(ds, undefined, true)}
              className="min-h-[1.75rem] min-w-0 flex-1 space-y-0.5 border-l border-white/5 p-0.5"
            >
              {list.map((e) => (
                <button
                  key={e.id}
                  onClick={(ev) => {
                    ev.stopPropagation();
                    onOpenEvent(e);
                  }}
                  className="block w-full truncate rounded px-1 py-0.5 text-left text-[11px] text-white"
                  style={{ backgroundColor: `${e.color ?? DEFAULT_COLOR}cc` }}
                >
                  {e.title}
                </button>
              ))}
            </div>
          );
        })}
      </div>

      {/* 時間帯（スクロール） */}
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto">
        <div className="flex" style={{ height: 24 * ROW_H }}>
          {/* 時刻の目盛り */}
          <div className="w-12 shrink-0">
            {hours.map((h) => (
              <div key={h} style={{ height: ROW_H }} className="relative">
                {h > 0 && <span className="absolute -top-2 right-1 text-[10px] text-white/40">{h}:00</span>}
              </div>
            ))}
          </div>
          {/* 日ごとの列 */}
          {days.map((d) => {
            const ds = ymd(d);
            const segs = timedSegments(events, d);
            const layout = packDay(segs.map((s) => ({ id: s.e.id, start: s.startMin, end: s.endMin })));
            return (
              <div
                key={ds}
                className="relative min-w-0 flex-1 border-l border-white/5"
                onClick={(ev) => {
                  const rect = ev.currentTarget.getBoundingClientRect();
                  const hour = Math.max(0, Math.min(23, Math.floor((ev.clientY - rect.top) / ROW_H)));
                  onNewAt(ds, `${pad(hour)}:00`);
                }}
              >
                {hours.map((h) => (
                  <div key={h} style={{ height: ROW_H }} className="border-b border-white/5" />
                ))}
                {ds === todayStr && (
                  <div className="pointer-events-none absolute left-0 right-0 z-10 flex items-center" style={{ top: (nowMin / 60) * ROW_H }}>
                    <span className="h-1.5 w-1.5 rounded-full bg-red-500" />
                    <span className="h-px flex-1 bg-red-500" />
                  </div>
                )}
                {segs.map((s) => {
                  const l = layout.get(s.e.id) ?? { col: 0, cols: 1 };
                  const top = (s.startMin / 60) * ROW_H;
                  const height = Math.max(18, ((s.endMin - s.startMin) / 60) * ROW_H);
                  return (
                    <button
                      key={s.e.id}
                      onClick={(ev) => {
                        ev.stopPropagation();
                        onOpenEvent(s.e);
                      }}
                      className="absolute overflow-hidden rounded px-1 py-0.5 text-left text-white ring-1 ring-black/10"
                      style={{
                        top,
                        height,
                        left: `calc(${(l.col / l.cols) * 100}% + 1px)`,
                        width: `calc(${100 / l.cols}% - 2px)`,
                        backgroundColor: `${s.e.color ?? DEFAULT_COLOR}e6`,
                      }}
                    >
                      <span className="block truncate text-[11px] font-medium leading-tight">{s.e.title}</span>
                      {height > 30 && <span className="block truncate text-[10px] leading-tight opacity-90">{timeOf(s.e.start_at)}</span>}
                    </button>
                  );
                })}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

/** 指定日に掛かる時間指定予定を、その日の [開始分, 終了分]（0..1440 にクランプ）で返す。 */
function timedSegments(events: EventSummary[], dayDate: Date): { e: EventSummary; startMin: number; endMin: number }[] {
  const ds = ymd(dayDate);
  const out: { e: EventSummary; startMin: number; endMin: number }[] = [];
  for (const e of events) {
    if (e.all_day) continue;
    const sDay = dayOf(e.start_at);
    const endIso = e.end_at ?? e.start_at;
    const eDay = dayOf(endIso);
    if (ds < sDay || ds > eDay) continue;
    const startMin = ds === sDay ? minutesOf(e.start_at) : 0;
    let endMin = ds === eDay ? minutesOf(endIso) : 1440;
    if (!e.end_at) endMin = Math.min(1440, startMin + 60); // 終了なしは1時間として表示
    if (endMin <= startMin) endMin = Math.min(1440, startMin + 30);
    out.push({ e, startMin, endMin });
  }
  return out;
}

/** 重なり配置。開始順に貪欲にレーンを割り当て、重なり集団ごとに列数で幅を分ける。 */
function packDay(items: { id: number; start: number; end: number }[]): Map<number, { col: number; cols: number }> {
  const out = new Map<number, { col: number; cols: number }>();
  const sorted = [...items].sort((a, b) => a.start - b.start || a.end - b.end);
  let cluster: { id: number; start: number; end: number; col: number }[] = [];
  let clusterEnd = -1;
  const finalize = () => {
    const laneEnds: number[] = []; // 各レーンの最終終了
    for (const it of cluster) {
      let placed = false;
      for (let c = 0; c < laneEnds.length; c++) {
        if (laneEnds[c] <= it.start) {
          it.col = c;
          laneEnds[c] = it.end;
          placed = true;
          break;
        }
      }
      if (!placed) {
        it.col = laneEnds.length;
        laneEnds.push(it.end);
      }
    }
    const cols = laneEnds.length;
    for (const it of cluster) out.set(it.id, { col: it.col, cols });
    cluster = [];
    clusterEnd = -1;
  };
  for (const it of sorted) {
    if (cluster.length && it.start >= clusterEnd) finalize();
    cluster.push({ ...it, col: 0 });
    clusterEnd = Math.max(clusterEnd, it.end);
  }
  if (cluster.length) finalize();
  return out;
}

/** 年表示のミニ月（予定のある日にドット、月名クリックでその月へ）。 */
function MiniMonth({
  year,
  month,
  weekdayNarrow,
  eventDays,
  selected,
  todayStr,
  locale,
  onSelectDay,
  onOpenDay,
  onOpenMonth,
}: {
  year: number;
  month: number;
  weekdayNarrow: string[];
  eventDays: Set<string>;
  selected: string;
  todayStr: string;
  locale: string;
  onSelectDay: (ds: string) => void;
  onOpenDay: (ds: string) => void;
  onOpenMonth: (month: number) => void;
}) {
  const first = new Date(year, month, 1);
  const gridStart = addDays(first, -first.getDay());
  const days = Array.from({ length: 42 }, (_, i) => addDays(gridStart, i));
  const monthName = new Intl.DateTimeFormat(locale, { month: 'long' }).format(first);
  return (
    <div className="rounded-lg bg-white/5 p-2 ring-1 ring-white/10">
      <button
        onClick={() => onOpenMonth(month)}
        className="mb-1 w-full truncate text-left text-xs font-semibold text-white/90 hover:text-white"
      >
        {monthName}
      </button>
      <div className="grid grid-cols-7 gap-0.5 text-center text-[9px] text-white/35">
        {weekdayNarrow.map((w, i) => (
          <span key={i}>{w}</span>
        ))}
      </div>
      <div className="mt-0.5 grid grid-cols-7 gap-0.5">
        {days.map((d) => {
          const ds = ymd(d);
          const inM = d.getMonth() === month;
          const isToday = ds === todayStr;
          const isSel = ds === selected;
          const has = inM && eventDays.has(ds);
          return (
            <button
              key={ds}
              onClick={() => onSelectDay(ds)}
              onDoubleClick={() => onOpenDay(ds)}
              className={`relative flex h-5 items-center justify-center rounded text-[10px] hover:bg-white/15 ${
                inM ? '' : 'opacity-25'
              } ${isSel ? 'ring-1 ring-white/70' : ''}`}
            >
              <span className={`flex h-4 w-4 items-center justify-center rounded-full ${isToday ? 'bg-blue-500 text-white' : ''}`}>
                {d.getDate()}
              </span>
              {has && !isToday && <span className="absolute bottom-0 h-1 w-1 rounded-full bg-white/70" />}
            </button>
          );
        })}
      </div>
    </div>
  );
}

/** 選択日のアジェンダ（右側パネル。年/月/2週で表示）。 */
function AgendaPanel({
  selected,
  list,
  locale,
  onOpen,
  onNew,
}: {
  selected: string;
  list: EventSummary[];
  locale: string;
  onOpen: (e: EventSummary) => void;
  onNew: () => void;
}) {
  const { t } = useTranslation();
  return (
    <aside className="flex w-72 shrink-0 flex-col overflow-hidden rounded-xl bg-white/5 ring-1 ring-white/10">
      <div className="border-b border-white/10 px-3 py-2 text-sm font-medium">
        {new Intl.DateTimeFormat(locale, { month: 'long', day: 'numeric', weekday: 'short' }).format(new Date(`${selected}T00:00`))}
      </div>
      <div className="min-h-0 flex-1 space-y-1.5 overflow-y-auto p-2">
        {list.length === 0 ? (
          <p className="px-1 py-6 text-center text-sm text-white/45">{t('cal.noEvents')}</p>
        ) : (
          list.map((e) => (
            <button key={e.id} onClick={() => onOpen(e)} className="flex w-full items-start gap-2 rounded-lg p-2 text-left hover:bg-white/10">
              <span className="mt-1 h-3 w-1 shrink-0 rounded-full" style={{ backgroundColor: e.color ?? DEFAULT_COLOR }} />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium">{e.title}</span>
                <span className="mt-0.5 flex items-center gap-1 text-xs text-white/55">
                  <Clock size={11} />
                  {e.all_day ? t('cal.allDay') : timeRange(e)}
                  {e.recurrence && <Repeat size={11} className="text-white/45" />}
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
        onClick={onNew}
        className="m-2 flex items-center justify-center gap-1.5 rounded-lg border border-dashed border-white/20 py-2 text-sm text-white/70 hover:bg-white/10"
      >
        <Plus size={14} />
        {t('cal.newEvent')}
      </button>
    </aside>
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
    return <div className="flex flex-1 items-center justify-center text-sm text-white/45">{t('cal.trashEmpty')}</div>;
  }
  const fmt = new Intl.DateTimeFormat(i18nLang, { year: 'numeric', month: 'short', day: 'numeric' });
  return (
    <div className="min-h-0 flex-1 space-y-1.5 overflow-y-auto rounded-xl bg-white/5 p-2 ring-1 ring-white/10">
      {items.map((e) => (
        <div key={e.id} className="flex items-center gap-2 rounded-lg p-2 hover:bg-white/10">
          <span className="h-3 w-1 shrink-0 rounded-full" style={{ backgroundColor: e.color ?? DEFAULT_COLOR }} />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-sm font-medium">{e.title}</span>
            <span className="text-xs text-white/50">{fmt.format(new Date(`${dayOf(e.start_at)}T00:00`))}</span>
          </span>
          <button onClick={() => onRestore(e.id)} className="flex items-center gap-1 rounded px-2 py-1 text-xs hover:bg-white/20">
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
  target,
  onClose,
  onSaved,
  onDeleted,
}: {
  target: EditTarget;
  onClose: () => void;
  onSaved: () => void;
  onDeleted: () => void;
}) {
  const { t } = useTranslation();
  const event = target.mode === 'edit' ? target.event : null;
  const prefDay = target.mode === 'new' ? target.day : dayOf(target.event.start_at);
  const prefTime = target.mode === 'new' ? target.time : undefined;
  const prefAllDay = target.mode === 'new' ? target.allDay ?? false : target.event.all_day;

  const [title, setTitle] = useState(event?.title ?? '');
  const [allDay, setAllDay] = useState(prefAllDay);
  const [startDate, setStartDate] = useState(event ? dayOf(event.start_at) : prefDay);
  const [startTime, setStartTime] = useState(event ? timeOf(event.start_at) || '09:00' : prefTime || '09:00');
  const [endDate, setEndDate] = useState(event?.end_at ? dayOf(event.end_at) : prefDay);
  const [endTime, setEndTime] = useState(
    event?.end_at ? timeOf(event.end_at) || '10:00' : addOneHour(prefTime || '09:00'),
  );
  const [location, setLocation] = useState(event?.location ?? '');
  const [description, setDescription] = useState(event?.description ?? '');
  const [color, setColor] = useState(event?.color ?? DEFAULT_COLOR);
  const initialRecur = ruleToPreset(event?.recurrence ?? null);
  const [recur, setRecur] = useState<RecurPreset>(initialRecur.preset);
  const [until, setUntil] = useState<string>(initialRecur.until ?? '');
  const [busy, setBusy] = useState(false);

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
      recurrence: presetToRule(recur, until || null),
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
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/50 p-4" onClick={onClose}>
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

        {/* 繰り返し */}
        <div className="mb-3 flex items-center gap-2">
          <Repeat size={14} className="text-white/55" />
          <select
            value={recur}
            onChange={(e) => setRecur(e.target.value as RecurPreset)}
            className="flex-1 rounded-lg bg-white/10 px-2 py-1.5 text-sm outline-none ring-1 ring-white/10 [color-scheme:dark] focus:ring-white/30"
          >
            {RECUR_PRESETS.map((p) => (
              <option key={p} value={p} className="bg-neutral-800">
                {t(`cal.r_${p}`)}
              </option>
            ))}
          </select>
          {recur !== 'none' && (
            <input
              type="date"
              value={until}
              min={startDate}
              onChange={(e) => setUntil(e.target.value)}
              title={t('cal.repeatUntil')}
              className="w-40 rounded-lg bg-white/10 px-2 py-1.5 text-sm outline-none ring-1 ring-white/10 [color-scheme:dark] focus:ring-white/30"
            />
          )}
        </div>
        {event && event.recurrence && (
          <p className="mb-3 text-xs text-white/45">{t('cal.seriesNote')}</p>
        )}

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

/** 'HH:MM' の1時間後（23時台は 23:59 で頭打ち）。新規作成時の既定終了時刻に使う。 */
function addOneHour(hm: string): string {
  const [h, m] = hm.split(':').map(Number);
  if (h >= 23) return '23:59';
  return `${pad(h + 1)}:${pad(m)}`;
}
