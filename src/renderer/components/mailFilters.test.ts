import { describe, it, expect } from 'vitest';
import { matchesFilters, matchesNoneOfFilters } from './mailFilters';

/** 試験用の一覧行。既定は「既読・スター無し・知り合いでない・添付なし」。 */
const row = (over: Partial<Parameters<typeof matchesFilters>[0]> = {}) => ({
  is_read: true,
  has_real_attachments: false,
  is_starred: false,
  is_green: false,
  is_vip: false,
  is_known: false,
  is_replied: false,
  ...over,
});

const f = (...keys: string[]) => new Set(keys);

describe('matchesFilters（通常＝選択した条件を AND）', () => {
  it('条件が無ければ全部通す', () => {
    expect(matchesFilters(row(), f())).toBe(true);
  });

  it('単独条件は素直に効く', () => {
    expect(matchesFilters(row({ is_starred: true }), f('star'))).toBe(true);
    expect(matchesFilters(row({ is_starred: false }), f('star'))).toBe(false);
  });

  it('複数条件は AND（両方を満たすものだけ）', () => {
    const both = row({ is_starred: true, is_known: true });
    expect(matchesFilters(both, f('star', 'known'))).toBe(true);
    expect(matchesFilters(row({ is_starred: true }), f('star', 'known'))).toBe(false);
    expect(matchesFilters(row({ is_known: true }), f('star', 'known'))).toBe(false);
  });
});

describe('matchesNoneOfFilters（反転＝どれにも当てはまらない）', () => {
  it('単独条件では matchesFilters の否定と一致する', () => {
    for (const starred of [true, false]) {
      const m = row({ is_starred: starred });
      expect(matchesNoneOfFilters(m, f('star'))).toBe(!matchesFilters(m, f('star')));
    }
  });

  it('複数条件では「どちらも満たさない」ものだけを通す', () => {
    const keys = f('star', 'known');
    // スターでも知り合いでもない＝通す（不要メールの一括選択で拾いたいもの）
    expect(matchesNoneOfFilters(row(), keys)).toBe(true);
    // 片方でも当てはまれば通さない
    expect(matchesNoneOfFilters(row({ is_starred: true }), keys)).toBe(false);
    expect(matchesNoneOfFilters(row({ is_known: true }), keys)).toBe(false);
    expect(matchesNoneOfFilters(row({ is_starred: true, is_known: true }), keys)).toBe(false);
  });

  it('複数条件では単純否定（!matchesFilters）と違う結果になる', () => {
    // ここが不具合の核心。片方だけ当てはまる行は、単純否定なら通ってしまう。
    const onlyStarred = row({ is_starred: true });
    const keys = f('star', 'known');
    expect(!matchesFilters(onlyStarred, keys)).toBe(true); // 旧実装は通していた
    expect(matchesNoneOfFilters(onlyStarred, keys)).toBe(false); // 新実装は通さない
  });

  it('unread の否定は「既読だけ」', () => {
    expect(matchesNoneOfFilters(row({ is_read: true }), f('unread'))).toBe(true);
    expect(matchesNoneOfFilters(row({ is_read: false }), f('unread'))).toBe(false);
  });

  it('「既読かつ知り合いでない」＝ 未読と知り合いを反転で除く、が成立する', () => {
    const keys = f('unread', 'known');
    expect(matchesNoneOfFilters(row({ is_read: true, is_known: false }), keys)).toBe(true);
    expect(matchesNoneOfFilters(row({ is_read: false, is_known: false }), keys)).toBe(false);
    expect(matchesNoneOfFilters(row({ is_read: true, is_known: true }), keys)).toBe(false);
  });

  it('添付・グリーン・お気に入りも同じ規則で効く', () => {
    const keys = f('attachment', 'green', 'vip');
    expect(matchesNoneOfFilters(row(), keys)).toBe(true);
    expect(matchesNoneOfFilters(row({ has_real_attachments: true }), keys)).toBe(false);
    expect(matchesNoneOfFilters(row({ is_green: true }), keys)).toBe(false);
    expect(matchesNoneOfFilters(row({ is_vip: true }), keys)).toBe(false);
  });

  it('返信歴（replied）も同じ規則で効く', () => {
    // 通常＝返信歴のある相手だけ、反転＝返信歴の無い相手だけ。
    expect(matchesFilters(row({ is_replied: true }), f('replied'))).toBe(true);
    expect(matchesFilters(row({ is_replied: false }), f('replied'))).toBe(false);
    expect(matchesNoneOfFilters(row({ is_replied: false }), f('replied'))).toBe(true);
    expect(matchesNoneOfFilters(row({ is_replied: true }), f('replied'))).toBe(false);
  });

  it('flag は通常・反転ともに非適用（マーク手段が無いため）', () => {
    expect(matchesFilters(row(), f('flag'))).toBe(true);
    expect(matchesNoneOfFilters(row(), f('flag'))).toBe(true);
  });
});
