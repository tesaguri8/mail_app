import { describe, expect, it } from 'vitest';
import { orgFieldStatus, orgMatchKey, orgMatchReasonLabel, othersLinked } from './orgMatch';

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

describe('orgFieldStatus', () => {
  it('つながっていなければ、候補があれば黄・無ければ赤', () => {
    expect(orgFieldStatus(true, 2)).toBe('similar');
    expect(orgFieldStatus(true, 0)).toBe('unlinked');
  });

  it('候補を取得中は色なし（赤から黄へちらつかない）', () => {
    expect(orgFieldStatus(true, null)).toBeNull();
  });

  it('つながっている・会社名が空（呼び出し側で unlinked=false）なら色なし', () => {
    expect(orgFieldStatus(false, 3)).toBeNull();
    expect(orgFieldStatus(false, null)).toBeNull();
  });
});

describe('orgMatchKey', () => {
  it('会社名と人で別のキー（前の人・前の会社名の結果を使わない）', () => {
    expect(orgMatchKey('sngDESIGN', 2)).not.toBe(orgMatchKey('sngDESIGN', 3));
    expect(orgMatchKey('sngDESIGN', 2)).not.toBe(orgMatchKey('別の会社', 2));
    expect(orgMatchKey(' sngDESIGN ', 2)).toBe(orgMatchKey('sngDESIGN', 2));
  });
});
