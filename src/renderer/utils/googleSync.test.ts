import { describe, expect, it } from 'vitest';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import type { GoogleSyncResult } from '@bindings/GoogleSyncResult';
import { summarizeGoogleSync } from './googleSyncSummary';
import { CONTACTS_AUTO_SYNC_MS, contactsDue } from './googleSyncSchedule';

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
      },
      matched: { linked: 2, created: 10, ambiguous: 1 },
    };
    expect(summarizeGoogleSync(r, t)).toBe(
      'calendar: none | contacts: contactsPulled=12 / contactsDeletedIn=1 / contactsCreated=10 / contactsLinked=2 / contactsAmbiguous=1'
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

describe('contactsDue', () => {
  const now = Date.parse('2026-10-09T12:00:00Z');
  const account = (over: Partial<GoogleAccount> = {}): GoogleAccount => ({
    id: 1,
    email: 'a@gmail.com',
    sync_calendar: true,
    sync_contacts: true,
    push_new_contacts: false,
    last_calendar_sync_at: null,
    last_contacts_sync_at: '2026-10-09 11:58:00',
    disconnected_at: null,
    ...over,
  });

  it('起動して最初の 1 回は、直前に同期していても回す', () => {
    expect(contactsDue(account(), now, false)).toBe(true);
  });
  it('起動後は前回から間が空いたときだけ', () => {
    expect(contactsDue(account(), now, true)).toBe(false);
    const old = new Date(now - CONTACTS_AUTO_SYNC_MS).toISOString().replace('T', ' ').slice(0, 19);
    expect(contactsDue(account({ last_contacts_sync_at: old }), now, true)).toBe(true);
    expect(contactsDue(account({ last_contacts_sync_at: null }), now, true)).toBe(true);
  });
  it('連絡先を同期しない・解除中のアカウントは回さない', () => {
    expect(contactsDue(account({ sync_contacts: false }), now, false)).toBe(false);
    expect(contactsDue(account({ disconnected_at: '2026-10-09 10:00:00' }), now, false)).toBe(
      false
    );
  });
});
