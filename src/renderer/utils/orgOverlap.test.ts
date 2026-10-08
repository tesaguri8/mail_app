import { describe, expect, it } from 'vitest';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import { findOrgOverlap, hasOrgOverlap, mergeOrgOverlap } from './orgOverlap';

const org = (o: Partial<OrganizationSummary>): OrganizationSummary => ({
  id: 1,
  name: '株式会社テスト',
  name_kana: null,
  note: null,
  phone: null,
  fax: null,
  email: null,
  url: null,
  address: { postal: null, region: null, city: null, street: null, extended: null, country: null },
  member_count: 1,
  deleted_at: null,
  ...o,
});

// テストでは数字だけを比べる（実際は E.164 へ正規化する）。
const phoneKey = (v: string) => v.replace(/\D/g, '').replace(/^81/, '0');

const addr = (postal: string, region: string, city: string, street: string) => ({
  label: null,
  postal,
  region,
  city,
  street,
  extended: null,
  country: null,
});

describe('findOrgOverlap', () => {
  it('代表電話・FAX と同じ電話を見つける（表記の違いは正規化で吸収）', () => {
    const o = org({ phone: '+81312345678', fax: '+81312345679' });
    const d = {
      emails: [],
      phones: [
        { label: null, value: '03-1234-5678', is_shared: false },
        { label: null, value: '090-1111-2222', is_shared: false },
        { label: 'FAX', value: '+81312345679', is_shared: false },
      ],
      addresses: [],
    };
    expect(findOrgOverlap(d, o, phoneKey).phones).toEqual([0, 2]);
  });

  it('代表メールは大文字小文字を無視し、共有の印が既にあるものは数えない', () => {
    const o = org({ email: 'info@example.co.jp' });
    const d = {
      emails: [
        { label: null, value: 'Info@Example.co.jp', is_shared: false },
        { label: null, value: 'info@example.co.jp', is_shared: true },
        { label: null, value: 'taro@example.co.jp', is_shared: false },
      ],
      phones: [],
      addresses: [],
    };
    expect(findOrgOverlap(d, o, phoneKey).emails).toEqual([0]);
  });

  it('住所は郵便番号の表記と空白の違いを無視して比べる', () => {
    const o = org({
      address: {
        postal: '100-0001',
        region: '東京都',
        city: '千代田区',
        street: '千代田 1-1',
        extended: null,
        country: null,
      },
    });
    const d = {
      emails: [],
      phones: [],
      addresses: [
        addr('1000001', '東京都', '千代田区', '千代田  1-1'),
        addr('1000001', '東京都', '千代田区', '丸の内 1-1'),
      ],
    };
    expect(findOrgOverlap(d, o, phoneKey).addresses).toEqual([0]);
  });

  it('組織側が空なら何も重ならない（空どうしを同じと見ない）', () => {
    const d = {
      emails: [{ label: null, value: 'a@b.jp', is_shared: false }],
      phones: [{ label: null, value: '', is_shared: false }],
      addresses: [addr('', '', '', '')],
    };
    expect(hasOrgOverlap(findOrgOverlap(d, org({}), phoneKey))).toBe(false);
  });
});

describe('mergeOrgOverlap', () => {
  it('電話・住所は外し、メールは消さずに共有の印を付ける', () => {
    const d = {
      emails: [
        { label: null, value: 'info@example.co.jp', is_shared: false },
        { label: null, value: 'taro@example.co.jp', is_shared: false },
      ],
      phones: [
        { label: null, value: '+81312345678', is_shared: false },
        { label: null, value: '+819011112222', is_shared: false },
      ],
      addresses: [addr('1000001', '東京都', '千代田区', '千代田 1-1')],
    };
    const merged = mergeOrgOverlap(d, { emails: [0], phones: [0], addresses: [0] });
    expect(merged.emails).toEqual([
      { label: null, value: 'info@example.co.jp', is_shared: true },
      { label: null, value: 'taro@example.co.jp', is_shared: false },
    ]);
    expect(merged.phones.map((p) => p.value)).toEqual(['+819011112222']);
    expect(merged.addresses).toEqual([]);
  });
});
