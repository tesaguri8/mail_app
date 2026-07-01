import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Merge, RefreshCw } from 'lucide-react';
import type { DuplicateGroup } from '@bindings/DuplicateGroup';
import { contactFindDuplicates, contactMerge } from '../services/contacts';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 重複整理パネル。正規化した表示名でまとめた重複候補を一覧し、
 * 残す 1 件を選んで残りを統合する（メール共有の同僚を誤って束ねないため氏名ベース）。
 */
export function ContactDuplicates({ onMerged }: { onMerged: () => void }) {
  const { t } = useTranslation();
  const [groups, setGroups] = useState<DuplicateGroup[]>([]);
  const [loading, setLoading] = useState(false);
  // グループごとに「残す連絡先 ID」。既定は先頭。
  const [keep, setKeep] = useState<Record<number, number>>({});
  const [busy, setBusy] = useState<number | null>(null);

  const load = () => {
    if (!isTauri) return;
    setLoading(true);
    contactFindDuplicates()
      .then((g) => {
        setGroups(g);
        setKeep(Object.fromEntries(g.map((grp) => [grp.contacts[0].id, grp.contacts[0].id])));
      })
      .catch(() => undefined)
      .finally(() => setLoading(false));
  };
  useEffect(load, []);

  const mergeGroup = async (group: DuplicateGroup) => {
    const gid = group.contacts[0].id;
    const keepId = keep[gid] ?? gid;
    const dropIds = group.contacts.map((c) => c.id).filter((id) => id !== keepId);
    if (dropIds.length === 0) return;
    setBusy(gid);
    try {
      await contactMerge(keepId, dropIds);
      setGroups((prev) => prev.filter((g) => g !== group));
      onMerged();
    } catch {
      /* noop */
    } finally {
      setBusy(null);
    }
  };

  const total = groups.reduce((n, g) => n + g.contacts.length - 1, 0);

  return (
    <div className="mx-auto max-w-2xl p-6">
      <div className="mb-4 flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold">{t('dupes.title')}</h2>
          <p className="text-xs text-white/50">
            {loading
              ? t('dupes.scanning')
              : groups.length === 0
                ? t('dupes.none')
                : t('dupes.summary', { groups: groups.length, extra: total })}
          </p>
        </div>
        <button
          onClick={load}
          disabled={loading}
          title={t('dupes.rescan')}
          aria-label={t('dupes.rescan')}
          className="flex h-9 w-9 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 disabled:opacity-40"
        >
          <RefreshCw size={16} className={loading ? 'animate-spin' : ''} />
        </button>
      </div>

      <ul className="space-y-3">
        {groups.map((group) => {
          const gid = group.contacts[0].id;
          const keepId = keep[gid] ?? gid;
          return (
            <li key={gid} className="rounded-lg bg-white/5 p-3">
              <div className="mb-2 flex items-center justify-between">
                <span className="text-sm font-medium">
                  {group.label}{' '}
                  <span className="text-white/40">
                    {t('dupes.count', { count: group.contacts.length })}
                  </span>
                </span>
                <button
                  onClick={() => mergeGroup(group)}
                  disabled={busy === gid}
                  className="flex items-center gap-1.5 rounded-md bg-white/15 px-3 py-1.5 text-xs font-medium hover:bg-white/25 disabled:opacity-40"
                >
                  <Merge size={14} />
                  {t('dupes.merge')}
                </button>
              </div>
              <ul className="space-y-1">
                {group.contacts.map((c) => (
                  <li key={c.id}>
                    <label
                      className={`flex cursor-pointer items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm ${
                        keepId === c.id ? 'bg-emerald-400/15' : 'hover:bg-white/5'
                      }`}
                    >
                      <span
                        className={`flex h-4 w-4 shrink-0 items-center justify-center rounded-full ${
                          keepId === c.id ? 'bg-emerald-400 text-black' : 'border border-white/30'
                        }`}
                      >
                        {keepId === c.id && <Check size={11} />}
                      </span>
                      <input
                        type="radio"
                        className="hidden"
                        name={`keep-${gid}`}
                        checked={keepId === c.id}
                        onChange={() => setKeep((k) => ({ ...k, [gid]: c.id }))}
                      />
                      <span className="min-w-0 flex-1 truncate">
                        {c.display_name}
                        {c.organization && (
                          <span className="text-white/40"> · {c.organization}</span>
                        )}
                      </span>
                      <span className="shrink-0 truncate text-xs text-white/45">
                        {[c.email, c.phone].filter(Boolean).join(' / ')}
                      </span>
                      {keepId === c.id && (
                        <span className="shrink-0 text-[10px] font-medium text-emerald-300">
                          {t('dupes.keep')}
                        </span>
                      )}
                    </label>
                  </li>
                ))}
              </ul>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
