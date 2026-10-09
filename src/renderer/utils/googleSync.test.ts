import { describe, expect, it } from 'vitest';
import type { GoogleSyncResult } from '@bindings/GoogleSyncResult';
import { summarizeGoogleSync } from './googleSyncSummary';

/** 翻訳の代わり: キーの末尾と値を並べるだけ。 */
const t = (key: string, opts?: Record<string, unknown>) => {
  const k = key.split('.').pop() ?? key;
  if (k === 'separator') return ' | ';
  if (opts && 'parts' in opts) return `${k}: ${String(opts.parts)}`;
  if (opts && 'count' in opts) return `${k}=${String(opts.count)}`;
  return k;
};

const empty: GoogleSyncResult = {
  calendar: null,
  calendar_error: null,
  contacts: null,
  matched: null,
  contacts_error: null,
};

describe('summarizeGoogleSync', () => {
  it('件数のある項目だけを並べ、何も無い種類は「変更なし」', () => {
    const r: GoogleSyncResult = {
      ...empty,
      calendar: { pulled: 0, pushed: 0, deleted_in: 0, deleted_out: 0, calendars: 3 },
      contacts: {
        pulled: 12,
        pushed: 0,
        deleted_in: 1,
        deleted_out: 0,
        skipped: 0,
        conflicts: 0,
        unlinked: 0,
        deferred: 0,
        unchanged: 0,
      },
      matched: { linked: 2, created: 10, ambiguous: 1 },
    };
    expect(summarizeGoogleSync(r, t)).toBe(
      'calendar: none | contacts: contactsPulled=12 / contactsDeletedIn=1 / contactsCreated=10 / contactsLinked=2 / contactsAmbiguous=1'
    );
  });

  it('送り切れずに次の同期へ回した件数も出す（統合で大量に溜まったとき）', () => {
    const r: GoogleSyncResult = {
      ...empty,
      contacts: {
        pulled: 0,
        pushed: 200,
        deleted_in: 0,
        deleted_out: 2000,
        skipped: 0,
        conflicts: 0,
        unlinked: 0,
        deferred: 3193,
        unchanged: 1059,
      },
    };
    expect(summarizeGoogleSync(r, t)).toBe(
      'contacts: contactsPushed=200 / contactsDeletedOut=2000 / contactsUnchanged=1059 / contactsDeferred=3193'
    );
  });

  it('同期しなかった種類は出さない（連絡先を同期しないアカウント）', () => {
    const r: GoogleSyncResult = {
      ...empty,
      calendar: { pulled: 4, pushed: 1, deleted_in: 0, deleted_out: 0, calendars: 1 },
    };
    expect(summarizeGoogleSync(r, t)).toBe('calendar: eventsPulled=4 / eventsPushed=1');
  });
});
