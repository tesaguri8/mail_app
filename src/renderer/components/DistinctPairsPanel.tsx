import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, ChevronRight, UserX } from 'lucide-react';
import type { DistinctPair } from '@bindings/DistinctPair';
import { contactDistinctPairs, contactUnmarkDistinct } from '../services/contacts';
import { groupDistinctPairs, type DistinctGroup } from '../utils/distinct';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 「別人」として外した組の取り消しの入口（重複の整理の左の下。小さく畳んでおく）。
 * 記録は「別人（統合しない）」と、統合でチェックを外した人（docs/CONTACT_MODEL.md §1-5）。
 *
 * `reloadKey` が変わると数え直す。戻したら `onUndo` で重複の一覧を取り直させる。
 */
export function DistinctPairsPanel({
  reloadKey,
  onUndo,
}: {
  reloadKey: number;
  onUndo: () => void;
}) {
  const { t } = useTranslation();
  const [pairs, setPairs] = useState<DistinctPair[]>([]);
  const [open, setOpen] = useState(false);

  const load = () => {
    if (!isTauri) return;
    contactDistinctPairs()
      .then(setPairs)
      .catch(() => setPairs([]));
  };
  useEffect(load, [reloadKey]);

  // 対ではなく、利用者が判断した単位（つながった人の組）で出す。
  const groups = useMemo(() => groupDistinctPairs(pairs), [pairs]);

  const undo = async (g: DistinctGroup) => {
    await Promise.all(g.pairs.map((p) => contactUnmarkDistinct(p.a, p.b))).catch(() => undefined);
    load();
    onUndo();
  };

  if (groups.length === 0) return null;
  return (
    <div className="text-[11px] text-white/50">
      <button
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="flex items-center gap-1 hover:text-white/80"
      >
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <UserX size={12} />
        {t('dupes.distinctPairs', { count: groups.length })}
      </button>
      {open && (
        <div className="mt-1 space-y-0.5">
          <p className="text-white/35">{t('dupes.distinctHint')}</p>
          <ul className="max-h-40 overflow-y-auto">
            {groups.map((g) => (
              <li
                key={g.members.map((m) => m.id).join('-')}
                className="flex items-center gap-2 rounded px-1 py-0.5 hover:bg-white/5"
              >
                <span className="min-w-0 flex-1 truncate text-white/70">
                  {t('dupes.distinctMembers', {
                    names: [...new Set(g.members.map((m) => m.name))].join(' / '),
                    count: g.members.length,
                  })}
                </span>
                <button
                  onClick={() => void undo(g)}
                  className="shrink-0 rounded px-1.5 py-0.5 text-sky-300 hover:bg-white/10"
                >
                  {t('dupes.distinctUndo')}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
