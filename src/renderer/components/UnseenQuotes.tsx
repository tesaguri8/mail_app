import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { MailPlus } from 'lucide-react';
import type { UnseenQuote } from '@bindings/UnseenQuote';
import { AutoLinkText } from './HtmlText';

/**
 * このメールに含まれていた、手元に無いメール（転送・Outlook の「元のメッセージ」で初めて届いた内容）。
 * 「引用を表示」に畳むと見落とすので、1 通ずつ小さなバブルで出す（docs/THREADING.md §5）。
 * 手元にある引用は、いままでどおり親のバブルの「引用を表示」に畳む。
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
      {quotes.map((q, i) => {
        return (
          <div
            key={i}
            className="w-full max-w-[92%] rounded-xl border border-amber-300/30 bg-amber-400/[0.07] px-3 py-1.5 text-[13px] leading-relaxed text-white/85"
          >
            <div className="mb-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[10px] text-amber-200/80">
              <span className="inline-flex items-center gap-1 font-medium">
                <MailPlus size={11} />
                {t('thread.unseenQuote')}
              </span>
              <span className="text-white/60" title={q.from}>
                {q.sender}
              </span>
              <span className="tabular-nums text-white/45">{q.at}</span>
              {q.subject && <span className="min-w-0 truncate text-white/45">{q.subject}</span>}
            </div>
            <AutoLinkText text={q.body} highlight={highlight} renderDate={renderDate} />
          </div>
        );
      })}
    </div>
  );
}
