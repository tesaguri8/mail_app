import { describe, expect, it } from 'vitest';
import { replyRecipients, type ReplySource } from './replyRecipients';

const ME = 'me@example.invalid';
const src = (over: Partial<ReplySource>): ReplySource => ({
  from_address: null,
  reply_to: null,
  to_addresses: null,
  cc_addresses: null,
  ...over,
});

describe('replyRecipients', () => {
  it('相手からのメールへの返信は差出人へ', () => {
    const s = src({ from_address: 'a@example.invalid', to_addresses: `${ME}, b@example.invalid` });
    expect(replyRecipients(s, 'reply', [ME])).toEqual({ to: 'a@example.invalid', cc: '' });
  });

  it('相手からのメールへの全員返信は、元の To と Cc から自分と差出人を除いて Cc に入れる', () => {
    const s = src({
      from_address: 'a@example.invalid',
      to_addresses: `山田 <${ME}>, b@example.invalid`,
      cc_addresses: 'c@example.invalid, A@example.invalid',
    });
    expect(replyRecipients(s, 'replyAll', [ME])).toEqual({
      to: 'a@example.invalid',
      cc: 'b@example.invalid, c@example.invalid',
    });
  });

  it('Reply-To があればそちらへ返す', () => {
    const s = src({ from_address: 'noreply@example.invalid', reply_to: 'desk@example.invalid' });
    expect(replyRecipients(s, 'reply', [ME]).to).toBe('desk@example.invalid');
  });

  it('自分が送ったメールへの返信は元の To へ（自分へは返さない）', () => {
    const s = src({ from_address: ME, to_addresses: '前田 <maeda@example.invalid>' });
    expect(replyRecipients(s, 'reply', [ME])).toEqual({
      to: '前田 <maeda@example.invalid>',
      cc: '',
    });
  });

  it('自分が送ったメールへの全員返信は元の To と Cc へ（自分を除く）', () => {
    const s = src({
      from_address: `末松 <ME@Example.Invalid>`,
      to_addresses: `maeda@example.invalid, ${ME}`,
      cc_addresses: `c@example.invalid, ${ME}, maeda@example.invalid`,
    });
    expect(replyRecipients(s, 'replyAll', [ME])).toEqual({
      to: 'maeda@example.invalid',
      cc: 'c@example.invalid',
    });
  });

  it('自分宛のメモ（To が自分だけ）への返信は自分へ', () => {
    const s = src({ from_address: ME, to_addresses: ME });
    expect(replyRecipients(s, 'replyAll', [ME])).toEqual({ to: ME, cc: '' });
  });

  it('別のアカウントのアドレスも自分として扱う', () => {
    const other = 'work@example.invalid';
    const s = src({ from_address: other, to_addresses: `x@example.invalid, ${ME}` });
    expect(replyRecipients(s, 'reply', [ME, other])).toEqual({ to: 'x@example.invalid', cc: '' });
  });
});
