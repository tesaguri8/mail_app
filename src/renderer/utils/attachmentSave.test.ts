import { describe, it, expect } from 'vitest';
import type { TFunction } from 'i18next';
import {
  ERR_IMAP_TIMEOUT,
  attachmentErrorReason,
  safeFilename,
  saveAllMessage,
  uniqueNames,
} from './attachmentSave';

describe('uniqueNames（まとめ保存のファイル名衝突回避）', () => {
  it('重複が無ければそのまま', () => {
    expect(uniqueNames(['a.pdf', 'b.pdf'])).toEqual(['a.pdf', 'b.pdf']);
  });

  it('同名は 2 件目以降に連番を付ける（黙って上書きしない）', () => {
    expect(uniqueNames(['image.png', 'image.png', 'image.png'])).toEqual([
      'image.png',
      'image (2).png',
      'image (3).png',
    ]);
  });

  it('大文字小文字が違うだけの名前も衝突として扱う（Windows/macOS を考慮）', () => {
    expect(uniqueNames(['Report.PDF', 'report.pdf'])).toEqual(['Report.PDF', 'report (2).pdf']);
  });

  it('拡張子の無い名前は末尾に付ける', () => {
    expect(uniqueNames(['README', 'README'])).toEqual(['README', 'README (2)']);
  });

  it('先頭のドットは拡張子として扱わない', () => {
    expect(uniqueNames(['.gitignore', '.gitignore'])).toEqual(['.gitignore', '.gitignore (2)']);
  });

  it('複数の拡張子は最後のドットで分ける', () => {
    expect(uniqueNames(['archive.tar.gz', 'archive.tar.gz'])).toEqual([
      'archive.tar.gz',
      'archive.tar (2).gz',
    ]);
  });

  it('別々の重複がそれぞれ独立して数えられる', () => {
    expect(uniqueNames(['a.txt', 'b.txt', 'a.txt', 'b.txt', 'a.txt'])).toEqual([
      'a.txt',
      'b.txt',
      'a (2).txt',
      'b (2).txt',
      'a (3).txt',
    ]);
  });

  it('空の入力は空を返す', () => {
    expect(uniqueNames([])).toEqual([]);
  });
});

describe('safeFilename（保存先パスに使えない文字の置換）', () => {
  it('普通のファイル名はそのまま', () => {
    expect(safeFilename('報告書 2025.pdf')).toBe('報告書 2025.pdf');
  });

  it('パス区切りや Windows で使えない文字を _ にする', () => {
    expect(safeFilename('2025/10 見積:A案.pdf')).toBe('2025_10 見積_A案.pdf');
    expect(safeFilename('a<b>c|d?e*f".txt')).toBe('a_b_c_d_e_f_.txt');
  });

  it('制御文字も _ にする', () => {
    expect(safeFilename('report\u0000\u001f.pdf')).toBe('report__.pdf');
  });

  it('末尾のドット・空白は落とす（Windows が受け付けない）', () => {
    expect(safeFilename('report.pdf. ')).toBe('report.pdf');
  });

  it('先頭のドットは残す', () => {
    expect(safeFilename('.gitignore')).toBe('.gitignore');
  });

  it('全部使えない文字でも空にはしない', () => {
    expect(safeFilename('   ')).toBe('attachment');
  });
});

describe('attachmentErrorReason / saveAllMessage（失敗を利用者に伝える）', () => {
  const t = ((key: string, opts?: Record<string, unknown>) =>
    opts ? `${key} ${JSON.stringify(opts)}` : key) as unknown as TFunction;

  it('時間切れの符号は、分かる文言に置き換える', () => {
    expect(attachmentErrorReason(ERR_IMAP_TIMEOUT, t)).toBe('mailbox.attachmentTimeout');
  });

  it('それ以外の失敗は、理由をそのまま出す', () => {
    expect(attachmentErrorReason('保存に失敗しました: 権限がありません', t)).toBe(
      '保存に失敗しました: 権限がありません'
    );
    expect(attachmentErrorReason(new Error('x'), t)).toBe('x');
  });

  it('全部保存できたら成功として件数を出す', () => {
    expect(saveAllMessage({ saved: 3, failed: 0 }, t)).toEqual({
      text: 'mailbox.attachmentSavedN {"count":3}',
      kind: 'ok',
    });
  });

  it('1 件でも失敗したら失敗として、件数と最初の理由を出す', () => {
    const m = saveAllMessage({ saved: 1, failed: 2, error: ERR_IMAP_TIMEOUT }, t);
    expect(m.kind).toBe('error');
    expect(m.text).toBe(
      'mailbox.attachmentSavedPartial {"saved":1,"failed":2,"reason":"mailbox.attachmentTimeout"}'
    );
  });
});
