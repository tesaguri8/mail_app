import { describe, expect, it } from 'vitest';
import type { MergeRemoteDeletion } from '@bindings/MergeRemoteDeletion';
import { remoteDeletionNotes, remoteDeletionTotal } from './mergeRemote';

/** 翻訳の代わり: キーの末尾と値を並べるだけ。 */
const t = (key: string, opts?: Record<string, unknown>) =>
  `${key.split('.').pop()}${opts ? ` ${opts.count}@${String(opts.account)}` : ''}`;

const d = (account_id: number, count: number): MergeRemoteDeletion => ({
  account_id,
  account_label: `acct${account_id}`,
  count,
});

describe('remoteDeletionNotes', () => {
  it('消すものが無ければ何も出さない（確認を挟まない）', () => {
    expect(remoteDeletionNotes([], t)).toEqual([]);
    expect(remoteDeletionNotes([d(1, 0)], t)).toEqual([]);
    expect(remoteDeletionTotal([])).toBe(0);
  });

  it('アカウントごとに 1 行、最後にゴミ箱から戻せる旨', () => {
    expect(remoteDeletionNotes([d(1, 2), d(2, 1)], t)).toEqual([
      'googleDeleteLine 2@acct1',
      'googleDeleteLine 1@acct2',
      'googleTrashNote',
    ]);
    expect(remoteDeletionTotal([d(1, 2), d(2, 1)])).toBe(3);
  });
});
