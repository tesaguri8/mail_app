import { describe, it, expect } from 'vitest';
import { splitPersonName } from './name';

describe('splitPersonName', () => {
  it('日本語の「姓 名」（半角/全角スペース）を分割する', () => {
    expect(splitPersonName('末松 慎吾')).toEqual({ family: '末松', given: '慎吾' });
    expect(splitPersonName('末松　慎吾')).toEqual({ family: '末松', given: '慎吾' });
  });

  it('欧文の「Given Family」を分割する（末尾＝姓）', () => {
    expect(splitPersonName('John Smith')).toEqual({ family: 'Smith', given: 'John' });
    expect(splitPersonName('Mary Jane Watson')).toEqual({ family: 'Watson', given: 'Mary Jane' });
  });

  it('欧文の「Family, Given」（カンマ表記）を分割する', () => {
    expect(splitPersonName('Smith, John')).toEqual({ family: 'Smith', given: 'John' });
  });

  it('空白の無い日本語名は分割せず表示名だけ残す（誤分割を避ける）', () => {
    expect(splitPersonName('末松慎吾')).toEqual({ family: null, given: null });
  });

  it('組織らしい名前は人名分割しない', () => {
    expect(splitPersonName('株式会社テサグリ')).toEqual({ family: null, given: null });
    expect(splitPersonName('Acme Inc')).toEqual({ family: null, given: null });
    expect(splitPersonName('サポート 窓口')).toEqual({ family: null, given: null });
  });

  it('前後の引用符・空白を取り除く', () => {
    expect(splitPersonName('"John Smith"')).toEqual({ family: 'Smith', given: 'John' });
    expect(splitPersonName('  末松 慎吾  ')).toEqual({ family: '末松', given: '慎吾' });
  });

  it('空・空白のみは空を返す', () => {
    expect(splitPersonName('')).toEqual({ family: null, given: null });
    expect(splitPersonName('   ')).toEqual({ family: null, given: null });
  });
});
