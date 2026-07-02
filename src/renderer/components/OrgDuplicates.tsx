import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowLeft, Building2, Check, Merge, RefreshCw, Users } from 'lucide-react';
import type { OrgDuplicateGroup } from '@bindings/OrgDuplicateGroup';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import { organizationFindDuplicates, organizationMerge } from '../services/organizations';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 重複整理の「連絡先／組織」切替（両画面で共用）。 */
export function DupModeToggle({
  mode,
  onChange,
}: {
  mode: 'contacts' | 'orgs';
  onChange: (m: 'contacts' | 'orgs') => void;
}) {
  const { t } = useTranslation();
  const btn = (m: 'contacts' | 'orgs', label: string) => (
    <button
      onClick={() => onChange(m)}
      className={`rounded-full px-2.5 py-1 text-xs ${
        mode === m ? 'bg-white/25 text-white' : 'text-white/55 hover:bg-white/10'
      }`}
    >
      {label}
    </button>
  );
  return (
    <div className="flex items-center gap-0.5 rounded-full bg-white/5 p-0.5">
      {btn('contacts', t('dupes.modeContacts'))}
      {btn('orgs', t('dupes.modeOrgs'))}
    </div>
  );
}

/**
 * 組織名の統一（2ペイン）。左＝重複グループ一覧、右＝統一名を決めて［統一する］。
 * 「株式会社◯◯」と「(株)◯◯」などを 1 つの組織にまとめ、所属連絡先を付け替える。
 */
