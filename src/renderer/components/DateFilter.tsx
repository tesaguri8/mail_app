import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CalendarDays, GripHorizontal, X } from 'lucide-react';

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
  const panelRef = useRef<HTMLDivElement>(null);
  const [mode, setMode] = useState<DateMode>(value?.mode ?? 'after');
  const [start, setStart] = useState(value?.start ?? '');
  const [end, setEnd] = useState(value?.end ?? '');
  // ドラッグで移動した位置（未移動は null＝アイコン直下に表示）。開くたびに戻す。
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    setMode(value?.mode ?? 'after');
    setStart(value?.start ?? '');
    setEnd(value?.end ?? '');
  }, [open, value]);

  // 見出し（グリップ）をつかんでポップオーバーを移動する。一覧に被って邪魔なときに避けられる。
  const onDragStart = (e: React.MouseEvent) => {
    e.preventDefault();
    const panel = panelRef.current;
    if (!panel) return;
    const rect = panel.getBoundingClientRect();
    const offX = e.clientX - rect.left;
    const offY = e.clientY - rect.top;
    const w = rect.width;
    const h = rect.height;
    const onMove = (ev: MouseEvent) => {
      const maxX = Math.max(4, window.innerWidth - w - 4);
      const maxY = Math.max(4, window.innerHeight - h - 4);
      setPos({
        x: Math.min(Math.max(4, ev.clientX - offX), maxX),
        y: Math.min(Math.max(4, ev.clientY - offY), maxY),
      });
    };
    const onUp = () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
    };
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  };

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

  // 即時反映: 必須の日付が空なら解除(null)、あれば設定。適用ボタンなしで編集がそのまま効く。
  const applyLive = (m: DateMode, s: string, e: string) => {
    const empty = (m === 'after' && !s) || (m === 'before' && !e) || (m === 'range' && !s && !e);
    onChange(empty ? null : { mode: m, start: s, end: e });
  };
  const applyPreset = (p: Preset) => {
    // クリックで即適用するが、続けて微調整できるようポップオーバーは閉じない。
    setStart(p.start);
    setEnd(p.end);
    applyLive(mode, p.start, p.end);
  };
  const changeMode = (m: DateMode) => {
    setMode(m);
    applyLive(m, start, end);
  };
  const changeStart = (s: string) => {
    setStart(s);
    applyLive(mode, s, end);
  };
  const changeEnd = (e: string) => {
    setEnd(e);
    applyLive(mode, start, e);
  };
  const clear = () => {
    setStart('');
    setEnd('');
    onChange(null);
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

      {/* 既定はアイコン直下に展開。見出しをドラッグすると position:fixed で自由に移動でき、
          一覧に被って邪魔なときに避けられる（親の overflow に切られないよう fixed にする）。 */}
      {open && (
        <div
          ref={panelRef}
          style={pos ? { left: pos.x, top: pos.y } : undefined}
          className={`${
            pos ? 'fixed' : 'absolute left-0 top-full mt-1'
          } z-30 w-60 rounded-md border border-white/15 bg-neutral-900/65 p-3 shadow-xl backdrop-blur`}
        >
          {/* 見出し（ドラッグの取っ手）＋閉じる。適用ボタンは廃止し、編集は即時反映する。 */}
          <div
            onMouseDown={onDragStart}
            className="mb-2 flex cursor-move select-none items-center justify-between border-b border-white/10 pb-2 text-xs text-white/60"
          >
            <span className="flex items-center gap-1.5">
              <GripHorizontal size={13} className="text-white/35" />
              {t('date.filter')}
            </span>
            <button
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => setOpen(false)}
              title={t('date.close')}
              aria-label={t('date.close')}
              className="flex h-5 w-5 items-center justify-center rounded hover:bg-white/15 hover:text-white/90"
            >
              <X size={13} />
            </button>
          </div>
          <div className="mb-2 flex gap-1">
            {MODES.map((m) => (
              <button
                key={m}
                onClick={() => changeMode(m)}
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
                  onChange={(e) => changeStart(e.target.value)}
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
                  onChange={(e) => changeEnd(e.target.value)}
                />
              </label>
            )}
          </div>

          <div className="mt-3 flex justify-end">
            <button
              onClick={clear}
              className="rounded px-2 py-1 text-xs text-white/55 hover:text-white/80"
            >
              {t('date.clear')}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
