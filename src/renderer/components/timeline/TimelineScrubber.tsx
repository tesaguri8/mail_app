/**
 * 日付のつまみ（会話の左の縦の線）。
 *
 * Photosky の src/renderer/components/timeline/TimelineScrubber.tsx（962b00c）から写した。
 * 撮影日不明の目盛り・ラベルを落とし、色を Rondine の配色（白の濃淡＋sky）に合わせた。
 * 設計: docs/CONVERSATION_TIMELINE.md
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DayCount } from './dayCounts';
import {
  buildTicks,
  layoutSegments,
  lensMagnification,
  offsetAtY,
  parentBands,
  segmentIndexAtY,
  yOfOffset,
  type TickLevel,
} from './scrubberLayout';

interface TimelineScrubberProps {
  /** 日ごとの件数（一覧の並び順） */
  days: readonly DayCount[];
  /** いま一覧の一番上に出ているのが何件目か（印を出す位置）。分からなければ null */
  viewOffset: number | null;
  /** 一覧の offset 件目へ飛ぶ（押したとき・引っ張っている間） */
  onJump: (offset: number) => void;
}

/** 線の上下の余白（px。端の目盛りの文字が切れないように） */
const PAD_Y = 12;

/** `YYYY-MM-DD` をローカル日付の 0 時として Date に戻す */
function dateOf(key: string): Date {
  const [y, m, d] = key.split('-').map(Number);
  return new Date(y, m - 1, d);
}

/** 目盛りの数字の見た目（数字だけでも年・月・日が見分けられるよう、大きさ・太さ・濃さに差を付ける） */
const TICK_TEXT: Record<TickLevel, string> = {
  year: 'text-[13px] font-bold text-white/85',
  month: 'text-[12px] font-semibold text-white/65',
  week: 'text-[9px] text-white/45',
  day: 'text-[9px] text-white/45',
};

/** 目盛りの横棒の長さ */
const TICK_BAR: Record<TickLevel, string> = {
  year: 'w-3',
  month: 'w-2',
  week: 'w-1',
  day: 'w-1',
};

/**
 * 日付のつまみ。最初〜最後（並び順どおり）を件数に比例して並べ、マウスの近くだけ目盛りを
 * 細かくする。押す・引っ張るとその日付へ飛ぶ。
 */
