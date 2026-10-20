// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest';
import { markAttrs, readMarks, summarizeMarks } from './marks';

const none = { attach: false, link: false };

// 端末のタイムゾーンに左右されないよう、日時はオフセット無し（ローカル時刻）で書く。
describe('summarizeMarks', () => {
  it('バブルと枠を並び順のまま数え、件数・上端・枠かどうかの並びをそろえる', () => {
    const got = summarizeMarks([
      { top: 0, date: '2026-10-12T10:00:00', quote: false, attach: true, link: false },
      { top: 120, date: '2026-10-09T15:29:01', quote: true, attach: false, link: true },
      { top: 200, date: '2026-10-07T19:36:00', quote: true, ...none },
      { top: 260, date: null, quote: true, ...none },
      { top: 300, date: '2026-10-02T14:50:00', quote: true, ...none },
    ]);
    expect(got.days).toEqual([
      { dateKey: '2026-10-12', count: 1 },
      { dateKey: '2026-10-09', count: 1 },
      // 日時の読めない件は直前の日に寄せる（件数は落とさない）
      { dateKey: '2026-10-07', count: 2 },
      { dateKey: '2026-10-02', count: 1 },
    ]);
    expect(got.tops).toEqual([0, 120, 200, 260, 300]);
    expect(got.quotes).toEqual([false, true, true, true, true]);
    expect(got.badges).toEqual([
      { attach: true, link: false },
      { attach: false, link: true },
      none,
      none,
      none,
    ]);
    expect(got.days.reduce((n, d) => n + d.count, 0)).toBe(got.tops.length);
  });
});

describe('readMarks', () => {
  it('markAttrs で付けた日時・枠・添付・リンクの印を、DOM の並び順に読む', () => {
    const scroller = document.createElement('div');
    const put = (attrs: Record<string, string>) => {
      const el = document.createElement('div');
      Object.entries(attrs).forEach(([k, v]) => el.setAttribute(k, v));
      scroller.appendChild(el);
    };
    put(markAttrs({ date: '2026-10-12T10:00:00', attach: true, link: true }));
    put(markAttrs({ date: '2026-10-09T15:29:01', quote: true, link: true }));
    put(markAttrs({ date: null }));
    expect(
      readMarks(scroller).map(({ date, quote, attach, link }) => ({ date, quote, attach, link }))
    ).toEqual([
      { date: '2026-10-12T10:00:00', quote: false, attach: true, link: true },
      { date: '2026-10-09T15:29:01', quote: true, attach: false, link: true },
      { date: null, quote: false, attach: false, link: false },
    ]);
  });
});
