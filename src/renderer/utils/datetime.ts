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
