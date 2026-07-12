import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Info } from 'lucide-react';
import type { SpamSenderConflict } from '@bindings/SpamSenderConflict';
import { spamFindConflicts, spamForgiveSender } from '../services/spam';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 迷惑差出人に登録済みだが住所録/グリーンと矛盾する差出人（＝誤登録の可能性）を
 * 情報アイコン＋件数バッジで知らせ、ポップオーバーから 1 件ずつ「迷惑解除」できる。
 * 矛盾が無ければ何も表示しない。
 */
export function SpamConflictAlert({ onResolved }: { onResolved?: () => void }) {
  const { t } = useTranslation();
  const [conflicts, setConflicts] = useState<SpamSenderConflict[]>([]);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  const load = useCallback(() => {
    if (!isTauri) return;
    spamFindConflicts()
      .then(setConflicts)
      .catch(() => setConflicts([]));
  }, []);
  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    return () => document.removeEventListener('mousedown', onDoc);
  }, [open]);

  if (conflicts.length === 0) return null;

  const reasonLabel = (r: string) =>
    r === 'green'
      ? t('spamAlert.reasonGreen')
      : r === 'contact_green'
        ? t('spamAlert.reasonContactGreen')
        : t('spamAlert.reasonContact');

  const forgive = async (address: string) => {
    try {
      await spamForgiveSender(address);
      setConflicts((cs) => {
        const next = cs.filter((c) => c.address !== address);
        if (next.length === 0) setOpen(false);
        return next;
      });
      onResolved?.();
    } catch {
      /* noop */
    }
  };

  return (
    <div ref={ref} className="relative shrink-0">
      <button
        onClick={() => setOpen((v) => !v)}
        title={t('spamAlert.title', { count: conflicts.length })}
        aria-label={t('spamAlert.title', { count: conflicts.length })}
        className="relative flex h-8 w-8 items-center justify-center rounded-md text-amber-300 hover:bg-white/10"
      >
        <Info size={16} />
        <span className="absolute -right-0.5 -top-0.5 flex h-3.5 min-w-[14px] items-center justify-center rounded-full bg-amber-400 px-0.5 text-[9px] font-bold leading-none text-black">
          {conflicts.length}
        </span>
      </button>

      {open && (
        <div className="absolute right-0 top-full z-30 mt-1 w-72 rounded-md border border-white/15 bg-neutral-900/80 p-3 shadow-xl backdrop-blur">
          <div className="mb-2 flex items-start gap-1.5 text-xs text-amber-100">
            <Info size={14} className="mt-0.5 shrink-0" />
            <span>{t('spamAlert.banner', { count: conflicts.length })}</span>
          </div>
          <ul className="max-h-64 space-y-1 overflow-y-auto">
            {conflicts.map((c) => (
              <li
                key={c.address}
                className="flex items-center gap-2 rounded bg-white/5 px-2 py-1.5 text-xs"
              >
                <span className="min-w-0 flex-1 truncate">
                  {c.display_name && <span className="font-medium">{c.display_name} </span>}
                  <span className="text-white/50">{c.address}</span>
                  <span className="ml-1 text-[10px] text-amber-200/80">{reasonLabel(c.reason)}</span>
                </span>
                <button
                  onClick={() => forgive(c.address)}
                  className="shrink-0 rounded bg-emerald-500/20 px-2 py-0.5 text-[11px] text-emerald-200 hover:bg-emerald-500/30"
                >
                  {t('spamAlert.forgive')}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
