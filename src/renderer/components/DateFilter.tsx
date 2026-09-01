import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from './Tooltip';
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
  'rounded-md border border-white/15 bg-white/10 px-2 py-1 text-xs text-white outline-none focus:bg-white/20';

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

/**
 * 日付フィルタ（カレンダーアイコン＋ポップオーバー）。
 * - 今日／昨日: その日だけ（単日）。
 * - 「この日以前」: 選んだ日から古い方（on/before）。
 * - 期間: 開始〜終了の範囲内。
 * 見出しをドラッグで移動でき、編集は即時反映（適用ボタンなし）。
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
  // ドラッグで移動した位置（未移動は null＝アイコン直下）。開くたびに戻す。
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  // 「この日以前」の単日。
  const [before, setBefore] = useState('');
  // 「期間」の開始／終了。
  const [start, setStart] = useState('');
  const [end, setEnd] = useState('');

  // 開くたびに現在値から各入力を復元する（単日＝期間 start=end もそのまま期間欄に出す）。
  useEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    const isRange = value?.mode === 'range';
    setBefore(value?.mode === 'before' ? (value.end ?? '') : '');
    setStart(isRange ? (value?.start ?? '') : '');
    setEnd(isRange ? (value?.end ?? '') : '');
  }, [open, value]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    return () => document.removeEventListener('mousedown', onDoc);
  }, [open]);

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

  const on = value !== null;
  const today = fmt(new Date());
  const yesterday = daysAgo(1);
  // その日だけ（開始=終了=その日）が選ばれているか（今日／昨日ボタンのハイライト用）。
  const isDay = (day: string) =>
    value?.mode === 'range' && value.start === day && value.end === day;

  // 今日／昨日: その日だけ。期間欄にも反映して「開始=終了=その日」を見せる。
  const applyDay = (day: string) => {
    setBefore('');
    setStart(day);
    setEnd(day);
    onChange({ mode: 'range', start: day, end: day });
  };
  // この日以前（その日から古い方）。
  const changeBefore = (d: string) => {
    setStart('');
    setEnd('');
    setBefore(d);
    onChange(d ? { mode: 'before', start: '', end: d } : null);
  };
  // 期間: 開始／終了を編集（片方だけでも可）。「この日以前」とは排他なので before は消す。
  const changeStart = (s: string) => {
    setBefore('');
    setStart(s);
    onChange(s || end ? { mode: 'range', start: s, end } : null);
  };
  const changeEnd = (e: string) => {
    setBefore('');
    setEnd(e);
    onChange(start || e ? { mode: 'range', start, end: e } : null);
  };
  const clear = () => {
    setBefore('');
    setStart('');
    setEnd('');
    onChange(null);
  };

  const dayBtn = (active: boolean) =>
    `flex-1 rounded px-2 py-1 text-xs ${
      active ? 'bg-sky-500/30 text-sky-200' : 'bg-white/10 text-white/70 hover:bg-white/15'
    }`;

  return (
    <div ref={ref} className="relative">
      <Tooltip label={t('date.filter')}>
        <button
          onClick={() => setOpen((v) => !v)}
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
      </Tooltip>

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
          {/* 見出し（ドラッグの取っ手）＋閉じる。編集は即時反映（適用ボタンなし）。 */}
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

          {/* 単日クイック（その日だけ） */}
          <div className="mb-3 flex gap-1">
            <button onClick={() => applyDay(today)} className={dayBtn(isDay(today))}>
              {t('date.today')}
            </button>
            <button onClick={() => applyDay(yesterday)} className={dayBtn(isDay(yesterday))}>
              {t('date.yesterday')}
            </button>
          </div>

          {/* この日以前（その日から古い方） */}
          <label className="mb-3 flex items-center justify-between gap-2 text-xs text-white/55">
            <span className="shrink-0">{t('date.orBefore')}</span>
            <input
              type="date"
              max={today}
              className={inputCls}
              value={before}
              onChange={(e) => changeBefore(e.target.value)}
            />
          </label>

          {/* 期間（開始〜終了） */}
          <div className="space-y-1.5 border-t border-white/10 pt-2">
            <div className="text-xs text-white/45">{t('date.range')}</div>
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
