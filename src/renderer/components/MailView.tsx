import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { MailDetail } from '@bindings/MailDetail';

function formatDate(d: string | null): string {
  if (!d) return '';
  const dt = new Date(d);
  return isNaN(dt.getTime()) ? d : dt.toLocaleString();
}

/**
 * メール本文の表示（モーダル）。Phase A: プレーン本文のみ（HTML/リモート画像は
 * 後続でサニタイズ＋ブロック方針。docs/MAIL_SECURITY.md）。
 * 既定は引用除去後の clean_body、トグルで全文（引用込み）を表示。
 */
export function MailView({ detail, onClose }: { detail: MailDetail; onClose: () => void }) {
  const { t } = useTranslation();
  const [showQuotes, setShowQuotes] = useState(false);

  const clean = detail.clean_body ?? '';
  const full = detail.body_plain ?? '';
  const hasQuotedExtra = full.trim().length > clean.trim().length;
  const body = showQuotes ? full : clean || full;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-6"
      onClick={onClose}
    >
      <div
        className="flex max-h-[80vh] w-[640px] max-w-full flex-col rounded-xl bg-neutral-900/95 text-white shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="border-b border-white/10 px-5 py-3">
          <div className="flex items-start justify-between gap-3">
            <h3 className="text-base font-semibold">
              {detail.subject ?? '(no subject)'} {detail.has_attachments && '📎'}
            </h3>
            <button
              className="rounded-md px-2 py-1 text-sm text-white/70 hover:bg-white/10"
              onClick={onClose}
            >
              ✕
            </button>
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

        <div className="flex-1 overflow-y-auto px-5 py-4">
          {body.trim() ? (
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
    </div>
  );
}
