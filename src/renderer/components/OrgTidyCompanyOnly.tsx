import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, Building2, Check, Combine, Link2 } from 'lucide-react';
import type { CompanyOnlyCandidate } from '@bindings/CompanyOnlyCandidate';
import {
  organizationCompanyOnlyCandidates,
  organizationConvertContacts,
} from '../services/organizations';
import { ContactLinkMarks } from './ContactLinks';
import { EmptyPane, MAX_ROWS, TidyHeader, hit, type PaneProps } from './OrgTidyParts';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 整理 4. 会社名だけの連絡先 → まとめて「組織に切り替える」（docs/ORG_CONTACTS.md §4-1）。
 *
 * 切り替えは種別を変えるだけで、Google のつながり・uid・別人の記録・メールとの関係は残る。
 * 同じ名前の組織が既にあればその組織とまとめ、同じ会社名の人はその組織へつなぐ。
 * 役職・部署の入った候補は人の可能性があるので、最初はチェックを外しておく。
 */
export function CompanyOnly({ mode, onModeChange, onChanged, onExit }: PaneProps) {
  const { t } = useTranslation();
  const [items, setItems] = useState<CompanyOnlyCandidate[]>([]);
  const [loading, setLoading] = useState(false);
  const [query, setQuery] = useState('');
  const [checked, setChecked] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<number | null>(null);

  const load = () => {
    if (!isTauri) return;
    setLoading(true);
    organizationCompanyOnlyCandidates()
      .then((r) => {
        setItems(r);
        setChecked(new Set(r.filter((c) => !c.has_title).map((c) => c.contact.id)));
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  };
  useEffect(load, []);

  const filtered = useMemo(
    () => items.filter((c) => hit(query, c.company, c.contact.display_name)),
    [items, query]
  );
  const toggle = (id: number) =>
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const allShown = filtered.length > 0 && filtered.every((c) => checked.has(c.contact.id));
  const toggleAll = () =>
    setChecked((prev) => {
      const next = new Set(prev);
      for (const c of filtered) {
        if (allShown) next.delete(c.contact.id);
        else next.add(c.contact.id);
      }
      return next;
    });
  const picked = items.filter((c) => checked.has(c.contact.id));
  const merges = picked.filter((c) => c.existing_org !== null).length;
  const linked = picked.reduce((n, c) => n + c.same_company_people, 0);

  const convert = async () => {
    if (picked.length === 0 || busy) return;
    setBusy(true);
    setError(null);
    try {
      const n = await organizationConvertContacts(picked.map((c) => c.contact.id));
      setDone(n);
      onChanged();
      load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full min-h-0">
      <aside className="flex w-96 shrink-0 flex-col border-r border-white/10">
        <TidyHeader
          mode={mode}
          onModeChange={onModeChange}
          onExit={onExit}
          onReload={load}
          loading={loading}
          query={query}
          onQuery={setQuery}
          summary={
            items.length === 0
              ? t('orgTidy.companyOnlyNone')
              : t('orgTidy.companyOnlySummary', { count: items.length })
          }
        />
        {filtered.length > 0 && (
          <label className="mx-3 mb-1 flex items-center gap-2 px-2.5 text-xs text-white/60">
            <input
              type="checkbox"
              checked={allShown}
              onChange={toggleAll}
              className="h-3.5 w-3.5 accent-sky-400"
            />
            {t('orgTidy.companyOnlyAll')}
          </label>
        )}
        <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {filtered.slice(0, MAX_ROWS).map((c) => (
            <li key={c.contact.id}>
              <label className="flex w-full cursor-pointer items-center gap-2 rounded-md px-2.5 py-2 hover:bg-white/10">
                <input
                  type="checkbox"
                  checked={checked.has(c.contact.id)}
                  onChange={() => toggle(c.contact.id)}
                  className="h-3.5 w-3.5 shrink-0 accent-sky-400"
                />
                <Building2 size={15} className="shrink-0 text-white/35" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm">{c.contact.display_name}</span>
                  <span className="flex flex-wrap gap-x-2 text-[11px] text-white/45">
                    {c.has_title && (
                      <span className="flex items-center gap-0.5 text-amber-200/80">
                        <AlertTriangle size={11} />
                        {t('orgTidy.companyOnlyHasTitle')}
                      </span>
                    )}
                    {c.existing_org !== null && (
                      <span className="flex items-center gap-0.5 text-sky-200/80">
                        <Combine size={11} />
                        {t('orgTidy.companyOnlyMerge')}
                      </span>
                    )}
                    {c.same_company_people > 0 && (
                      <span className="flex items-center gap-0.5">
                        <Link2 size={11} />
                        {t('orgTidy.companyOnlyPeople', { count: c.same_company_people })}
                      </span>
                    )}
                  </span>
                </span>
                <ContactLinkMarks links={c.contact.links} />
              </label>
            </li>
          ))}
          {filtered.length > MAX_ROWS && (
            <li className="px-2.5 py-2 text-xs text-white/40">
              {t('orgTidy.more', { count: filtered.length - MAX_ROWS })}
            </li>
          )}
        </ul>
      </aside>

      <section className="min-h-0 flex-1 overflow-y-auto">
        {items.length === 0 && done === null ? (
          <EmptyPane text={t('orgTidy.companyOnlyNone')} />
        ) : (
          <div className="mx-auto max-w-xl p-6">
            <h2 className="mb-1 text-lg font-semibold">{t('orgTidy.companyOnlyTitle')}</h2>
            <p className="mb-4 text-xs leading-relaxed text-white/45">
              {t('orgTidy.companyOnlyHint')}
            </p>
            {done !== null && (
              <p className="mb-3 flex items-center gap-1.5 text-sm text-emerald-300">
                <Check size={15} />
                {t('orgTidy.companyOnlyDone', { count: done })}
              </p>
            )}
            <ul className="mb-4 space-y-1 text-sm text-white/70">
              <li>{t('orgTidy.companyOnlyPicked', { count: picked.length })}</li>
              {merges > 0 && <li>{t('orgTidy.companyOnlyMerges', { count: merges })}</li>}
              {linked > 0 && <li>{t('orgTidy.companyOnlyLinks', { count: linked })}</li>}
            </ul>
            <button
              onClick={() => void convert()}
              disabled={picked.length === 0 || busy}
              className="flex items-center gap-1.5 rounded-md bg-emerald-500/80 px-4 py-2 text-sm font-medium text-white hover:bg-emerald-500 disabled:cursor-not-allowed disabled:opacity-40"
            >
              <Building2 size={15} />
              {t('orgTidy.companyOnlyRun', { count: picked.length })}
            </button>
            {error && <p className="mt-3 text-sm text-red-300">{error}</p>}
          </div>
        )}
      </section>
    </div>
  );
}
