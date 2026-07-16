import { describe, it, expect } from 'vitest';
import { htmlToText } from './htmlToText';

describe('htmlToText', () => {
  it('HTML でないプレーンテキストはそのまま返す', () => {
    expect(htmlToText('打ち合わせ 15:00 会議室A')).toBe('打ち合わせ 15:00 会議室A');
    expect(htmlToText('  前後に空白  ')).toBe('前後に空白');
  });

  it('null / undefined は空文字にする', () => {
    expect(htmlToText(null)).toBe('');
    expect(htmlToText(undefined)).toBe('');
  });

  it('<br> を改行にし、連続する空行は 1 つにまとめる', () => {
    expect(htmlToText('一行目<br>二行目<br><br><br>四行目')).toBe('一行目\n二行目\n\n四行目');
  });

  it('Google カレンダーの Zoom 招待（実例）を読める素のテキストにする', () => {
    const html =
      '<br>Zoom ミーティングに参加する<br>' +
      '<a href="https://www.google.com/url?q=https://us02web.zoom.us/j/87449834066?pwd%3Dn51BV0fv9Q0tP4RBwsFfehi5Snb0F9.1&amp;sa=D&amp;source=calendar&amp;usd=2&amp;usg=AOvVaw0FavdJ6QZATFzzWtZGvvXO" target="_blank">https://us02web.zoom.us/j/87449834066?pwd=n51BV0fv9Q0tP4RBwsFfehi5Snb0F9.1</a>' +
      '<br><br>ミーティング ID: 874 4983 4066<br>パスコード: 760129';
    expect(htmlToText(html)).toBe(
      'Zoom ミーティングに参加する\n' +
        'https://us02web.zoom.us/j/87449834066?pwd=n51BV0fv9Q0tP4RBwsFfehi5Snb0F9.1\n\n' +
        'ミーティング ID: 874 4983 4066\nパスコード: 760129',
    );
  });

  it('リンク文字が URL でないときは「文字 (URL)」を残す', () => {
    expect(htmlToText('詳細は<a href="https://example.com/info">こちら</a>')).toBe(
      '詳細はこちら (https://example.com/info)',
    );
  });

  it('リンク文字が空なら Google リダイレクトを実 URL に展開して使う', () => {
    const html =
      '<a href="https://www.google.com/url?q=https://zoom.us/j/123&amp;sa=D"></a>';
    expect(htmlToText(html)).toBe('https://zoom.us/j/123');
  });

  it('HTML エンティティを復号する', () => {
    expect(htmlToText('A &amp; B &lt;tag&gt; &quot;q&quot; &#39;a&#39;')).toBe('A & B <tag> "q" \'a\'');
  });

  it('段落・リスト等のブロック要素を改行に変換する', () => {
    expect(htmlToText('<p>第一段落</p><p>第二段落</p>')).toBe('第一段落\n\n第二段落');
    expect(htmlToText('<ul><li>りんご</li><li>みかん</li></ul>')).toBe('りんご\n\nみかん');
  });

  it('script / style の中身は捨てる', () => {
    expect(htmlToText('前<style>.x{color:red}</style>後<br>次')).toBe('前後\n次');
  });
});
