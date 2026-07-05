import type { EventSummary } from '@bindings/EventSummary';

// 繰り返し（RRULE のサブセット）を端末ローカル日付で展開するユーティリティ。
// 対応: FREQ=DAILY | WEEKLY(+INTERVAL=2) | MONTHLY | YEARLY、任意で UNTIL=YYYYMMDD。
// 日時は素の ISO 文字列（終日='YYYY-MM-DD' / 時間指定='YYYY-MM-DDTHH:MM'）で保持し、
// 表示・リマインダーの両方でこの展開を共有する（バックエンドは繰り返し元をそのまま返す）。

/** UI が扱う繰り返しプリセット。 */
export type RecurPreset = 'none' | 'daily' | 'weekly' | 'biweekly' | 'monthly' | 'yearly';

export const RECUR_PRESETS: RecurPreset[] = ['none', 'daily', 'weekly', 'biweekly', 'monthly', 'yearly'];

type Freq = 'daily' | 'weekly' | 'monthly' | 'yearly';
interface Rule {
  freq: Freq;
  interval: number;
  until: string | null; // 'YYYY-MM-DD'
}

const DAY_MS = 86_400_000;
const pad = (n: number) => String(n).padStart(2, '0');
const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const dayOf = (iso: string) => iso.slice(0, 10);
const timeOf = (iso: string) => (iso.length > 10 ? iso.slice(11, 16) : '');
const at0 = (day: string) => new Date(`${day}T00:00`);
const addDays = (d: Date, n: number) => {
  const x = new Date(d);
  x.setDate(x.getDate() + n);
  return x;
};
const daysBetween = (a: string, b: string) => Math.round((at0(b).getTime() - at0(a).getTime()) / DAY_MS);

/** プリセット → RRULE 文字列（until は 'YYYY-MM-DD'）。none は null。 */
export function presetToRule(preset: RecurPreset, until: string | null): string | null {
  if (preset === 'none') return null;
  const base: Record<Exclude<RecurPreset, 'none'>, string> = {
    daily: 'FREQ=DAILY',
    weekly: 'FREQ=WEEKLY',
    biweekly: 'FREQ=WEEKLY;INTERVAL=2',
    monthly: 'FREQ=MONTHLY',
    yearly: 'FREQ=YEARLY',
  };
  let rule = base[preset];
  if (until) rule += `;UNTIL=${until.replace(/-/g, '')}`;
  return rule;
}

/** RRULE 文字列 → プリセット＋until（編集フォームの復元用）。 */
export function ruleToPreset(rrule: string | null): { preset: RecurPreset; until: string | null } {
  const parsed = parseRule(rrule);
  if (!parsed) return { preset: 'none', until: null };
  const preset: RecurPreset =
    parsed.freq === 'daily'
      ? 'daily'
      : parsed.freq === 'weekly'
        ? parsed.interval === 2
          ? 'biweekly'
          : 'weekly'
        : parsed.freq === 'monthly'
          ? 'monthly'
          : 'yearly';
  return { preset, until: parsed.until };
}

function parseRule(rrule: string | null): Rule | null {
  if (!rrule) return null;
  const parts = new Map<string, string>();
  for (const kv of rrule.split(';')) {
    const [k, v] = kv.split('=');
    if (k && v) parts.set(k.trim().toUpperCase(), v.trim());
  }
  const freqRaw = parts.get('FREQ');
  const freq: Freq | null =
    freqRaw === 'DAILY'
      ? 'daily'
      : freqRaw === 'WEEKLY'
        ? 'weekly'
        : freqRaw === 'MONTHLY'
          ? 'monthly'
          : freqRaw === 'YEARLY'
            ? 'yearly'
            : null;
  if (!freq) return null;
  const interval = Math.max(1, parseInt(parts.get('INTERVAL') ?? '1', 10) || 1);
  const untilRaw = parts.get('UNTIL');
  const until = untilRaw && untilRaw.length >= 8 ? `${untilRaw.slice(0, 4)}-${untilRaw.slice(4, 6)}-${untilRaw.slice(6, 8)}` : null;
  return { freq, interval, until };
}

function stepDate(d: Date, rule: Rule, n: number): Date {
  const x = new Date(d);
  if (rule.freq === 'daily') x.setDate(x.getDate() + n);
  else if (rule.freq === 'weekly') x.setDate(x.getDate() + 7 * rule.interval * n);
  else if (rule.freq === 'monthly') x.setMonth(x.getMonth() + n);
  else x.setFullYear(x.getFullYear() + n);
  return x;
}

/** 繰り返し予定 e を [from, to)（'YYYY-MM-DD'）に展開。各出現は同じ id を持つ複製。 */
function expandOne(e: EventSummary, from: string, to: string): EventSummary[] {
  const rule = parseRule(e.recurrence);
  if (!rule) return coversRange(e, from, to) ? [e] : [];

  const baseDay = dayOf(e.start_at);
  const startTime = e.all_day ? '' : timeOf(e.start_at);
  // 期間長（終日は日数、時間指定は終了日/時刻のオフセット）。
  const endDayOffset = e.end_at ? daysBetween(baseDay, dayOf(e.end_at)) : 0;
  const endTime = e.end_at && !e.all_day ? timeOf(e.end_at) : '';

  const toDate = at0(to);
  const untilDate = rule.until ? at0(rule.until) : null;
  const out: EventSummary[] = [];
  let cur = at0(baseDay);

  // 高頻度（日/週）は from 直前まで一気に早送りして無駄な反復を避ける。
  if (rule.freq === 'daily' || rule.freq === 'weekly') {
    const stepDays = rule.freq === 'daily' ? 1 : 7 * rule.interval;
    const gap = Math.floor((at0(from).getTime() - cur.getTime()) / DAY_MS);
    if (gap > stepDays) cur = addDays(cur, (Math.floor(gap / stepDays) - 1) * stepDays);
  } else {
    // 月/年は範囲手前まで単位で早送り（反復数は小さい）。
    let guard = 0;
    while (stepDate(cur, rule, 1) < at0(from) && guard < 1200) {
      cur = stepDate(cur, rule, 1);
      guard++;
    }
  }

  let guard = 0;
  while (cur < toDate && guard < 1000) {
    guard++;
    if (untilDate && cur > untilDate) break;
    const occDay = ymd(cur);
    const start_at = e.all_day ? occDay : `${occDay}T${startTime}`;
    let end_at: string | null = null;
    if (e.end_at) {
      const endDay = ymd(addDays(cur, endDayOffset));
      end_at = e.all_day ? (endDayOffset > 0 ? endDay : null) : `${endDay}T${endTime}`;
    }
    const occEndDay = end_at ? dayOf(end_at) : occDay;
    if (occEndDay >= from) out.push({ ...e, start_at, end_at });
    cur = stepDate(cur, rule, 1);
  }
  return out;
}

/** 単発が [from, to) に重なるか。 */
function coversRange(e: EventSummary, from: string, to: string): boolean {
  const start = dayOf(e.start_at);
  const end = e.end_at ? dayOf(e.end_at) : start;
  return start < to && end >= from;
}

/**
 * バックエンドの行（単発＋繰り返し元）を [from, to) の実出現に展開する。
 * 単発はそのまま、繰り返しは複数の出現（同一 id）に広げて返す。
 */
export function expandEvents(rows: EventSummary[], from: string, to: string): EventSummary[] {
  return rows.flatMap((e) => expandOne(e, from, to));
}
