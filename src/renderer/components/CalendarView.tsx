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

/** 表示単位。年＝12ミニ月、月＝6週グリッド、2週＝14日、週＝7日。 */
type ViewMode = 'year' | 'month' | 'fortnight' | 'week';
const MODE_KEY = 'rondine.cal.mode';
const MODES: { m: ViewMode; key: string }[] = [
  { m: 'year', key: 'cal.vYear' },
  { m: 'month', key: 'cal.vMonth' },
  { m: 'fortnight', key: 'cal.vFortnight' },
  { m: 'week', key: 'cal.vWeek' },
];

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

/** 月/週/2週グリッド、または年（12ミニ月）の表示範囲・見出しをまとめた導出値。 */
type Period =
  | { kind: 'year'; year: number; from: string; to: string; label: string }
  | { kind: 'grid'; rows: number; days: Date[]; from: string; to: string; label: string };

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
  // 週 / 2週（日曜始まり）
  const start = startOfWeek(anchor);
  const len = mode === 'fortnight' ? 14 : 7;
  const days = Array.from({ length: len }, (_, i) => addDays(start, i));
  const f = new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric' });
  return {
    kind: 'grid',
    rows: len / 7,
    days,
    from: ymd(days[0]),
    to: ymd(addDays(days[len - 1], 1)),
    label: `${f.format(days[0])} – ${f.format(days[len - 1])}`,
  };
}

/** 前/次ボタンで anchor をモードの単位（年/月/2週/週）だけ動かす。 */
function stepAnchor(mode: ViewMode, a: Date, dir: number): Date {
  const x = new Date(a);
  if (mode === 'year') x.setFullYear(x.getFullYear() + dir);
  else if (mode === 'month') x.setMonth(x.getMonth() + dir);
  else x.setDate(x.getDate() + (mode === 'fortnight' ? 14 : 7) * dir);
  return x;
}

export function CalendarView() {
  const { t, i18n } = useTranslation();
  const [mode, setMode] = useState<ViewMode>(() => {
    const s = localStorage.getItem(MODE_KEY);
    return s === 'year' || s === 'fortnight' || s === 'week' ? s : 'month';
  });
  useEffect(() => {
    localStorage.setItem(MODE_KEY, mode);
  }, [mode]);
  // 表示中の基準日（この日を含む年/月/週を表示する）。
  const [anchor, setAnchor] = useState<Date>(() => new Date());
  const [selected, setSelected] = useState<string>(() => ymd(new Date()));
  const [events, setEvents] = useState<EventSummary[]>([]);
  const [trashed, setTrashed] = useState<EventSummary[]>([]);
  const [showTrash, setShowTrash] = useState(false);
  // 編集/新規モーダル。null=閉じている。'new' は新規、それ以外は編集対象の予定。
  const [editing, setEditing] = useState<EventSummary | 'new' | null>(null);
  const todayStr = ymd(new Date());

  const period = useMemo(() => computePeriod(mode, anchor, i18n.language), [mode, anchor, i18n.language]);

  const reload = useCallback(() => {
    if (!isTauri) return;
    if (showTrash) {
      eventListTrashed().then(setTrashed).catch(() => setTrashed([]));
      return;
    }
    eventList(period.from, period.to).then(setEvents).catch(() => setEvents([]));
  }, [period.from, period.to, showTrash]);
  useEffect(reload, [reload]);

  // 予定が掛かる日の集合（年表示のドット。存在判定を O(1) に）。
  const eventDays = useMemo(() => new Set(events.flatMap(coveredDays)), [events]);

  // 曜日見出し（既知の日曜=2024-01-07 を起点に Intl で生成）。short=大グリッド, narrow=ミニ月。
  const weekdayShort = useMemo(
    () => weekdayLabels(i18n.language, 'short'),
    [i18n.language],
  );
  const weekdayNarrow = useMemo(
    () => weekdayLabels(i18n.language, 'narrow'),
    [i18n.language],
  );

  const gotoToday = () => {
    const now = new Date();
    setAnchor(now);
    setSelected(ymd(now));
  };
  const onSaved = () => {
    setEditing(null);
    reload();
  };
  const selectedList = eventsOn(events, selected);
  const maxVisible = mode === 'month' ? 3 : mode === 'fortnight' ? 5 : 12;

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
            {/* 表示単位の切替（年/月/2週/週） */}
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
                  onOpenDay={(ds) => {
                    setSelected(ds);
                    setEditing('new');
                  }}
                  onOpenMonth={(mm) => {
                    setAnchor(new Date(period.year, mm, 1));
                    setMode('month');
                  }}
                />
              ))}
            </div>
          </div>
          <AgendaPanel
            selected={selected}
            list={selectedList}
            locale={i18n.language}
            onOpen={setEditing}
            onNew={() => setEditing('new')}
          />
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
            <div
              className="grid min-h-0 flex-1 grid-cols-7"
              style={{ gridTemplateRows: `repeat(${period.rows}, minmax(0, 1fr))` }}
            >
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
                  onOpenNew={(ds) => {
                    setSelected(ds);
                    setEditing('new');
                  }}
                />
              ))}
            </div>
          </div>
          <AgendaPanel
            selected={selected}
            list={selectedList}
            locale={i18n.language}
            onOpen={setEditing}
            onNew={() => setEditing('new')}
          />
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

/** 選択日のアジェンダ（右側パネル。全モード共通）。 */
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
        {new Intl.DateTimeFormat(locale, { month: 'long', day: 'numeric', weekday: 'short' }).format(
          new Date(`${selected}T00:00`),
        )}
      </div>
      <div className="min-h-0 flex-1 space-y-1.5 overflow-y-auto p-2">
        {list.length === 0 ? (
          <p className="px-1 py-6 text-center text-sm text-white/45">{t('cal.noEvents')}</p>
        ) : (
          list.map((e) => (
            <button
              key={e.id}
              onClick={() => onOpen(e)}
              className="flex w-full items-start gap-2 rounded-lg p-2 text-left hover:bg-white/10"
            >
              <span className="mt-1 h-3 w-1 shrink-0 rounded-full" style={{ backgroundColor: e.color ?? DEFAULT_COLOR }} />
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
          <span className="h-3 w-1 shrink-0 rounded-full" style={{ backgroundColor: e.color ?? DEFAULT_COLOR }} />
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
