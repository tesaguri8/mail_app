import { describe, it, expect } from 'vitest';
import { isEmlAttachment } from './EmlInnerAttachments';

describe('isEmlAttachment', () => {
  it('message/rfc822 か、名前が .eml の添付を転送メールとみなす', () => {
    expect(isEmlAttachment({ content_type: 'message/rfc822', filename: 'attachment-1.eml' })).toBe(
      true
    );
    expect(isEmlAttachment({ content_type: 'Message/RFC822', filename: '転送' })).toBe(true);
    // 型が application/octet-stream で付いてくる .eml もある。
    expect(
      isEmlAttachment({ content_type: 'application/octet-stream', filename: 'fwd.EML ' })
    ).toBe(true);
  });

  it('ほかの添付は転送メールとみなさない', () => {
    expect(isEmlAttachment({ content_type: 'application/pdf', filename: 'mitsumori.pdf' })).toBe(
      false
    );
    expect(isEmlAttachment({ content_type: null, filename: 'eml.txt' })).toBe(false);
  });
});
