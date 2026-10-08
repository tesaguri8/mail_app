// 自動同期で Google の連絡先も同期するかの判断（カレンダーは毎回、連絡先は間隔を空ける）。
//
// 連絡先の同期は件数が多いと重い（ラベル・全件の差分・住所録への反映）ので、自動同期の
// 間隔（既定 30 秒）では回さない。起動して最初の 1 回と、前回から間が空いたときだけ回す。

import type { GoogleAccount } from '@bindings/GoogleAccount';

/** 自動同期で連絡先を同期する最小の間隔（ミリ秒）。 */
export const CONTACTS_AUTO_SYNC_MS = 10 * 60 * 1000;

/** SQLite の CURRENT_TIMESTAMP（'YYYY-MM-DD HH:MM:SS'・UTC）を時刻（ミリ秒）へ。読めなければ null。 */
const parseUtc = (s: string): number | null => {
  const ms = Date.parse(s.replace(' ', 'T') + 'Z');
  return Number.isNaN(ms) ? null : ms;
};

/**
 * この自動同期で連絡先も同期するか。
 * `syncedThisRun` は、アプリを起動してからこのアカウントの連絡先を同期したか。
 */
export function contactsDue(a: GoogleAccount, now: number, syncedThisRun: boolean): boolean {
  if (!a.sync_contacts || a.disconnected_at != null) return false;
  if (!syncedThisRun) return true;
  const last = a.last_contacts_sync_at ? parseUtc(a.last_contacts_sync_at) : null;
  return last === null || now - last >= CONTACTS_AUTO_SYNC_MS;
}