export function TimelineScrubber({ days, viewOffset, onJump }: TimelineScrubberProps) {
  const { t, i18n } = useTranslation();
  const trackRef = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(0);
  const [pointerY, setPointerY] = useState<number | null>(null);
  const [dragging, setDragging] = useState(false);

  // 線の高さを監視
  useEffect(() => {
    const el = trackRef.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) setHeight(entry.contentRect.height);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const segments = useMemo(() => layoutSegments(days, height), [days, height]);
  const magnification = useMemo(() => lensMagnification(segments), [segments]);
  const ticks = useMemo(
    () => buildTicks(segments, pointerY, height, magnification),
    [segments, pointerY, height, magnification]
  );
  // マウス位置の日を含む年・月の帯（いまどの年・月の中かを見せる）
  const bands = useMemo(
    () => parentBands(segments, pointerY, height, magnification),
    [segments, pointerY, height, magnification]
  );

  // マウス位置の日付の書き方（言語に合わせる）
  const fullFormat = useMemo(
    () =>
      new Intl.DateTimeFormat(i18n.language, {
        year: 'numeric',
        month: 'long',
        day: 'numeric',
        weekday: 'short',
      }),
    [i18n.language]
  );

  // 目盛りは数字だけ（「年」「月」「日」を付けない方がすっきりする）
  const tickLabel = (level: TickLevel, dateKey: string): string => {
    const d = dateOf(dateKey);
    if (level === 'year') return String(d.getFullYear());
    if (level === 'month') return String(d.getMonth() + 1);
    return String(d.getDate());
  };

  const hovered =
    pointerY !== null && segments.length > 0 ? segments[segmentIndexAtY(segments, pointerY)] : null;
  const hoveredLabel = hovered ? fullFormat.format(dateOf(hovered.dateKey)) : null;

  // 引っ張っている間は、描画 1 回につき 1 回だけ飛ぶ
  const frameRef = useRef<number | null>(null);
  const pendingYRef = useRef<number | null>(null);
  const jumpTo = useCallback(
    (y: number) => {
      pendingYRef.current = y;
      if (frameRef.current !== null) return;
      frameRef.current = requestAnimationFrame(() => {
        frameRef.current = null;
        const py = pendingYRef.current;
        if (py !== null && segments.length > 0) onJump(offsetAtY(segments, py));
      });
    },
    [segments, onJump]
  );
  useEffect(
    () => () => {
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    },
    []
  );

  const localY = (e: React.PointerEvent) => {
    const rect = trackRef.current?.getBoundingClientRect();
    if (!rect) return 0;
    return Math.min(rect.height, Math.max(0, e.clientY - rect.top));
  };

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragging(true);
    const y = localY(e);
    setPointerY(y);
    jumpTo(y);
  };
  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const y = localY(e);
    setPointerY(y);
    if (dragging) jumpTo(y);
  };
  const handlePointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    setDragging(false);
  };
  const handlePointerLeave = () => {
    if (!dragging) setPointerY(null);
  };

  if (days.length === 0) return null;

  const markerY =
    viewOffset !== null && segments.length > 0 ? yOfOffset(segments, viewOffset) : null;

  return (
    <div
      className="relative h-full w-16 shrink-0 select-none border-r border-white/10"
      role="navigation"
      aria-label={t('timeline.label')}
    >
      <div
        ref={trackRef}
        className={`absolute inset-x-0 touch-none ${dragging ? 'cursor-grabbing' : 'cursor-row-resize'}`}
        style={{ top: PAD_Y, bottom: PAD_Y }}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
        onPointerLeave={handlePointerLeave}
        onClick={(e) => e.stopPropagation()}
      >
        {/* マウス位置の日を含む年（薄い）と月（もう一段濃い）の帯 */}
        {bands && (
          <>
            <div
              className="pointer-events-none absolute inset-x-0 rounded-sm bg-sky-300/10"
              style={{ top: bands.year.y0, height: Math.max(1, bands.year.y1 - bands.year.y0) }}
            />
            <div
              className="pointer-events-none absolute inset-x-0 rounded-sm bg-sky-300/15"
              style={{ top: bands.month.y0, height: Math.max(1, bands.month.y1 - bands.month.y0) }}
            />
          </>
        )}

        {/* 目盛り（同じ日が並びの中で離れて二度現れても key が重ならないよう、番号を足す） */}
        {ticks.map((tick, i) => (
          <div
            key={`${tick.level}-${tick.dateKey}-${i}`}
            className="pointer-events-none absolute right-2 flex -translate-y-1/2 items-center gap-1"
            style={{ top: tick.y }}
          >
            <span className={`whitespace-nowrap leading-none ${TICK_TEXT[tick.level]}`}>
              {tickLabel(tick.level, tick.dateKey)}
            </span>
            <span className={`h-px bg-white/35 ${TICK_BAR[tick.level]}`} />
          </div>
        ))}

        {/* いま見ている位置 */}
        {markerY !== null && (
          <div
            className="pointer-events-none absolute right-0.5 h-1.5 w-3 -translate-y-1/2 rounded-sm bg-sky-400"
            style={{ top: markerY }}
          />
        )}

        {/* マウスの位置の日付（引っ張っている間も出し続ける） */}
        {pointerY !== null && hoveredLabel && (
          <>
            <div
              className="pointer-events-none absolute inset-x-0 h-px bg-sky-300"
              style={{ top: pointerY }}
            />
            <div
              className="pointer-events-none absolute left-full z-30 ml-2 -translate-y-1/2 whitespace-nowrap rounded-md border border-white/15 bg-neutral-900/95 px-3 py-1.5 text-sm font-semibold text-white/90 shadow-lg"
              style={{ top: pointerY }}
            >
              {hoveredLabel}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
