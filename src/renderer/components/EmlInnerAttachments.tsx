import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { BookOpen, ChevronDown, Download, Loader2 } from 'lucide-react';
import type { AttachmentSummary } from '@bindings/AttachmentSummary';
import type { EmlContents } from '@bindings/EmlContents';
import type { EmlAttachment } from '@bindings/EmlAttachment';
import { attachmentEmlContents } from '../services/mail';
import { trackAttachment, type AttachmentRef } from '../stores/attachmentDownloads';
import { withActivity } from '../stores/activity';
import { attachmentErrorReason, openEmlPart, saveEmlPart } from '../utils/attachmentSave';
import { formatSize } from '../utils/fileSize';
import { useToast } from './SyncProvider';

/** 添付された転送メール（message/rfc822・.eml）か。 */
export function isEmlAttachment(a: Pick<AttachmentSummary, 'content_type' | 'filename'>): boolean {
  return a.content_type?.toLowerCase() === 'message/rfc822' || /\.eml$/i.test(a.filename.trim());
}

/**
 * 添付された転送メール（.eml）の中の添付を、添付の行の下に開いて出す。
 * 同期は .eml の中を辿らないので、開いたときに .eml を取得して中身を読む（取得の進み具合は .eml の行に出る）。
 * 中の添付は 1 件ずつ開く・保存できる（docs/THREADING.md §5）。
 */
export function EmlInnerAttachments({ att }: { att: AttachmentRef }) {
  const { t } = useTranslation();
  const toast = useToast();
  const [open, setOpen] = useState(false);
  const [contents, setContents] = useState<EmlContents | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busyIndex, setBusyIndex] = useState<number | null>(null);

  const toggle = async () => {
    if (open) {
      setOpen(false);
      return;
    }
    setOpen(true);
    if (contents || loading) return;
    setLoading(true);
    setError(null);
    try {
      setContents(
        await trackAttachment(att, () =>
          withActivity(t('activity.downloadingAttachment'), () => attachmentEmlContents(att.id))
        )
      );
    } catch (e) {
      setError(attachmentErrorReason(e, t));
    } finally {
      setLoading(false);
    }
  };

  const run = async (p: EmlAttachment, mode: 'open' | 'save') => {
    setBusyIndex(p.index);
    try {
      if (mode === 'open') {
        await openEmlPart(att, p.index, t('activity.openingAttachment'));
      } else if (await saveEmlPart(att, p, t('activity.downloadingAttachment'))) {
        toast(t('mailbox.attachmentSaved'));
      }
    } catch (e) {
      const reason = attachmentErrorReason(e, t);
      toast(
        t(mode === 'open' ? 'mailbox.attachmentOpenFailed' : 'mailbox.attachmentSaveFailed', {
          reason,
        }),
        'error'
      );
    } finally {
      setBusyIndex(null);
    }
  };

  return (
    <div className="mt-1.5">
      <button
        onClick={() => void toggle()}
        className="flex items-center gap-1 text-xs text-white/50 hover:text-white/75"
      >
        <ChevronDown size={12} className={`transition-transform ${open ? '' : '-rotate-90'}`} />
        {open ? t('mailbox.emlInnerHide') : t('mailbox.emlInnerShow')}
      </button>
      {open && (
        <div className="mt-1 border-l border-white/15 pl-3">
          {loading && (
            <div className="flex items-center gap-1.5 py-1 text-xs text-white/40">
              <Loader2 size={11} className="animate-spin" />
              {t('mailbox.emlInnerLoading')}
            </div>
          )}
          {error && (
            <div className="py-1 text-xs text-rose-300/80">
              {t('mailbox.emlInnerFailed', { reason: error })}
            </div>
          )}
          {contents && (
            <>
              {(contents.from || contents.subject) && (
                <div
                  className="truncate py-0.5 text-[11px] text-white/45"
                  title={contents.from ?? ''}
                >
                  {[contents.from, contents.subject].filter(Boolean).join('・')}
                </div>
              )}
              {contents.attachments.length === 0 ? (
                <div className="py-1 text-xs text-white/40">{t('mailbox.emlInnerNone')}</div>
              ) : (
                <ul className="space-y-1 py-0.5">
                  {contents.attachments.map((p) => (
                    <li key={p.index} className="flex items-center gap-3">
                      <span
                        className="min-w-0 flex-1 truncate text-sm text-white/80"
                        title={p.filename}
                      >
                        {p.filename}
                      </span>
                      <span className="shrink-0 text-xs text-white/40">{formatSize(p.size)}</span>
                      <button
                        onClick={() => void run(p, 'open')}
                        disabled={busyIndex === p.index}
                        title={t('mailbox.attachmentOpen')}
                        aria-label={t('mailbox.attachmentOpen')}
                        className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-white/10 text-white/80 hover:bg-white/20 disabled:opacity-50"
                      >
                        <BookOpen size={12} />
                      </button>
                      <button
                        onClick={() => void run(p, 'save')}
                        disabled={busyIndex === p.index}
                        title={t('mailbox.attachmentDownload')}
                        aria-label={t('mailbox.attachmentDownload')}
                        className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-white/10 text-white/80 hover:bg-white/20 disabled:opacity-50"
                      >
                        <Download size={12} />
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
