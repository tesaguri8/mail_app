import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { MergeRemoteDeletion } from '@bindings/MergeRemoteDeletion';
import { contactGoogleDuplicates, contactGoogleDuplicatesTidy } from '../services/contacts';
import { remoteDeletionNotes, remoteDeletionTotal } from '../utils/mergeRemote';
import { ConfirmDialog } from './ConfirmDialog';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 以前の統合で残った Google の重複（1 人の連絡先に同じアカウントの ID が 2 つ以上）の案内。
 * 「まとめますか？」で了承を取ってから、統合と同じ規則（1 つ残し、余りは次の同期で Google 側から
 * 削除）を当てる。黙って消さない（docs/CONTACTS_SYNC.md §3-5）。重複が無ければ何も出さない。
 *
 * `reloadKey` が変わると数え直す（統合のあとなど）。
 */
export function GoogleDuplicateNotice({ reloadKey }: { reloadKey: number }) {
  const { t } = useTranslation();
  const [found, setFound] = useState<MergeRemoteDeletion[]>([]);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<number | null>(null);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!isTauri) return;
    contactGoogleDuplicates()
      .then(setFound)
      .catch(() => setFound([]));
  }, [reloadKey]);

  const total = remoteDeletionTotal(found);

  const tidy = async () => {
    setBusy(true);
    setError('');
    try {
      setDone(await contactGoogleDuplicatesTidy());
      setConfirming(false);
      setFound(await contactGoogleDuplicates());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (total === 0 && done == null) return null;
  return (
    <div className="rounded-md bg-amber-400/10 px-2.5 py-2 text-xs text-amber-100/90">
      {total > 0 ? (
        <div className="flex items-center gap-2">
          <span className="flex-1">{t('dupes.googleLeftover', { count: total })}</span>
          <button
            onClick={() => setConfirming(true)}
            className="shrink-0 rounded bg-white/15 px-2 py-1 font-medium hover:bg-white/25"
          >
            {t('dupes.googleLeftoverRun')}
          </button>
        </div>
      ) : (
        <span>{t('dupes.googleLeftoverDone', { count: done ?? 0 })}</span>
      )}
      {error && <p className="mt-1 text-red-300">{error}</p>}
      {confirming && (
        <ConfirmDialog
          title={t('dupes.googleLeftoverTitle')}
          body={t('dupes.googleLeftoverBody')}
          notes={remoteDeletionNotes(found, t)}
          confirmLabel={t('dupes.googleLeftoverRun')}
          danger
          busy={busy}
          onConfirm={() => void tidy()}
          onCancel={() => setConfirming(false)}
        />
      )}
    </div>
  );
}
