import { describe, expect, it } from 'vitest';
import type { ContactLink } from '@bindings/ContactLink';
import { emptyOrganization, isBlankOrganization } from './contactDraft';
import { matchesSource } from '../components/ContactLinks';

describe('isBlankOrganization', () => {
  it('何も入れていない会社は空', () => {
    expect(isBlankOrganization(emptyOrganization())).toBe(true);
    expect(isBlankOrganization({ ...emptyOrganization(), name: '  ' })).toBe(true);
  });
  it('どれか 1 つでも入っていれば空ではない', () => {
    expect(isBlankOrganization({ ...emptyOrganization(), title: '部長' })).toBe(false);
    expect(isBlankOrganization({ ...emptyOrganization(), org_id: 3 })).toBe(false);
  });
});

describe('matchesSource', () => {
  const google: ContactLink = {
    provider: 'google',
    account_id: 1,
    account_email: 'a@x.jp',
    account_label: 'a@x.jp',
    disconnected: false,
    state: 'synced',
  };
  const icloud: ContactLink = {
    provider: 'icloud',
    account_id: 2,
    account_email: null,
    account_label: null,
    disconnected: false,
    state: 'synced',
  };
  it('すべては常に当たる', () => {
    expect(matchesSource([], 'all')).toBe(true);
    expect(matchesSource([google], 'all')).toBe(true);
  });
  it('Rondine のみは、どこにもつながっていない連絡先だけ', () => {
    expect(matchesSource([], 'rondine')).toBe(true);
    expect(matchesSource([google], 'rondine')).toBe(false);
  });
  it('サービスごとは、そのサービスにつながっていれば当たる（複数つながりも）', () => {
    expect(matchesSource([google, icloud], 'google')).toBe(true);
    expect(matchesSource([google, icloud], 'icloud')).toBe(true);
    expect(matchesSource([google], 'icloud')).toBe(false);
  });
});
