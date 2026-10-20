/**
 * メールの日時をロケール表記で整形する（秒は省く）。
 * メール一覧・会話バブル・全文表示ヘッダで共通利用する（表記を1か所に集約）。
 * 無効値・パース不能な文字列は元の文字列をそのまま返す。
 */
export function formatDateTime(d: string | null): string {
  if (!d) return '';
  const dt = new Date(d);
  return isNaN(dt.getTime())
    ? d
    : dt.toLocaleString(undefined, {
        year: 'numeric',
        month: 'numeric',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
      });
}

/** 時差の書かれていない ISO 8601 の日時（"2026-10-15T11:21:00"）。 */
const NAIVE_ISO_RE = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/;

/**
 * 引用の見出しから読んだ日時（`UnseenQuote.date`。ISO 8601）を Date にする。
 * 時差が付いていればその時刻、時差が無い（元の見出しに書かれていない）ものは、
 * 書かれたままの時刻を端末の時刻とみなす（ずらさない）。読めなければ null。
 */
export function parseQuotedDate(iso: string): Date | null {
  const naive = NAIVE_ISO_RE.exec(iso);
  const dt = naive
    ? new Date(
        Number(naive[1]),
        Number(naive[2]) - 1,
        Number(naive[3]),
        Number(naive[4]),
        Number(naive[5]),
        Number(naive[6] ?? 0)
      )
    : new Date(iso);
  return isNaN(dt.getTime()) ? null : dt;
}

/**
 * 引用の見出しから読んだ日時を、バブルと同じ表記で整形する（`parseQuotedDate` の扱いのまま）。
 * 読めなければ `fallback`（見出しの元の文字列）を返す。
 */
export function formatQuotedDateTime(iso: string, fallback: string): string {
  const dt = parseQuotedDate(iso);
  return dt ? formatDateTime(dt.toISOString()) : fallback;
}

const pad2 = (n: number) => String(n).padStart(2, '0');
const JA_WEEKDAYS = ['日', '月', '火', '水', '木', '金', '土'];
const EN_WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const EN_MONTHS = [
  'Jan',
  'Feb',
  'Mar',
  'Apr',
  'May',
  'Jun',
  'Jul',
  'Aug',
  'Sep',
  'Oct',
  'Nov',
  'Dec',
];

/** 端末の時差を "+0900" / "-0500" の形で返す（その日時の時差。夏時間を含む）。 */
function zoneText(dt: Date): string {
  const minutes = -dt.getTimezoneOffset();
  const abs = Math.abs(minutes);
  return `${minutes < 0 ? '-' : '+'}${pad2(Math.floor(abs / 60))}${pad2(abs % 60)}`;
}

/**
 * 返信の引用見出し・転送の見出しに書く日時を組み立てる。日時は端末の時刻に直し、端末の時差を添える
 * （元のメールの時差ではない）。曜日・月名は言語に合わせる。
 * - 日本語: "2026/10/14(水) 12:50 +0900"
 * - それ以外（英語）: Gmail と同じ "Wed, Oct 14, 2026 at 12:50 PM +0900"
 *
 * この形は Rondine の引用の解析（src-tauri の quotes）が読める形で、往復の試験で固定している。
 * 読めない日時は元の文字列をそのまま返す。
 */
export function formatQuoteHeaderDate(d: string | null, lang: string): string {
  if (!d) return '';
  const dt = new Date(d);
  if (isNaN(dt.getTime())) return d;
  const [y, mo, day, w] = [dt.getFullYear(), dt.getMonth(), dt.getDate(), dt.getDay()];
  const [h, mi] = [dt.getHours(), pad2(dt.getMinutes())];
  const zone = zoneText(dt);
  if (lang.startsWith('ja')) {
    return `${y}/${pad2(mo + 1)}/${pad2(day)}(${JA_WEEKDAYS[w]}) ${pad2(h)}:${mi} ${zone}`;
  }
  const ampm = h < 12 ? 'AM' : 'PM';
  return `${EN_WEEKDAYS[w]}, ${EN_MONTHS[mo]} ${day}, ${y} at ${h % 12 || 12}:${mi} ${ampm} ${zone}`;
}
