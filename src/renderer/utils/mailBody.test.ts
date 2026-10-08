import { describe, expect, it } from 'vitest';
import { hasReadableBody, htmlHasContent } from './mailBody';

describe('hasReadableBody', () => {
  it('本文が入っていれば「ある」', () => {
    expect(hasReadableBody({ clean_body: 'こんにちは' })).toBe(true);
    expect(hasReadableBody({ body_plain: 'こんにちは' })).toBe(true);
    expect(hasReadableBody({ body_html: '<html><body>こんにちは</body></html>' })).toBe(true);
  });

  it('合成された空の骨組みは「無い」', () => {
    // これを「ある」と数えたために、本文が空のまま取得済み扱いになる不具合が起きた。
    expect(hasReadableBody({ body_html: '<html><body></body></html>' })).toBe(false);
    expect(hasReadableBody({ body_plain: '', body_html: '<HTML><BODY></BODY></HTML>' })).toBe(false);
  });

  it('空白や改行だけなら「無い」', () => {
    expect(hasReadableBody({ clean_body: '  \n ', body_plain: '' })).toBe(false);
    expect(hasReadableBody({})).toBe(false);
  });

  it('画像だけの HTML は「ある」（取得済みの本文なので取り直さない）', () => {
    expect(htmlHasContent('<html><body><img src="cid:a"></body></html>')).toBe(true);
  });
});
