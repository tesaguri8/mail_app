import { describe, expect, it } from 'vitest';
import type { ContactMatch } from '@bindings/ContactMatch';
import { conflictOwnersByValue } from './contactConflicts';

const match = (id: number, name: string, emails: string[], phones: string[]): ContactMatch => ({
  id,
  display_name: name,
  organization: null,
  email: null,
  phone: null,
  matched_emails: emails,
  matched_phones: phones,
  matched_name: false,
});

describe('conflictOwnersByValue', () => {
  it('値ごとに一致した連絡先を名前つきで引ける', () => {
    const table = conflictOwnersByValue(
      [
        match(1, 'sngDESIGN名護', [], ['+81980510322']),
        match(2, '山田 花子', ['a@example.invalid'], ['+81980510322']),
      ],
      'matched_phones'
    );
    expect(table.get('+81980510322')).toEqual([
      { id: 1, display_name: 'sngDESIGN名護' },
      { id: 2, display_name: '山田 花子' },
    ]);
    expect(table.has('a@example.invalid')).toBe(false);
  });

  it('同じ連絡先が同じ値で二度当たっても 1 回だけ', () => {
    const table = conflictOwnersByValue(
      [match(1, 'A', ['x@example.invalid', 'x@example.invalid'], [])],
      'matched_emails'
    );
    expect(table.get('x@example.invalid')).toEqual([{ id: 1, display_name: 'A' }]);
  });
});
