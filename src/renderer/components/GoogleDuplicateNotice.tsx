import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { MergeRemoteDeletion } from '@bindings/MergeRemoteDeletion';
import { contactGoogleDuplicates, contactGoogleDuplicatesTidy } from '../services/contacts';
import { remoteDeletionNotes, remoteDeletionTotal } from '../utils/mergeRemote';
import { ConfirmDialog } from './ConfirmDialog';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * Google の重複の帯（黄色）と、その操作ボタン。一括の案内（重複の整理）と連絡先の詳細（同期先の
 * 欄）で見た目をそろえる。ボタンは帯の色の塗りにする — 薄いボタンだと、隣の緑の「確実な重複を
 * まとめて統合」に目が行って見落とされた（利用者の指摘 2026-10-10）。
 */
export function GoogleDuplicateBar({
  text,
  actionLabel,
  onAction,
  disabled = false,
}: {
  text: string;
  actionLabel: string;
  onAction: () => void;
  disabled?: boolean;
}) {
  return (
    // 狭い欄（重複の整理の左）ではボタンが次の行へ回る（文を細切れに折らない）。
    <div className="flex flex-wrap items-center gap-2 rounded-md bg-amber-400/10 px-2.5 py-2 text-xs text-amber-100/90">
      <span className="min-w-[12rem] flex-1">{text}</span>
      <button
        onClick={onAction}
        disabled={disabled}
        className="shrink-0 rounded bg-amber-400 px-2.5 py-1 font-semibold text-amber-950 hover:bg-amber-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-200 disabled:cursor-not-allowed disabled:opacity-50"
      >
        {actionLabel}
      </button>
    </div>
  );
}

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
    <div>
      {total > 0 ? (
        <GoogleDuplicateBar
          text={t('dupes.googleLeftover', { count: total })}
          actionLabel={t('dupes.googleLeftoverRun')}
          onAction={() => setConfirming(true)}
          disabled={busy}
        />
      ) : (
        <div className="rounded-md bg-amber-400/10 px-2.5 py-2 text-xs text-amber-100/90">
          {t('dupes.googleLeftoverDone', { count: done ?? 0 })}
        </div>
      )}
      {error && <p className="mt-1 text-xs text-red-300">{error}</p>}
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
