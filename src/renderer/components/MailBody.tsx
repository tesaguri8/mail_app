import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Forward, Reply, ReplyAll } from 'lucide-react';
import type { MailDetail } from '@bindings/MailDetail';
import { HtmlText } from './HtmlText';

function formatDate(d: string | null): string {
  if (!d) return '';
  const dt = new Date(d);
  return isNaN(dt.getTime()) ? d : dt.toLocaleString();
}

/**
 * メール本文の表示（インライン）。Phase: プレーン本文のみ（HTML/リモート画像は
 * 後続でサニタイズ＋ブロック。docs/MAIL_SECURITY.md）。既定は引用除去後の clean_body。
 */
export function MailBody({ detail }: { detail: MailDetail }) {
  const { t } = useTranslation();
  const [showQuotes, setShowQuotes] = useState(false);
  const [note, setNote] = useState('');

  // 作成機能は後続。今はアイコン配置とフィードバックのみ。
  const composeStub = () => setNote(t('comingSoon'));
  const COMPOSE_ACTIONS = [
    { key: 'reply', Icon: Reply },
    { key: 'replyAll', Icon: ReplyAll },
    { key: 'forward', Icon: Forward },
  ] as const;

  const clean = detail.clean_body ?? '';
  const full = detail.body_plain ?? '';
  const html = detail.body_html?.trim() ?? '';
  const hasHtml = html.length > 0;
  const hasQuotedExtra = !hasHtml && full.trim().length > clean.trim().length;
  const body = showQuotes ? full : clean || full;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="border-b border-white/10 px-5 py-3">
        <div className="flex items-start justify-between gap-3">
          <h3 className="min-w-0 text-base font-semibold">
            {detail.subject ?? '(no subject)'} {detail.has_attachments && '📎'}
          </h3>
          <div className="flex shrink-0 items-center gap-1">
            {COMPOSE_ACTIONS.map(({ key, Icon }) => (
              <button
                key={key}
                onClick={composeStub}
                title={t(`compose.${key}`)}
                aria-label={t(`compose.${key}`)}
                className="flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80"
              >
                <Icon size={16} />
              </button>
            ))}
            {note && <span className="ml-1 text-[10px] text-white/45">{note}</span>}
          </div>
        </div>
        <div className="mt-1 text-xs text-white/50">
          <div>
            {t('mailbox.from')}: {detail.from_address ?? '—'}
          </div>
          {detail.to_addresses && (
            <div>
              {t('mailbox.to')}: {detail.to_addresses}
            </div>
          )}
          <div>{formatDate(detail.date)}</div>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        {hasHtml ? (
          <HtmlText html={html} />
        ) : body.trim() ? (
          <pre className="whitespace-pre-wrap break-words font-sans text-sm leading-relaxed text-white/90">
            {body}
          </pre>
        ) : (
          <p className="text-sm text-white/40">{t('mailbox.noBody')}</p>
        )}
      </div>

      {hasQuotedExtra && (
        <div className="border-t border-white/10 px-5 py-2">
          <button
            className="text-xs text-sky-300 hover:underline"
            onClick={() => setShowQuotes((v) => !v)}
          >
            {showQuotes ? t('mailbox.hideQuotes') : t('mailbox.showQuotes')}
          </button>
        </div>
      )}
    </div>
  );
}
