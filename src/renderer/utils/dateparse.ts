/**
 * メール本文中の「明示的な日付（＋時刻）」を検出して {day, time?, allDay} に正規化する。
 * 相対表現（明日・来週火曜）や期間（7/10〜7/12）は対象外（docs のスコープ確定事項）。
 *
 * 出力書式は既存のカレンダー実装に合わせる: day='YYYY-MM-DD' / time='HH:MM'。
 * 年が本文に無ければ baseISO（＝メール受信日）の年を採用する。
 */

export type ParsedDate = {
  /** 'YYYY-MM-DD'（ローカル）。 */
  day: string;
  /** 'HH:MM'（24時間）。終日なら undefined。 */
  time?: string;
  /** 時刻が無い＝終日。 */
  allDay: boolean;
};

const pad = (n: number) => String(n).padStart(2, '0');

/** 全角数字・全角コロンを半角へ。日付/時刻の解釈を簡単にする。 */
function toHalf(s: string): string {
  return s
    .replace(/[０-９]/g, (c) => String.fromCharCode(c.charCodeAt(0) - 0xfee0))
    .replace(/[：]/g, ':');
}

// 検出用の下位パターン（全角数字も許容）。半角化前の生テキストにマッチさせる。
const D = '[0-9０-９]';
const SEP = '[/／\\-]';
// 日付本体: 「YYYY年M月D日(曜)」または「(YYYY/)M/D」。
const JP_DATE = `(?:${D}{4}\\s*年\\s*)?${D}{1,2}\\s*月\\s*${D}{1,2}\\s*日(?:\\s*[（(][日月火水木金土][）)])?`;
const NUM_DATE = `(?:${D}{4}${SEP})?${D}{1,2}${SEP}${D}{1,2}`;
// 時刻（日付の直後に続くときだけ拾う）: 「HH:MM」「H時MM分」「午前/午後H時」など。
const AMPM = '(?:午前|午後|AM|PM|am|pm)';
const CLOCK = `${D}{1,2}\\s*:\\s*${D}{2}|${D}{1,2}\\s*時(?:\\s*${D}{1,2}\\s*分?)?`;
const TIME = `(?:\\s*(?:${AMPM}\\s*)?(?:${CLOCK}))`;

const DATE_RE = new RegExp(`(?:${JP_DATE}|${NUM_DATE})(?:${TIME})?`, 'g');

const DIGIT_OR_SEP = /[0-9０-９/／\-.]/;

/**
 * 本文の断片から、正しく解釈できる日付スパンだけを返す（前後が数字/区切りに連なる
 * ものは電話番号などの誤検出として除外）。index は s 内の開始位置。
 */
export function matchDates(text: string, baseISO?: string): { index: number; raw: string; parsed: ParsedDate }[] {
  const out: { index: number; raw: string; parsed: ParsedDate }[] = [];
  DATE_RE.lastIndex = 0;
  for (const m of text.matchAll(DATE_RE)) {
    const raw = m[0];
    const index = m.index ?? 0;
    const before = index > 0 ? text[index - 1] : '';
    const after = text[index + raw.length] ?? '';
    // 「03-1234-5678」のような長い数字列の一部を拾わない。
    if (before && DIGIT_OR_SEP.test(before)) continue;
    if (after && DIGIT_OR_SEP.test(after)) continue;
    const parsed = parseDateTime(raw, baseISO);
    if (parsed) out.push({ index, raw, parsed });
  }
  return out;
}

/** 生の日付文字列（例: '7月10日 14:00'）を ParsedDate に。解釈できなければ null。 */
export function parseDateTime(raw: string, baseISO?: string): ParsedDate | null {
  const s = toHalf(raw);
  let year: number | undefined;
  let month: number;
  let day: number;
  let rest: string;

  let m = s.match(/(?:(\d{4})\s*年\s*)?(\d{1,2})\s*月\s*(\d{1,2})\s*日/);
  if (m) {
    year = m[1] ? Number(m[1]) : undefined;
    month = Number(m[2]);
    day = Number(m[3]);
    rest = s.slice((m.index ?? 0) + m[0].length);
  } else if ((m = s.match(/(?:(\d{4})[/-])?(\d{1,2})[/-](\d{1,2})/))) {
    year = m[1] ? Number(m[1]) : undefined;
    month = Number(m[2]);
    day = Number(m[3]);
    rest = s.slice((m.index ?? 0) + m[0].length);
  } else {
    return null;
  }

  if (month < 1 || month > 12 || day < 1 || day > 31) return null;
  const y = year ?? baseYear(baseISO);
  // 2/31 などの繰り上がりを弾く（実在日だけ通す）。
  const probe = new Date(y, month - 1, day);
  if (probe.getFullYear() !== y || probe.getMonth() !== month - 1 || probe.getDate() !== day) {
    return null;
  }
  const dayStr = `${y}-${pad(month)}-${pad(day)}`;
  const time = parseTime(rest);
  return time ? { day: dayStr, time, allDay: false } : { day: dayStr, allDay: true };
}

/** baseISO の年（無ければ現在の年）。 */
function baseYear(baseISO?: string): number {
  if (baseISO) {
    const d = new Date(baseISO);
    if (!Number.isNaN(d.getTime())) return d.getFullYear();
  }
  return new Date().getFullYear();
}

/** 日付の直後の文字列から時刻（HH:MM）を取り出す。無ければ undefined。 */
function parseTime(after: string): string | undefined {
  const seg = toHalf(after);
  // 先頭（空白を挟んで）に来る時刻のみを対象にする。
  const ampm = /^\s*(午前|AM|am)/.test(seg) ? 'am' : /^\s*(午後|PM|pm)/.test(seg) ? 'pm' : '';
  // 先頭の空白と（あれば）午前/午後トークンを取り除く。
  const body = seg.replace(/^\s*(?:午前|午後|AM|PM|am|pm)?\s*/, '');

  let h: number;
  let min = 0;
  let m: RegExpMatchArray | null;
  if ((m = body.match(/^(\d{1,2}):(\d{2})/))) {
    h = Number(m[1]);
    min = Number(m[2]);
  } else if ((m = body.match(/^(\d{1,2})\s*時(?:\s*(\d{1,2})\s*分?)?/))) {
    h = Number(m[1]);
    min = m[2] ? Number(m[2]) : 0;
  } else {
    return undefined;
  }

  if (ampm === 'pm' && h < 12) h += 12;
  if (ampm === 'am' && h === 12) h = 0;
  if (h < 0 || h > 23 || min < 0 || min > 59) return undefined;
  return `${pad(h)}:${pad(min)}`;
}
