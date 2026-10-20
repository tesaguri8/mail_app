import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { MailPlus } from 'lucide-react';
import type { UnseenQuote } from '@bindings/UnseenQuote';
import { AutoLinkText } from './HtmlText';
import { formatQuotedDateTime } from '../utils/datetime';
import { TIMELINE_DATE_ATTR, TIMELINE_QUOTE_ATTR } from './timeline/marks';

/**
 * このメールに含まれていた、手元に無いメール（転送・Outlook の「元のメッセージ」で初めて届いた内容）。
 * 「引用を表示」に畳むと見落とすので、1 通ずつ小さなバブルで出す（docs/THREADING.md §5）。
 * 手元にある引用は、いままでどおり親のバブルの「引用を表示」に畳む。
 *
 * 見た目はふつうのバブルと同じ地の色で、枠線だけ点線にして「手元に届いたメールではない」ことを示す
 * （色を変えると、こちらが主役に見えてしまう）。説明の文言は、いちばん上の見出し（何通あるか）と、
 * 封筒のアイコンのツールチップに出す。
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
    <div className={`mt-1.5 flex flex-col gap-1.5 ${out ? 'items-end' : 'items-start'}`}>
      <div className="px-1 text-[10px] text-white/40">
        {t('thread.unseenQuotesHeading', { count: quotes.length })}
      </div>
      {quotes.map((q, i) => {
        return (
          <div
            key={i}
            // 日付のつまみに 1 件として載せる（timeline/marks.ts）
            {...{ [TIMELINE_DATE_ATTR]: q.date, [TIMELINE_QUOTE_ATTR]: '' }}
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
              {/* 表記はバブルにそろえ、見出しに書かれていた元の文字列はツールチップに残す */}
              <span className="tabular-nums text-white/45" title={q.at}>
                {q.date ? formatQuotedDateTime(q.date, q.at) : q.at}
              </span>
              {q.subject && <span className="min-w-0 truncate text-white/45">{q.subject}</span>}
            </div>
            <AutoLinkText text={q.body} highlight={highlight} renderDate={renderDate} />
          </div>
        );
      })}
    </div>
  );
}
