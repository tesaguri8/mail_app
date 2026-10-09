// Google 同期（カレンダー＋連絡先＋住所録への反映）の結果を、1 つの文にまとめる。
//
// 0 の項目は省く（毎回「送信 0 件」と並べても読みにくい）。何も変わらなかった種類は
// 「変更なし」とだけ出す。種類ごとの失敗は呼び出し側が別に出す。

import type { GoogleSyncResult } from '@bindings/GoogleSyncResult';

/** 翻訳関数（i18next の t と同じ呼び方）。 */
type Translate = (key: string, opts?: Record<string, unknown>) => string;

/** 件数のある項目だけを「名前 N 件」の並びにする。 */
const counted = (t: Translate, items: [string, number][]): string[] =>
  items.filter(([, n]) => n > 0).map(([key, count]) => t(`settings.syncPart.${key}`, { count }));

/** 結果を 1 行の文にする。同期しなかった種類は出さない。 */
export function summarizeGoogleSync(r: GoogleSyncResult, t: Translate): string {
  const sections: string[] = [];
  if (r.calendar) {
    const c = r.calendar;
    const parts = counted(t, [
      ['eventsPulled', c.pulled],
      ['eventsPushed', c.pushed],
      ['eventsDeletedIn', c.deleted_in],
      ['eventsDeletedOut', c.deleted_out],
    ]);
    sections.push(
      t('settings.syncSection.calendar', {
        parts: parts.length > 0 ? parts.join(' / ') : t('settings.syncPart.none'),
      })
    );
  }
  if (r.contacts) {
    const c = r.contacts;
    const m = r.matched;
    const parts = counted(t, [
      ['contactsPulled', c.pulled],
      ['contactsPushed', c.pushed],
      ['contactsDeletedIn', c.deleted_in],
      ['contactsDeletedOut', c.deleted_out],
      ['contactsCreated', m?.created ?? 0],
      ['contactsLinked', m?.linked ?? 0],
      ['contactsAmbiguous', m?.ambiguous ?? 0],
      ['contactsConflicts', c.conflicts],
      ['contactsDeferred', c.deferred],
    ]);
    sections.push(
      t('settings.syncSection.contacts', {
        parts: parts.length > 0 ? parts.join(' / ') : t('settings.syncPart.none'),
      })
    );
  }
  return sections.join(t('settings.syncSection.separator'));
}
