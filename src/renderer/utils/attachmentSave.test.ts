import { describe, it, expect } from 'vitest';
import { uniqueNames } from './attachmentSave';

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
