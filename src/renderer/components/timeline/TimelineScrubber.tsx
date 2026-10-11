/**
 * 日付のつまみ（会話の左の縦の線）。
 *
 * Photosky の src/renderer/components/timeline/TimelineScrubber.tsx（962b00c）から写した。
 * 撮影日不明の目盛り・ラベルを落とし、色を Rondine の配色（白の濃淡＋sky）に合わせた。
 * 目盛りの位置の決め方を外から渡せるようにした（`starts` を渡すと各件の始まりの位置に置き、
 * 月日の札を常に出す。渡さなければ Photosky と同じ件数に比例）。
 * 設計: docs/CONVERSATION_TIMELINE.md
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { topVisibleIndex, type DayCount } from './dayCounts';
import {
  buildDayLabels,
  buildTicks,
  layoutSegments,
  layoutSegmentsAt,
  lensMagnification,
  offsetAtY,
  parentBands,
  segmentIndexAtY,
  snapToNearest,
  yOfOffset,
  type TickLevel,
} from './scrubberLayout';

interface TimelineScrubberProps {
  /** 日ごとの件数（一覧の並び順） */
  days: readonly DayCount[];
  /**
   * 各件が実際に始まる位置（全体に対する割合 0〜1・並び順に昇順）。渡すと目盛りをこの位置に置き、
   * 各日の始まりに月日の札を常に出す（会話用）。省略すると件数に比例して置き、マウスの近くだけ
   * 目盛りを細かくする（一覧用）
   */
  starts?: readonly number[];
  /** いま一覧の一番上に出ているのが何件目か（印を出す位置）。分からなければ null */
  viewOffset?: number | null;
  /** いま見えている範囲（全体に対する割合 0〜1）。渡すと viewOffset の印の代わりに範囲を出す */
  viewRange?: { top: number; bottom: number } | null;
  /**
   * 一覧の offset 件目へ飛ぶ（押したとき・引っ張っている間）。`fraction` は押した位置の
   * 線全体に対する割合（`starts` を渡したときは、その割合の位置へスクロールすればよい）
   */
  onJump: (offset: number, fraction: number) => void;
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

/** 札と点の間をあける（点の幅 w-1.5 ＋ 余白） */
const LABEL_RIGHT = 'right-5';
/** 札どうしの最小の間隔（px） */
const LABEL_MIN_GAP = 13;
/** マウスを点へ吸い付ける距離（px。札の高さの半分ほど） */
const SNAP_PX = 6;

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
export function TimelineScrubber({
  days,
  starts,
  viewOffset = null,
  viewRange = null,
  onJump,
}: TimelineScrubberProps) {
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

  // 位置に合わせる（会話）か、件数に比例（一覧）か
  const placed = starts !== undefined;
  const segments = useMemo(
    () => (starts ? layoutSegmentsAt(days, starts, height) : layoutSegments(days, height)),
    [days, starts, height]
  );
  // 位置に合わせるときは魚眼を使わない（札は常に出ているので拡大は要らず、引き伸ばすと
  // 点がバブルの本当の位置からずれる）
  const magnification = useMemo(
    () => (placed ? 0 : lensMagnification(segments)),
    [placed, segments]
  );
  const ticks = useMemo(
    () => (placed ? [] : buildTicks(segments, pointerY, height, magnification)),
    [placed, segments, pointerY, height, magnification]
  );
  const labels = useMemo(
    () => (placed ? buildDayLabels(segments, LABEL_MIN_GAP) : []),
    [placed, segments]
  );
  // 各件の点（位置に合わせるときだけ。札を間引いても点は全件ぶん出す）
  const dots = useMemo(
    () => (starts && segments.length > 0 ? starts.map((f) => f * height) : []),
    [starts, segments, height]
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
  // 札の書き方（「10/2」。年の変わり目は「2026/1/5」）
  const labelFormat = useMemo(
    () => ({
      day: new Intl.DateTimeFormat(i18n.language, { month: 'numeric', day: 'numeric' }),
      year: new Intl.DateTimeFormat(i18n.language, {
        year: 'numeric',
        month: 'numeric',
        day: 'numeric',
      }),
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
        if (py === null || segments.length === 0 || height <= 0) return;
        const fraction = py / height;
        const offset = starts ? (topVisibleIndex(starts, fraction) ?? 0) : offsetAtY(segments, py);
        onJump(offset, fraction);
      });
    },
    [segments, starts, height, onJump]
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
    const y = Math.min(rect.height, Math.max(0, e.clientY - rect.top));
    // 札は点を中心に置くので、札の上半分を押しても前の件にならないよう点へ吸い付ける
    return placed ? snapToNearest(dots, y, SNAP_PX) : y;
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
    viewRange === null && viewOffset !== null && segments.length > 0
      ? yOfOffset(segments, viewOffset)
      : null;

  return (
    <div
      // 位置に合わせるときは札（「2026/10/12」）が入る幅にする
      className={`relative h-full shrink-0 select-none border-r border-white/10 ${placed ? 'w-22' : 'w-16'}`}
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

        {/* 各件の始まりの点（位置に合わせるとき） */}
        {dots.map((y, i) => (
          <div
            key={`dot-${i}`}
            className="pointer-events-none absolute right-2 h-px w-1.5 bg-white/45"
            style={{ top: y }}
          />
        ))}

        {/* 各日の始まりの札（位置に合わせるとき。詰まったところは間引く） */}
        {labels.map((label, i) => (
          <div
            key={`label-${label.dateKey}-${i}`}
            className={`pointer-events-none absolute ${LABEL_RIGHT} -translate-y-1/2 whitespace-nowrap text-[10px] leading-none tabular-nums ${
              label.withYear ? 'font-bold text-white/85' : 'text-white/60'
            }`}
            style={{ top: label.y }}
          >
            {(label.withYear ? labelFormat.year : labelFormat.day).format(dateOf(label.dateKey))}
          </div>
        ))}

        {/* いま見えている範囲（スクロールの割合） */}
        {viewRange !== null && (
          <div
            className="pointer-events-none absolute right-0.5 w-1 rounded-full bg-sky-400/80"
            style={{
              top: viewRange.top * height,
              height: Math.max(4, (viewRange.bottom - viewRange.top) * height),
            }}
          />
        )}

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
