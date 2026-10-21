import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowDown, MailPlus, Sparkles } from 'lucide-react';
import type { UnseenQuote } from '@bindings/UnseenQuote';
import { AutoLinkText, hasAutoLinkUrl } from './HtmlText';
import { formatQuotedDateTime, parseQuotedDate } from '../utils/datetime';
import { markAttrs } from './timeline/marks';

/**
 * 枠を古い順に並べる（会話のバブルと同じく、上が古く下が新しい。docs/THREADING.md §5）。
 * 本文の中では新しい順（転送の外側 → 内側）に出てくるので、本文の逆順を土台にして日時で並べ替える。
 *
 * 日時の読めない枠は、本文で直前にあった（＝その続きとして枠になった）日時のある枠に寄せ、そのすぐ上
 * （古い側）に置く。本文の先頭にあって直前が無ければ、本文で直後の枠に寄せ、そのすぐ下に置く。
 * どちらも無ければ（全部読めない）本文の逆順のまま。同じ日時どうしも本文の逆順を保つ。
 *
 * 差出人を推定した枠（`guessed`。返信のいちばん新しい引用で、日時は分からない）は、日時の分かる枠の
 * うち、いちばん新しいものの次（親のバブルのすぐ上）に置く。
 */
export function oldestFirst(quotes: readonly UnseenQuote[]): UnseenQuote[] {
  const times = quotes.map((q) => (q.date ? (parseQuotedDate(q.date)?.getTime() ?? null) : null));
  // 寄せる先の日時: 推定の枠はいちばん後ろ。ほかは本文で直前（i より前）の日時のある枠、無ければ直後
  const keyOf = (i: number): number => {
    if (quotes[i].guessed) return Number.POSITIVE_INFINITY;
    for (let j = i; j >= 0; j--) if (times[j] !== null) return times[j] as number;
    for (let j = i + 1; j < times.length; j++) if (times[j] !== null) return times[j] as number;
    return 0;
  };
  return quotes
    .map((q, i) => ({ q, key: keyOf(i), order: quotes.length - 1 - i }))
    .sort((a, b) => a.key - b.key || a.order - b.order)
    .map((e) => e.q);
}

/**
 * このメールに含まれていた、手元に無いメール（転送・Outlook の「元のメッセージ」で初めて届いた内容）。
 * 「引用を表示」に畳むと見落とすので、1 通ずつ小さなバブルで出す（docs/THREADING.md §5）。
 * 手元にある引用は、いままでどおり親のバブルの「引用を表示」に畳む。
 *
 * 見た目はふつうのバブルと同じ地の色で、枠線だけ点線にして「手元に届いたメールではない」ことを示す
 * （色を変えると、こちらが主役に見えてしまう）。説明の文言は、いちばん上の見出し（何通あるか）と、
 * 封筒のアイコンのツールチップに出す。
 *
 * 親のバブルの**上**に、古い順に並べる（会話全体が上から下へ時間どおりになり、日付のつまみも逆向きに
 * ならない）。どのバブルのものかは、見出しの下向きの矢印・文言と、まとまりの横の点線で示す
 * （点線はバブルの寄せる側に引き、まとまりの下端まで伸ばしてバブルに届かせる）。
 */
export function UnseenQuotes({
  quotes,
  out,
  highlight,
  renderDate,
}: {
  quotes: UnseenQuote[];
  /** 親のバブルが自分の送信か（寄せる側をそろえる）。 */
  out: boolean;
  highlight?: string[];
  renderDate?: (raw: string) => ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <div
      className={`mb-1.5 flex flex-col gap-1.5 border-dashed border-white/20 pb-1.5 ${
        out ? 'items-end border-r pr-2' : 'items-start border-l pl-2'
      }`}
    >
      <div className="flex items-center gap-1 px-1 text-[10px] text-white/40">
        <ArrowDown size={10} aria-hidden />
        {t('thread.unseenQuotesHeading', { count: quotes.length })}
      </div>
      {oldestFirst(quotes).map((q, i) => {
        return (
          <div
            key={i}
            // 日付のつまみに 1 件として載せる（timeline/marks.ts）。添付は外側のメールのもので
            // この枠のものか分からないので、リンクだけ載せる
            {...markAttrs({ date: q.date, quote: true, link: hasAutoLinkUrl(q.body) })}
            className="w-full max-w-[92%] rounded-xl border border-dashed border-white/30 bg-white/10 px-3 py-1.5 text-[13px] leading-relaxed text-white/90"
          >
            <div className="mb-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[10px]">
              <span
                className="inline-flex items-center text-white/50"
                title={t('thread.unseenQuote')}
                aria-label={t('thread.unseenQuote')}
                role="img"
              >
                <MailPlus size={11} />
              </span>
              <span className="text-white/60" title={q.from}>
                {q.sender}
              </span>
              {q.guessed ? (
                // 差出人の行が無い「>」だけの引用: 差出人は返信の相手から推定、日時は分からない
                <>
                  <span
                    className="inline-flex items-center gap-0.5 rounded-full border border-amber-300/40 px-1.5 text-amber-200/80"
                    title={t('thread.quoteGuessedHint')}
                  >
                    <Sparkles size={9} aria-hidden />
                    {t('thread.quoteGuessed')}
                  </span>
                  <span className="text-white/45">{t('thread.quoteDateUnknown')}</span>
                </>
              ) : (
                // 表記はバブルにそろえ、見出しに書かれていた元の文字列はツールチップに残す
                <span className="tabular-nums text-white/45" title={q.at ?? undefined}>
                  {q.date ? formatQuotedDateTime(q.date, q.at ?? '') : q.at}
                </span>
              )}
              {q.subject && <span className="min-w-0 truncate text-white/45">{q.subject}</span>}
            </div>
            <AutoLinkText text={q.body} highlight={highlight} renderDate={renderDate} />
          </div>
        );
      })}
    </div>
  );
}
