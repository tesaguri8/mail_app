import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Layers } from 'lucide-react';
import type { SureMergePreview } from '@bindings/SureMergePreview';
import { contactSureMerge, contactSureMergePreview } from '../services/contacts';
import { remoteDeletionNotes, remoteDeletionTotal } from '../utils/mergeRemote';
import { ConfirmDialog } from './ConfirmDialog';

/**
 * 重複の整理の「確実な重複をまとめて統合」（docs/CONTACTS_SYNC.md §3-5）。
 * 押すと下見（N 組・M 件を N 件に／Google から K 件削除・組の一覧）を出し、了承したらまとめて統合する。
 * 基準と残す 1 件の選び方は Rust 側（services::sure_duplicates）。
 */
export function SureMergePanel({ onMerged }: { onMerged: () => void }) {
  const { t } = useTranslation();
  const [preview, setPreview] = useState<SureMergePreview | null>(null);
  const [busy, setBusy] = useState<'idle' | 'loading' | 'merging'>('idle');
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');

  const open = async () => {
    setBusy('loading');
    setError('');
    setMessage('');
    try {
      const p = await contactSureMergePreview();
      if (p.groups.length === 0) setMessage(t('dupes.sureNone'));
      else setPreview(p);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy('idle');
    }
  };

  const run = async () => {
    setBusy('merging');
    setError('');
    try {
      const r = await contactSureMerge();
      setPreview(null);
      setMessage(
        t('dupes.sureDone', {
          groups: r.groups,
          merged: r.merged,
          remote: r.remote_deletions,
        })
      );
      onMerged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy('idle');
    }
  };

  const remote = preview ? remoteDeletionTotal(preview.remote_deletions) : 0;
  return (
    <div className="space-y-1">
      <button
        onClick={() => void open()}
        disabled={busy !== 'idle'}
        className="flex w-full items-center justify-center gap-1.5 rounded-md bg-emerald-500/80 px-2.5 py-1.5 text-xs font-medium text-white hover:bg-emerald-500 disabled:opacity-40"
      >
        <Layers size={14} />
        {busy === 'loading' ? t('dupes.sureLoading') : t('dupes.sureOpen')}
      </button>
      {message && <p className="text-xs text-emerald-300">{message}</p>}
      {error && <p className="text-xs text-red-300">{error}</p>}
      {preview && (
        <ConfirmDialog
          title={t('dupes.sureTitle')}
          body={t('dupes.sureBody', {
            groups: preview.groups.length,
            contacts: preview.contacts,
          })}
          notes={[
            t('dupes.sureRule'),
            ...(remote > 0
              ? remoteDeletionNotes(preview.remote_deletions, t, 'dupes.googleDeleteLineBulk')
              : []),
          ]}
          confirmLabel={t('dupes.sureRun', { groups: preview.groups.length })}
          busy={busy === 'merging'}
          onConfirm={() => void run()}
          onCancel={() => setPreview(null)}
        >
          {/* 組の一覧は畳んでおき、開けば全部見られる。 */}
          <details className="mt-3 rounded-md bg-white/5 px-2.5 py-2 text-xs">
            <summary className="cursor-pointer text-white/70">
              {t('dupes.sureList', { groups: preview.groups.length })}
            </summary>
            <ul className="mt-2 max-h-64 space-y-0.5 overflow-y-auto pr-1">
              {preview.groups.map((g) => (
                <li key={g.keep_id} className="flex gap-2 text-white/75">
                  <span className="min-w-0 flex-1 truncate">
                    {g.display_name}
                    <span className="ml-2 text-white/40">
                      {[g.email, g.phone].filter(Boolean).join(' · ')}
                    </span>
                  </span>
                  <span className="shrink-0 text-white/45">
                    {t('dupes.count', { count: g.count })}
                  </span>
                </li>
              ))}
            </ul>
          </details>
        </ConfirmDialog>
      )}
    </div>
  );
}
