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
 * 引用の見出しから読んだ日時（`UnseenQuote.date`）を、バブルと同じ表記で整形する。
 * 時差が付いていれば端末の時刻に直す。時差が無い（元の見出しに書かれていない）ものは、
 * 書かれたままの時刻として出す（端末の時刻とみなして組み立てるので、ずらさない）。
 * 読めなければ `fallback`（見出しの元の文字列）を返す。
 */
export function formatQuotedDateTime(iso: string, fallback: string): string {
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
  return isNaN(dt.getTime()) ? fallback : formatDateTime(dt.toISOString());
}
