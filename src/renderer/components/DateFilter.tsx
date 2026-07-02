import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CalendarDays } from 'lucide-react';

export type DateMode = 'after' | 'before' | 'range';
export type DateRange = { mode: DateMode; start: string; end: string };

/** メールの日付が指定範囲に合致するか（日単位、両端含む）。 */
export function matchesDate(dateStr: string | null, df: DateRange | null): boolean {
  if (!df) return true;
  if (!dateStr) return false;
  const d = new Date(dateStr);
  if (isNaN(d.getTime())) return false;
  const startOk = df.start ? d >= new Date(`${df.start}T00:00:00`) : true;
  const endOk = df.end ? d <= new Date(`${df.end}T23:59:59.999`) : true;
  if (df.mode === 'after') return df.start ? startOk : true;
  if (df.mode === 'before') return df.end ? endOk : true;
  return startOk && endOk;
}

const inputCls =
  'rounded-md border border-white/15 bg-white/10 px-2 py-1 text-xs text-white outline-none focus:bg-white/20 [color-scheme:dark]';

/** ローカル日付を YYYY-MM-DD に整形（UTC ずれを避ける）。 */
function fmt(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${y}-${m}-${day}`;
}
function daysAgo(n: number): string {
  const d = new Date();
  d.setDate(d.getDate() - n);
  return fmt(d);
}
function monthStart(offset: number): string {
  const t = new Date();
  return fmt(new Date(t.getFullYear(), t.getMonth() + offset, 1));
}
function monthEnd(offset: number): string {
  const t = new Date();
  return fmt(new Date(t.getFullYear(), t.getMonth() + offset + 1, 0));
}

/**
 * 期間フィルタ（カレンダーアイコン＋ポップオーバー）。
 * 以降 / 以前 / 期間 を選び、日付を指定して絞り込む。
 */
export function DateFilter({
  value,
  onChange,
}: {
  value: DateRange | null;
  onChange: (v: DateRange | null) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const [mode, setMode] = useState<DateMode>(value?.mode ?? 'after');
  const [start, setStart] = useState(value?.start ?? '');
  const [end, setEnd] = useState(value?.end ?? '');

  useEffect(() => {
    if (!open) return;
    setMode(value?.mode ?? 'after');
    setStart(value?.start ?? '');
    setEnd(value?.end ?? '');
  }, [open, value]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    return () => document.removeEventListener('mousedown', onDoc);
  }, [open]);

  const on = value !== null;
  const today = fmt(new Date());

  // ワンクリックで過去日を入れるプリセット（モードに応じて start/end を決定）
  type Preset = { key: string; start: string; end: string };
  const presets: Preset[] =
    mode === 'range'
      ? [
          { key: 'today', start: today, end: today },
          { key: 'last7', start: daysAgo(6), end: today },
          { key: 'last30', start: daysAgo(29), end: today },
          { key: 'thisMonth', start: monthStart(0), end: today },
          { key: 'lastMonth', start: monthStart(-1), end: monthEnd(-1) },
        ]
      : (['today', 'yesterday', 'd7', 'd30', 'd90'] as const).map((key) => {
          const v =
            key === 'today'
              ? today
              : key === 'yesterday'
                ? daysAgo(1)
                : daysAgo(Number(key.slice(1)));
          return mode === 'after'
            ? { key, start: v, end: '' }
            : { key, start: '', end: v };
        });

  const applyPreset = (p: Preset) => {
    setStart(p.start);
    setEnd(p.end);
    onChange({ mode, start: p.start, end: p.end });
    setOpen(false);
  };

  const apply = () => {
    if ((mode === 'after' && !start) || (mode === 'before' && !end) || (mode === 'range' && !start && !end)) {
      onChange(null);
    } else {
      onChange({ mode, start, end });
    }
    setOpen(false);
  };
  const clear = () => {
    onChange(null);
    setOpen(false);
  };

  const MODES: DateMode[] = ['after', 'before', 'range'];

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        title={t('date.filter')}
        aria-label={t('date.filter')}
        aria-pressed={on}
        className={`flex h-8 w-8 items-center justify-center rounded-md ${
          on
            ? 'bg-sky-500/30 text-sky-200 ring-1 ring-sky-300/40'
            : 'text-white/55 hover:text-white/80'
        }`}
      >
        <CalendarDays size={15} />
      </button>

      {/* アイコンの左下を起点に展開（コンテンツ側への重なりは許容。親の overflow-hidden は外してある） */}
      {open && (
        <div className="absolute left-0 top-full z-30 mt-1 w-60 rounded-md border border-white/15 bg-neutral-900/65 p-3 shadow-xl backdrop-blur">
          <div className="mb-2 flex gap-1">
            {MODES.map((m) => (
              <button
                key={m}
                onClick={() => setMode(m)}
                className={`flex-1 rounded px-2 py-1 text-xs ${
                  mode === m ? 'bg-sky-500/30 text-sky-200' : 'bg-white/10 text-white/70 hover:bg-white/15'
                }`}
              >
                {t(`date.${m}`)}
              </button>
            ))}
          </div>

          {/* 過去日のクイック入力 */}
          <div className="mb-2 flex flex-wrap gap-1">
            {presets.map((p) => (
              <button
                key={p.key}
                onClick={() => applyPreset(p)}
                className="rounded bg-white/10 px-2 py-0.5 text-[11px] text-white/75 hover:bg-white/20 hover:text-white"
              >
                {t(`date.${p.key}`)}
              </button>
            ))}
          </div>

          <div className="space-y-2">
            {(mode === 'after' || mode === 'range') && (
              <label className="flex items-center justify-between gap-2 text-xs text-white/55">
                <span className="shrink-0">{t('date.start')}</span>
                <input
                  type="date"
                  max={today}
                  className={inputCls}
                  value={start}
                  onChange={(e) => setStart(e.target.value)}
                />
              </label>
            )}
            {(mode === 'before' || mode === 'range') && (
              <label className="flex items-center justify-between gap-2 text-xs text-white/55">
                <span className="shrink-0">{t('date.end')}</span>
                <input
                  type="date"
                  max={today}
                  className={inputCls}
                  value={end}
                  onChange={(e) => setEnd(e.target.value)}
                />
              </label>
            )}
          </div>

          <div className="mt-3 flex justify-between">
            <button
              onClick={clear}
              className="rounded px-2 py-1 text-xs text-white/55 hover:text-white/80"
            >
              {t('date.clear')}
            </button>
            <button
              onClick={apply}
              className="rounded bg-white/15 px-3 py-1 text-xs hover:bg-white/25"
            >
              {t('date.apply')}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