export function OrgDuplicates({
  mode,
  onModeChange,
  onMerged,
  onExit,
}: {
  mode: 'contacts' | 'orgs';
  onModeChange: (m: 'contacts' | 'orgs') => void;
  onMerged: () => void;
  onExit: () => void;
}) {
  const { t } = useTranslation();
  const [groups, setGroups] = useState<OrgDuplicateGroup[]>([]);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState(0);
  const [included, setIncluded] = useState<Set<number>>(new Set());
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);

  const load = () => {
    if (!isTauri) return;
    setLoading(true);
    organizationFindDuplicates()
      .then((g) => {
        setGroups(g);
        setSelected(0);
      })
      .catch(() => undefined)
      .finally(() => setLoading(false));
  };
  useEffect(load, []);

  const group: OrgDuplicateGroup | undefined = groups[selected];

  // グループを選び直したら全員を含め、統一名を既定（canonical）に戻す。
  useEffect(() => {
    if (group) {
      setIncluded(new Set(group.organizations.map((o) => o.id)));
      setName(group.canonical);
    } else {
      setIncluded(new Set());
      setName('');
    }
  }, [group]);

  const includedOrgs = useMemo(
    () => (group ? group.organizations.filter((o) => included.has(o.id)) : []),
    [group, included],
  );
  // 残す（keep）＝含める中で最多所属。
  const keep = useMemo(
    () =>
      includedOrgs.reduce<OrganizationSummary | null>(
        (best, o) => (!best || o.member_count > best.member_count ? o : best),
        null,
      ),
    [includedOrgs],
  );
  const totalMembers = includedOrgs.reduce((n, o) => n + o.member_count, 0);

  const toggle = (id: number) =>
    setIncluded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const dropCurrent = () => {
    setGroups((prev) => prev.filter((_, i) => i !== selected));
    setSelected((i) => Math.max(0, Math.min(i, groups.length - 2)));
  };

  const doMerge = async () => {
    if (!keep || includedOrgs.length < 2 || name.trim() === '' || busy) return;
    const dropIds = includedOrgs.map((o) => o.id).filter((id) => id !== keep.id);
    setBusy(true);
    try {
      await organizationMerge(keep.id, dropIds, name.trim());
      dropCurrent();
      onMerged();
    } catch {
      /* noop */
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full min-h-0">
      {/* 左：グループ一覧 */}
      <aside className="flex w-72 shrink-0 flex-col border-r border-white/10">
        <div className="flex items-center gap-2 p-3">
          <button
            onClick={onExit}
            title={t('dupes.back')}
            aria-label={t('dupes.back')}
            className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white"
          >
            <ArrowLeft size={17} />
          </button>
          <DupModeToggle mode={mode} onChange={onModeChange} />
          <span className="flex-1" />
          <button
            onClick={load}
            disabled={loading}
            title={t('dupes.rescan')}
            aria-label={t('dupes.rescan')}
            className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 disabled:opacity-40"
          >
            <RefreshCw size={16} className={loading ? 'animate-spin' : ''} />
          </button>
        </div>
        <div className="px-3 pb-1 text-xs text-white/45">
          {loading
            ? t('dupes.scanning')
            : groups.length === 0
              ? t('dupes.orgNone')
              : t('dupes.orgSummary', { count: groups.length })}
        </div>

        <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {groups.map((g, i) => (
            <li key={g.organizations[0].id}>
              <button
                onClick={() => setSelected(i)}
                className={`flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left ${
                  i === selected ? 'bg-white/20' : 'hover:bg-white/10'
                }`}
              >
                <Building2 size={15} className="shrink-0 text-white/45" />
                <span className="min-w-0 flex-1 truncate text-sm">{g.canonical}</span>
                <span className="shrink-0 text-xs text-white/40">
                  {t('dupes.orgCount', { count: g.organizations.length })}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </aside>

      {/* 右：統一名を決めて統合 */}
      <section className="min-h-0 flex-1 overflow-y-auto">
        {!group ? (
          <div className="flex h-full flex-col items-center justify-center gap-1 text-center">
            <Building2 size={40} className="text-white/25" />
            <p className="text-sm text-white/45">
              {groups.length === 0 ? t('dupes.orgNone') : t('dupes.pickGroup')}
            </p>
          </div>
        ) : (
          <div className="mx-auto max-w-2xl p-6">
            <h2 className="mb-1 text-lg font-semibold">{t('dupes.orgTitle')}</h2>
            <p className="mb-4 text-xs text-white/45">{t('dupes.orgPickMembers')}</p>

            {/* 変種の一覧（含める組織を選ぶ） */}
            <ul className="space-y-2">
              {group.organizations.map((o) => {
                const inc = included.has(o.id);
                const isKeep = keep?.id === o.id;
                return (
                  <li
                    key={o.id}
                    className={`flex items-center gap-2.5 rounded-lg border px-3 py-2.5 ${
                      inc
                        ? 'border-white/15 bg-white/5'
                        : 'border-white/10 bg-transparent opacity-45'
                    }`}
                  >
                    <button
                      onClick={() => toggle(o.id)}
                      className="flex min-w-0 flex-1 items-center gap-2.5 text-left"
                    >
                      <span
                        className={`flex h-5 w-5 shrink-0 items-center justify-center rounded ${
                          inc ? 'bg-sky-500 text-white' : 'border border-white/30'
                        }`}
                      >
                        {inc && <Check size={13} />}
                      </span>
                      <span className="min-w-0 flex-1 truncate text-sm font-medium">{o.name}</span>
                    </button>
                    <span className="flex shrink-0 items-center gap-1 text-xs text-white/45">
                      <Users size={12} />
                      {t('dupes.orgMembers', { count: o.member_count })}
                    </span>
                    {inc && isKeep && includedOrgs.length > 1 && (
                      <span className="shrink-0 rounded bg-emerald-400/20 px-1.5 py-0.5 text-[10px] font-semibold text-emerald-200">
                        {t('dupes.orgKeep')}
                      </span>
                    )}
                  </li>
                );
              })}
            </ul>

            {/* 統一名 */}
            <label className="mt-4 block">
              <span className="mb-1 block text-xs text-white/50">{t('dupes.orgUnifyName')}</span>
              <input
                className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </label>

            <div className="mt-4 flex flex-wrap items-center gap-3">
              <button
                onClick={doMerge}
                disabled={busy || includedOrgs.length < 2 || name.trim() === ''}
                className="flex items-center gap-1.5 rounded-md bg-emerald-500/80 px-4 py-2 text-sm font-medium text-white hover:bg-emerald-500 disabled:cursor-not-allowed disabled:opacity-40"
              >
                <Merge size={15} />
                {t('dupes.orgUnify')}
              </button>
              <button
                onClick={dropCurrent}
                className="rounded-md border border-white/20 px-3 py-2 text-sm text-white/70 hover:bg-white/10"
              >
                {t('dupes.orgNotDup')}
              </button>
              {includedOrgs.length >= 2 && keep && (
                <span className="text-xs text-white/45">
                  {t('dupes.orgUnifyHint', { count: totalMembers })}
                </span>
              )}
            </div>
          </div>
        )}
      </section>
    </div>
  );
}
