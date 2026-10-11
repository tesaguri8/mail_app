import { describe, expect, it } from 'vitest';
import { orgMatchReasonLabel, othersLinked } from './orgMatch';

describe('orgMatchReasonLabel', () => {
  const fmt = (s: string) => (s === '+81980510322' ? '0980-51-0322' : s);

  it('理由ごとの文言と値', () => {
    expect(orgMatchReasonLabel({ kind: 'same_name' }, fmt)).toEqual({
      key: 'contact.orgMatchSameName',
    });
    expect(orgMatchReasonLabel({ kind: 'similar_name' }, fmt)).toEqual({
      key: 'contact.orgMatchSimilarName',
    });
    expect(orgMatchReasonLabel({ kind: 'domain', domain: 'example.co.jp' }, fmt)).toEqual({
      key: 'contact.orgMatchDomain',
      values: { domain: 'example.co.jp' },
    });
  });

  it('電話は表示用に整えて出す', () => {
    expect(orgMatchReasonLabel({ kind: 'phone', phone: '+81980510322' }, fmt)).toEqual({
      key: 'contact.orgMatchPhone',
      values: { phone: '0980-51-0322' },
    });
  });
});

describe('othersLinked', () => {
  it('本人を引いた人数（0 未満にしない）', () => {
    expect(othersLinked(3)).toBe(2);
    expect(othersLinked(1)).toBe(0);
    expect(othersLinked(0)).toBe(0);
  });
});
