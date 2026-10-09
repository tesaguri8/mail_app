import { describe, it, expect } from 'vitest';
import type { PostalAddress } from '@bindings/PostalAddress';
import {
  isJapanese,
  patchFromAddress,
  patchFromCode,
  postalDigits,
  wantsAddressFromCode,
  wantsCodeFromAddress,
  type PostalFields,
} from './postalAutofill';

const empty: PostalFields = { postal: null, region: null, city: null, street: null, extended: null, country: null };
const found: PostalAddress = { postal: '060-0042', region: '北海道', city: '札幌市中央区', town: '大通西' };

describe('postalAutofill', () => {
  it('国が空か日本のときだけ働く', () => {
    expect(isJapanese(null)).toBe(true);
    expect(isJapanese('日本')).toBe(true);
    expect(isJapanese(' Japan ')).toBe(true);
    expect(isJapanese('USA')).toBe(false);
    expect(isJapanese(null, 'US')).toBe(false);
    expect(isJapanese(null, 'jp')).toBe(true);
  });

  it('郵便番号は全角・ハイフンを吸収して数字だけにする', () => {
    expect(postalDigits('０６０－００４２')).toBe('0600042');
    expect(postalDigits('060 0042')).toBe('0600042');
  });

  it('郵便番号 7 桁で住所が空なら、住所を引く', () => {
    expect(wantsAddressFromCode({ ...empty, postal: '060-0042' })).toBe(true);
    expect(wantsAddressFromCode({ ...empty, postal: '06000' })).toBe(false);
    expect(wantsAddressFromCode({ ...empty, postal: '0600042', city: '札幌市' })).toBe(false);
    expect(wantsAddressFromCode({ ...empty, postal: '0600042', country: 'USA' })).toBe(false);
  });

  it('郵便番号が空で住所がそろったら、郵便番号を引く', () => {
    const a = { ...empty, region: '北海道', city: '札幌市中央区', street: '大通西3丁目' };
    expect(wantsCodeFromAddress(a)).toBe(true);
    expect(wantsCodeFromAddress({ ...a, street: null })).toBe(false);
    expect(wantsCodeFromAddress({ ...a, postal: '0600042' })).toBe(false);
  });

  it('入っている値は上書きしない', () => {
    expect(patchFromAddress({ ...empty, city: '札幌市' }, found)).toEqual({ region: '北海道', street: '大通西' });
    expect(patchFromAddress(empty, { ...found, town: '' })).toEqual({ region: '北海道', city: '札幌市中央区' });
    expect(patchFromCode({ ...empty, postal: '1000005' }, found)).toEqual({});
    expect(patchFromCode(empty, found)).toEqual({ postal: '060-0042' });
  });
});
