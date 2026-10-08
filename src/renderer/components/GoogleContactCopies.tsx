import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Trash2 } from 'lucide-react';
import { gcontactsCopiesCount, gcontactsCopiesTrash } from '../services/gcontacts';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * ファイル（CSV／vCard）で取り込んだ Google 連絡先の「写し」の片付け。
 *
 * 同期に置き換えたあとも写しが残ると、同じ人が 2 件になる。対象は同期とつながって
 * いない写しだけで、消しても Google には何も送られない（docs/CONTACTS_SYNC.md §2）。
 * 「住所録へ反映」を先に済ませると、一致した写しは同期の側へ移る（お気に入り・タグ・
 * メモが残る）ので、案内文でその順を勧める。
 *
 * 確認は画面内で行う。`window.confirm` は Linux（WebKitGTK）で出ずに素通りするため使わない
 * （docs/CROSS_CUTTING.md #21）。
 */
export function GoogleContactCopies({ version }: { version: number }) {
  const { t } = useTranslation();
  const [count, setCount] = useState(0);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // 取り込み・住所録への反映のたびに数え直す（version が変わる）。
  useEffect(() => {
    if (!isTauri) return;
    gcontactsCopiesCount()
      .then(setCount)
      .catch(() => setCount(0));
  }, [version]);

  const trash = async () => {
    setBusy(true);
    setError(null);
    try {
      const n = await gcontactsCopiesTrash();
      setMessage(t('settings.gcontactsCopiesDone', { count: n }));
      setCount(0);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setConfirming(false);
    }
  };

  if (count === 0) {
    return message ? <p className="text-sm text-emerald-300">{message}</p> : null;
  }
  return (
    <div className="space-y-2 rounded-lg border border-white/15 bg-white/5 p-3">
      <p className="text-sm text-white/80">{t('settings.gcontactsCopies', { count })}</p>
      <p className="text-xs leading-relaxed text-white/50">{t('settings.gcontactsCopiesHint')}</p>
      {confirming ? (
        <div className="space-y-2">
          <p className="text-sm text-amber-200">
            {t('settings.gcontactsCopiesConfirm', { count })}
          </p>
          <div className="flex items-center gap-2">
            <button
              onClick={trash}
              disabled={busy}
              className="rounded-md bg-rose-500/80 px-3 py-1.5 text-xs font-medium hover:bg-rose-500 disabled:opacity-40"
            >
              {t('settings.gcontactsCopiesRun')}
            </button>
            <button
              onClick={() => setConfirming(false)}
              disabled={busy}
              className="rounded-md border border-white/20 px-3 py-1.5 text-xs text-white/70 hover:bg-white/10 disabled:opacity-40"
            >
              {t('settings.gcontactsMatchCancel')}
            </button>
          </div>
        </div>
      ) : (
        <button
          onClick={() => setConfirming(true)}
          className="flex items-center gap-1.5 rounded-md border border-white/20 px-3 py-1.5 text-xs text-white/80 hover:bg-white/10"
        >
          <Trash2 size={13} />
          {t('settings.gcontactsCopiesTrash')}
        </button>
      )}
      {error && (
        <p className="text-sm text-red-300">{t('settings.gcalError', { message: error })}</p>
      )}
    </div>
  );
}
