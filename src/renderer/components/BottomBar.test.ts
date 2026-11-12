import { describe, expect, it } from 'vitest';
import { mailCountLabel } from './BottomBar';

// フッター左のメールの件数。絞り込み中は絞り込んだスレッドの数を、外したら今までの表示に戻す。

describe('mailCountLabel', () => {
  it('絞り込み中は「絞り込み n スレッド / 全 m 件」', () => {
    expect(mailCountLabel(39_471, 39_500, 1_234)).toEqual({
      key: 'mailbox.filteredTotal',
      values: { filtered: (1_234).toLocaleString(), total: (39_471).toLocaleString() },
    });
  });

  it('絞り込んだ結果が 0 件でも絞り込みの表示にする', () => {
    expect(mailCountLabel(10, null, 0)?.key).toBe('mailbox.filteredTotal');
  });

  it('絞り込みを外したら「ローカル / サーバ」に戻る', () => {
    expect(mailCountLabel(37_215, 39_471, null)).toEqual({
      key: 'mailbox.accountTotalOf',
      values: { shown: (37_215).toLocaleString(), total: (39_471).toLocaleString() },
    });
  });

  it('サーバの数が分からなければ「メール n 件」', () => {
    expect(mailCountLabel(5, 0, null)?.key).toBe('mailbox.accountTotal');
    expect(mailCountLabel(5, null, undefined)?.key).toBe('mailbox.accountTotal');
  });

  it('メールモードでなければ出さない', () => {
    expect(mailCountLabel(null, null, 3)).toBeNull();
  });
});
