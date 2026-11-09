import { describe, expect, it } from 'vitest';
import { displayUrl, linkTrust, type LinkTrustInput } from './linkTrust';

/** 目印が何も立っていない差出人（知らない相手）。 */
const stranger: LinkTrustInput = {
  is_green: false,
  is_known: false,
  is_vip: false,
  is_replied: false,
  own: false,
  spoof_suspect: false,
};

describe('linkTrust（左クリックで開いてよいか）', () => {
  it('知らない差出人は止める', () => {
    expect(linkTrust(stranger)).toBe('guarded');
  });

  it('グリーン・住所録・VIP・返信歴・自分が送ったメールのどれかなら開く', () => {
    expect(linkTrust({ ...stranger, is_green: true })).toBe('open');
    expect(linkTrust({ ...stranger, is_known: true })).toBe('open');
    expect(linkTrust({ ...stranger, is_vip: true })).toBe('open');
    expect(linkTrust({ ...stranger, is_replied: true })).toBe('open');
    expect(linkTrust({ ...stranger, own: true })).toBe('open');
  });

  it('なりすましの疑いがあれば、ほかに当たっていても止める', () => {
    expect(
      linkTrust({
        is_green: true,
        is_known: true,
        is_vip: true,
        is_replied: true,
        own: true,
        spoof_suspect: true,
      })
    ).toBe('guarded');
  });
});

describe('displayUrl（メニューに出す行き先）', () => {
  it('短ければそのまま', () => {
    expect(displayUrl('https://example.invalid/a')).toBe('https://example.invalid/a');
  });

  it('長ければ後ろを詰め、ドメインは残す', () => {
    const url = `https://shop.example.invalid/${'x'.repeat(100)}?id=1`;
    const s = displayUrl(url, 40);
    expect(s.length).toBe(40);
    expect(s.startsWith('https://shop.example.invalid/')).toBe(true);
    expect(s.endsWith('…')).toBe(true);
  });

  it('ドメインだけで長くても、ドメインは削らない', () => {
    const host = `${'a'.repeat(50)}.example.invalid`;
    expect(displayUrl(`https://${host}/path`, 30)).toBe(`https://${host}…`);
    expect(displayUrl(`https://${host}`, 30)).toBe(`https://${host}`);
  });

  it('mailto も扱える', () => {
    const url = `mailto:someone@example.invalid?subject=${'y'.repeat(80)}`;
    expect(displayUrl(url, 40).startsWith('mailto:someone@example.invalid')).toBe(true);
  });
});
