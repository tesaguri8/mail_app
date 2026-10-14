/**
 * 会話の日付のつまみに載せる要素の目印（バブルと、その下の未受信の引用の枠）。
 *
 * つまみは、この目印の付いた要素を DOM の並び順に測り、目盛り（位置）と日ごとの件数（日付）を
 * 同じ並び・同じ件数で作る（docs/CONVERSATION_TIMELINE.md §2）。枠は開閉で出たり消えたりする
 * ので、データからではなく描かれているものから数える。
 */
import { countByLocalDay, type DayCount } from './dayCounts';

/** 目印の属性（値は日時。読めなければ空文字） */
export const TIMELINE_DATE_ATTR = 'data-timeline-date';
/** 枠から来た目印に付ける属性（つまみで色を分ける） */
export const TIMELINE_QUOTE_ATTR = 'data-timeline-quote';

/** 目印 1 つ分（スクロール域の中身の先頭からの上端 px・日時・枠か） */
export interface TimelineMark {
  top: number;
  date: string | null;
  quote: boolean;
}

/** つまみに渡すもの（並び・件数がそろっている） */
export interface TimelineMarks {
  days: DayCount[];
  tops: number[];
  /** i 件目が枠から来たか */
  quotes: boolean[];
}

/** 目印の並びから、日ごとの件数・上端・枠かどうかを作る（日時の読めないものは直前の日に寄せる）。 */
export function summarizeMarks(marks: readonly TimelineMark[]): TimelineMarks {
  return {
    days: countByLocalDay(marks.map((m) => m.date)),
    tops: marks.map((m) => m.top),
    quotes: marks.map((m) => m.quote),
  };
}

/** スクロール域の中の目印を、DOM の並び順に読む。 */
export function readMarks(scroller: HTMLElement): TimelineMark[] {
  const base = scroller.getBoundingClientRect().top - scroller.scrollTop;
  return Array.from(scroller.querySelectorAll<HTMLElement>(`[${TIMELINE_DATE_ATTR}]`)).map(
    (el) => ({
      top: el.getBoundingClientRect().top - base,
      date: el.getAttribute(TIMELINE_DATE_ATTR) || null,
      quote: el.hasAttribute(TIMELINE_QUOTE_ATTR),
    })
  );
}
