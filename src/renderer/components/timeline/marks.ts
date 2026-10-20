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
/** 添付のあるメールに付ける属性（つまみにクリップを出す） */
export const TIMELINE_ATTACH_ATTR = 'data-timeline-attach';
/** 本文に URL のあるメールに付ける属性（つまみに鎖を出す） */
export const TIMELINE_LINK_ATTR = 'data-timeline-link';

/** 目印の要素に付ける属性をまとめて作る（バブルと枠で付け方をそろえる）。 */
export function markAttrs(m: {
  date: string | null;
  quote?: boolean;
  attach?: boolean;
  link?: boolean;
}): Record<string, string> {
  const attrs: Record<string, string> = { [TIMELINE_DATE_ATTR]: m.date ?? '' };
  if (m.quote) attrs[TIMELINE_QUOTE_ATTR] = '';
  if (m.attach) attrs[TIMELINE_ATTACH_ATTR] = '';
  if (m.link) attrs[TIMELINE_LINK_ATTR] = '';
  return attrs;
}

/** 目盛りの横に出す目安のアイコン（添付・リンク） */
export interface MarkBadge {
  attach: boolean;
  link: boolean;
}

/** 目印 1 つ分（スクロール域の中身の先頭からの上端 px・日時・枠か） */
export interface TimelineMark {
  top: number;
  date: string | null;
  quote: boolean;
  attach: boolean;
  link: boolean;
}

/** つまみに渡すもの（並び・件数がそろっている） */
export interface TimelineMarks {
  days: DayCount[];
  tops: number[];
  /** i 件目が枠から来たか */
  quotes: boolean[];
  /** i 件目に添付・リンクがあるか */
  badges: MarkBadge[];
}

/** 目印の並びから、日ごとの件数・上端・枠かどうか・添付とリンクの有無を作る（日時の読めないものは直前の日に寄せる）。 */
export function summarizeMarks(marks: readonly TimelineMark[]): TimelineMarks {
  return {
    days: countByLocalDay(marks.map((m) => m.date)),
    tops: marks.map((m) => m.top),
    quotes: marks.map((m) => m.quote),
    badges: marks.map((m) => ({ attach: m.attach, link: m.link })),
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
      attach: el.hasAttribute(TIMELINE_ATTACH_ATTR),
      link: el.hasAttribute(TIMELINE_LINK_ATTR),
    })
  );
}
