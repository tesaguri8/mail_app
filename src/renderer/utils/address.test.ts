import { describe, it, expect } from 'vitest';
import { formatSender } from './address';

describe('formatSender', () => {
  it('名前があれば "名前 <アドレス>"', () => {
    expect(formatSender('金城 彩', 'aya@kinjo.invalid')).toBe('金城 彩 <aya@kinjo.invalid>');
  });

  it('名前が無い・アドレスと同じならアドレスだけ', () => {
    expect(formatSender(null, 'aya@kinjo.invalid')).toBe('aya@kinjo.invalid');
    expect(formatSender('  ', 'aya@kinjo.invalid')).toBe('aya@kinjo.invalid');
    expect(formatSender('Aya@Kinjo.invalid', 'aya@kinjo.invalid')).toBe('aya@kinjo.invalid');
  });

  it('アドレスが無ければ名前だけ（どちらも無ければ空）', () => {
    expect(formatSender('金城 彩', null)).toBe('金城 彩');
    expect(formatSender(null, null)).toBe('');
  });
});
