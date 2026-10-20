// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { UnseenQuote } from '@bindings/UnseenQuote';
import { oldestFirst, UnseenQuotes } from './UnseenQuotes';
import { markAttrs, readMarks } from './timeline/marks';
import { formatDateTime, parseQuotedDate } from '../utils/datetime';

/**
 * 引用されていたメールの枠（未受信）の本文も、通常のバブルと同じく URL を押せるリンクにする。
 * 枠の本文は外側のメールの body_plain から切り出したもので、改行が \r\r\n のこともある（実測 53359）。
 */
let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const quote = (body: string): UnseenQuote => ({
  from: '前田 太郎 <taro@maeda.invalid>',
  sender: '前田 太郎',
  at: '2026年10月15日 11:21',
  date: '2026-10-15T11:21:00',
  subject: '図面の件',
  body,
});

describe('UnseenQuotes の本文のリンク', () => {
  it('URL は通常のバブルと同じリンクになる（改行が \\r\\r\\n でも末尾に紛れない）', () => {
    const body =
      'データ便（IFCデータ）\r\r\nhttps://deliver.example.invalid/d/abc123\r\r\n\r\r\nよろしくお願いします。';
    act(() => root.render(<UnseenQuotes quotes={[quote(body)]} out={false} />));
    const links = [...container.querySelectorAll('a')];
    expect(links.map((a) => a.getAttribute('href'))).toEqual([
      'https://deliver.example.invalid/d/abc123',
    ]);
    expect(links[0].textContent).toBe('https://deliver.example.invalid/d/abc123');
  });

  it('<mailto:…> の中の URL でないアドレスにはリンクを張らない（通常のバブルと同じ）', () => {
    const body = 'Email: taro@maeda.invalid <mailto:taro@maeda.invalid>\r\nwww.maeda.invalid';
    act(() => root.render(<UnseenQuotes quotes={[quote(body)]} out={false} />));
    const hrefs = [...container.querySelectorAll('a')].map((a) => a.getAttribute('href'));
    expect(hrefs).toEqual(['https://www.maeda.invalid']);
    expect(container.textContent).toContain('<mailto:taro@maeda.invalid>');
  });
});

describe('UnseenQuotes の見出しの日時', () => {
  const stamp = () => container.querySelector('span.tabular-nums');

  it('表記はバブルと同じにし、見出しの元の文字列はツールチップに残す', () => {
    const q = {
      ...quote('本文'),
      at: 'Tue, 20 Oct 2026 11:21:08 +0900',
      date: '2026-10-20T11:21:08+09:00',
    };
    act(() => root.render(<UnseenQuotes quotes={[q]} out={false} />));
    expect(stamp()?.textContent).toBe(formatDateTime('2026-10-20T11:21:08+09:00'));
    expect(stamp()?.getAttribute('title')).toBe('Tue, 20 Oct 2026 11:21:08 +0900');
  });

  it('読んだ日時が無ければ、元の文字列をそのまま出す', () => {
    const q = { ...quote('本文'), at: '2026/10/02 12:10', date: '' };
    act(() => root.render(<UnseenQuotes quotes={[q]} out={false} />));
    expect(stamp()?.textContent).toBe('2026/10/02 12:10');
  });
});

describe('枠の並び（古い順・親のバブルの上）', () => {
  /** 本文に出てくる順（転送の外側 → 内側＝新しい → 古い）。日時は時差あり・なしを混ぜる */
  const at = (subject: string, date: string): UnseenQuote => ({ ...quote(subject), subject, date });
  const inBody = [
    at('e', '2026-10-19T22:00:00+09:00'),
    at('d', '2026-10-15T09:00:00'),
    at('c', '2026-10-09T15:29:01+09:00'),
    at('b', '2026-10-07T17:55:00'),
    at('a', '2026-10-02T12:10:00'),
  ];
  const subjects = (qs: UnseenQuote[]) => qs.map((q) => q.subject);

  it('本文の新しい順を、古い順に並べ替える', () => {
    expect(subjects(oldestFirst(inBody))).toEqual(['a', 'b', 'c', 'd', 'e']);
  });

  it('本文の順が日時の順と食い違っても、日時で並べる', () => {
    expect(subjects(oldestFirst([inBody[2], inBody[0], inBody[4]]))).toEqual(['a', 'c', 'e']);
  });

  it('日時の読めない枠は、本文で直前の枠のすぐ上（古い側）に置く', () => {
    const lost = at('c の続き', '');
    expect(subjects(oldestFirst([inBody[0], inBody[2], lost, inBody[4]]))).toEqual([
      'a',
      'c の続き',
      'c',
      'e',
    ]);
  });

  it('本文の先頭の枠が読めなければ、直後の枠のすぐ下（新しい側）に置く', () => {
    const lost = at('先頭', '');
    expect(subjects(oldestFirst([lost, inBody[2], inBody[4]]))).toEqual(['a', 'c', '先頭']);
  });

  it('つまみが DOM の順に読む日時は、枠 → 親のバブルで上から下へ単調に増える', () => {
    act(() =>
      root.render(
        <>
          <UnseenQuotes quotes={inBody} out={false} />
          <div {...markAttrs({ date: '2026-10-20T09:00:00+09:00' })} />
        </>
      )
    );
    const times = readMarks(container).map((m) => parseQuotedDate(m.date ?? '')?.getTime() ?? NaN);
    expect(times).toHaveLength(6);
    times.slice(1).forEach((t, i) => expect(t).toBeGreaterThan(times[i]));
  });
});
