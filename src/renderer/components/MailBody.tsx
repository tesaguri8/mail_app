import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Download, Forward, Paperclip, Reply, ReplyAll } from 'lucide-react';
import type { MailDetail } from '@bindings/MailDetail';
import type { AttachmentSummary } from '@bindings/AttachmentSummary';
import { attachmentDownload, attachmentOpen, mailAttachments } from '../services/mail';
import { HtmlText } from './HtmlText';

function formatDate(d: string | null): string {
  if (!d) return '';
  const dt = new Date(d);
  return isNaN(dt.getTime()) ? d : dt.toLocaleString();
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/**
 * メール本文の表示（インライン）。Phase: プレーン本文のみ（HTML/リモート画像は
 * 後続でサニタイズ＋ブロック。docs/MAIL_SECURITY.md）。既定は引用除去後の clean_body。
 */
export function MailBody({ detail }: { detail: MailDetail }) {
  const { t } = useTranslation();
  const [showQuotes, setShowQuotes] = useState(false);
  const [note, setNote] = useState('');
  const [attachments, setAttachments] = useState<AttachmentSummary[]>([]);
  const [busyId, setBusyId] = useState<number | null>(null);

  // メール切り替えごとに添付メタを読み込む（本体は押下時に取得）。
  useEffect(() => {
    let active = true;
    setAttachments([]);
    if (detail.has_attachments) {
      mailAttachments(detail.id)
        .then((a) => active && setAttachments(a))
        .catch(() => {});
    }
    return () => {
      active = false;
    };
  }, [detail.id, detail.has_attachments]);

  // 未取得ならダウンロードしてから、OS の関連アプリで開く。
  const handleAttachment = async (a: AttachmentSummary) => {
    setBusyId(a.id);
    setNote('');
    try {
      if (!a.is_downloaded) {
        const updated = await attachmentDownload(a.id);
        setAttachments((list) => list.map((x) => (x.id === a.id ? updated : x)));
      }
      await attachmentOpen(a.id);
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusyId(null);
    }
  };

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

      {attachments.length > 0 && (
        <div className="border-t border-white/10 px-5 py-3">
          <div className="mb-2 text-xs font-medium text-white/50">
            {t('mailbox.attachments')} ({attachments.length})
          </div>
          <ul className="space-y-1.5">
            {attachments.map((a) => (
              <li
                key={a.id}
                className="flex items-center gap-3 rounded-md bg-white/5 px-3 py-2"
              >
                <Paperclip size={14} className="shrink-0 text-white/40" />
                <span className="min-w-0 flex-1 truncate text-sm text-white/85" title={a.filename}>
                  {a.filename}
                </span>
                <span className="shrink-0 text-xs text-white/40">{formatSize(a.size)}</span>
                <button
                  onClick={() => handleAttachment(a)}
                  disabled={busyId === a.id}
                  className="flex shrink-0 items-center gap-1 rounded-md bg-white/10 px-2.5 py-1 text-xs text-white/80 hover:bg-white/20 disabled:opacity-50"
                >
                  {busyId === a.id ? (
                    t('mailbox.attachmentBusy')
                  ) : a.is_downloaded ? (
                    t('mailbox.attachmentOpen')
                  ) : (
                    <>
                      <Download size={12} />
                      {t('mailbox.attachmentDownload')}
                    </>
                  )}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

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
