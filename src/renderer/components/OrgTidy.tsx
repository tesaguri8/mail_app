import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowLeft,
  AtSign,
  Building2,
  Check,
  ChevronDown,
  ChevronRight,
  Link2,
  Plus,
  RefreshCw,
  Search,
  User,
  Users,
  X,
} from 'lucide-react';
import type { UnlinkedOrgName } from '@bindings/UnlinkedOrgName';
import type { OrgLinkSuggestion } from '@bindings/OrgLinkSuggestion';
import type { OrgLinkCandidate } from '@bindings/OrgLinkCandidate';
import {
  organizationCreateFromName,
  organizationCreateFromNameImpact,
  organizationLinkContacts,
  organizationLinkContactsImpact,
  organizationLinkSuggestions,
  organizationUnlinkedNames,
} from '../services/organizations';
import { OrgDuplicates } from './OrgDuplicates';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
/** 左の一覧に一度に出す件数（同期で何千件も入るため。超えたら検索で絞ってもらう）。 */
const MAX_ROWS = 300;

export type OrgTidyMode = 'unlinked' | 'suggest' | 'dupes';

/** 整理の種類の切替（カードにする／つなぐ／重複）。 */
function OrgTidyToggle({
  mode,
  onChange,
}: {
  mode: OrgTidyMode;
  onChange: (m: OrgTidyMode) => void;
}) {
  const { t } = useTranslation();
  const modes: OrgTidyMode[] = ['unlinked', 'suggest', 'dupes'];
  return (
    <div className="flex min-w-0 items-center gap-0.5 rounded-full bg-white/5 p-0.5">
      {modes.map((m) => (
        <button
          key={m}
          onClick={() => onChange(m)}
          className={`whitespace-nowrap rounded-full px-2 py-1 text-[11px] ${
            mode === m ? 'bg-white/25 text-white' : 'text-white/55 hover:bg-white/10'
          }`}
        >
          {t(`orgTidy.mode.${m}`)}
        </button>
      ))}
    </div>
  );
}

/** 左ペインの頭（戻る・切替・取り直し・検索）。 */
function TidyHeader({
  mode,
  onModeChange,
  onExit,
  onReload,
  loading,
  query,
  onQuery,
  summary,
}: {
  mode: OrgTidyMode;
  onModeChange: (m: OrgTidyMode) => void;
  onExit: () => void;
  onReload: () => void;
  loading: boolean;
  query: string;
  onQuery: (q: string) => void;
  summary: string;
}) {
  const { t } = useTranslation();
  return (
    <>
      <div className="flex items-center gap-2 p-3">
        <button
          onClick={onExit}
          title={t('dupes.back')}
          aria-label={t('dupes.back')}
          className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white"
        >
          <ArrowLeft size={17} />
        </button>
        <OrgTidyToggle mode={mode} onChange={onModeChange} />
        <span className="flex-1" />
        <button
          onClick={onReload}
          disabled={loading}
          title={t('dupes.rescan')}
          aria-label={t('dupes.rescan')}
          className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 disabled:opacity-40"
        >
          <RefreshCw size={16} className={loading ? 'animate-spin' : ''} />
        </button>
      </div>
      <div className="mx-3 mb-2 flex items-center gap-2 rounded-md bg-white/10 px-2.5 py-1.5">
        <Search size={15} className="shrink-0 text-white/50" />
        <input
          className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-white/40"
          placeholder={t('orgTidy.search')}
          value={query}
          onChange={(e) => onQuery(e.target.value)}
        />
        {query && (
          <button
            onClick={() => onQuery('')}
            title={t('contact.clearSearch')}
            aria-label={t('contact.clearSearch')}
            className="flex h-4 w-4 shrink-0 items-center justify-center rounded-full text-white/40 hover:bg-white/20 hover:text-white"
          >
            <X size={12} />
          </button>
        )}
      </div>
      <div className="px-3 pb-1 text-xs text-white/45">
        {loading ? t('dupes.scanning') : summary}
      </div>
    </>
  );
}

/**
 * 画面内の確認欄（カードにする・つなぐ）。会社名がそろう・会社が足されるために次の同期で
 * 送り直しになる人がいれば、その人数を添える（下見で数える。数え終わるまで実行できない）。
 */
function ConfirmBox({
  text,
  resent,
  runLabel,
  busy,
  onRun,
  onCancel,
}: {
  text: string;
  /** 送り直しになる人数（null＝数えている途中）。 */
  resent: number | null;
  runLabel: string;
  busy: boolean;
  onRun: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="mt-4 space-y-2 rounded-lg border border-white/20 bg-white/5 p-3">
      <p className="text-sm text-white/80">{text}</p>
      {resent === null ? (
        <p className="text-xs text-white/45">{t('orgTidy.impactCounting')}</p>
      ) : (
        resent > 0 && (
          <p className="rounded-md bg-amber-300/10 px-2.5 py-1.5 text-xs text-amber-100">
            {t('orgTidy.impactResent', { count: resent })}
          </p>
        )
      )}
      <div className="flex items-center gap-2">
        <button
          onClick={onRun}
          disabled={busy || resent === null}
          className="rounded-md bg-emerald-500/80 px-3 py-1.5 text-xs font-medium hover:bg-emerald-500 disabled:opacity-40"
        >
          {runLabel}
        </button>
        <button
          onClick={onCancel}
          disabled={busy}
          className="rounded-md border border-white/20 px-3 py-1.5 text-xs text-white/70 hover:bg-white/10 disabled:opacity-40"
        >
          {t('org.cancel')}
        </button>
      </div>
    </div>
  );
}

function EmptyPane({ text }: { text: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-1 text-center">
      <Building2 size={40} className="text-white/25" />
      <p className="text-sm text-white/45">{text}</p>
    </div>
  );
}

/** 検索語に合うか（小文字の部分一致）。 */
const hit = (q: string, ...texts: (string | null | undefined)[]) => {
  const n = q.trim().toLowerCase();
  return n === '' || texts.some((s) => (s ?? '').toLowerCase().includes(n));
};

/**
 * 組織タブの「整理」（docs/CONTACT_MODEL.md §1-5-1）。どれも候補を出すだけで、作る・つなぐは
 * 人が選ぶ。
 * - カードにする: 組織カードになっていない会社名を人数の多い順に出し、選んでカードにする
 * - つなぐ: 組織カードごとに、つながっていない人の候補を理由（会社名／メールのドメイン）つきで出す
 * - 重複: 組織名の統一（住所録の重複整理と同じ画面）
 */
export function OrgTidy({
  onChanged,
  onExit,
  onOpenOrg,
}: {
  /** カードを作った・つないだ・統一した（組織の一覧を取り直す）。 */
  onChanged: () => void;
  onExit: () => void;
  /** 作った・つないだ組織カードを開く。 */
  onOpenOrg: (id: number) => void;
}) {
  const [mode, setMode] = useState<OrgTidyMode>('unlinked');
  if (mode === 'dupes') {
    return (
      <OrgDuplicates
        modeToggle={<OrgTidyToggle mode={mode} onChange={setMode} />}
        onMerged={onChanged}
        onExit={onExit}
      />
    );
  }
  return mode === 'unlinked' ? (
    <UnlinkedNames
      mode={mode}
      onModeChange={setMode}
      onChanged={onChanged}
      onExit={onExit}
      onOpenOrg={onOpenOrg}
    />
  ) : (
    <LinkSuggestions
      mode={mode}
      onModeChange={setMode}
      onChanged={onChanged}
      onExit={onExit}
      onOpenOrg={onOpenOrg}
    />
  );
}

type PaneProps = {
  mode: OrgTidyMode;
  onModeChange: (m: OrgTidyMode) => void;
  onChanged: () => void;
  onExit: () => void;
  onOpenOrg: (id: number) => void;
};

/** 1. 組織カードになっていない会社名 → 選んで「組織カードにする」。 */
function UnlinkedNames({ mode, onModeChange, onChanged, onExit, onOpenOrg }: PaneProps) {
  const { t } = useTranslation();
  const [items, setItems] = useState<UnlinkedOrgName[]>([]);
  const [loading, setLoading] = useState(false);
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<string | null>(null);
  // カードにするときの名前（既定は最も多い表記）と、画面内の確認の表示。
  const [name, setName] = useState('');
  const [confirming, setConfirming] = useState(false);
  // 確認欄の下見（送り直しになる人数。null＝数えている途中）。
  const [resent, setResent] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<{ id: number; name: string; count: number } | null>(null);

  const askCreate = () => {
    setConfirming(true);
    setResent(null);
    organizationCreateFromNameImpact(name.trim())
      .then((r) => setResent(r.resent))
      .catch((e) => {
        setConfirming(false);
        setError(String(e));
      });
  };

  const load = () => {
    if (!isTauri) return;
    setLoading(true);
    organizationUnlinkedNames()
      .then(setItems)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  };
  useEffect(load, []);

  const filtered = useMemo(
    () => items.filter((u) => hit(query, u.name, ...u.variants)),
    [items, query]
  );
  const item = items.find((u) => u.name === selected) ?? null;

  const pick = (u: UnlinkedOrgName) => {
    setSelected(u.name);
    setName(u.name);
    setConfirming(false);
    setError(null);
    setDone(null);
  };

  const create = async () => {
    if (!item || name.trim() === '' || busy) return;
    setBusy(true);
    setError(null);
    try {
      const org = await organizationCreateFromName(name.trim());
      setItems((prev) => prev.filter((u) => u.name !== item.name));
      setSelected(null);
      setConfirming(false);
      setDone({ id: org.id, name: org.name, count: org.member_count });
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full min-h-0">
      <aside className="flex w-80 shrink-0 flex-col border-r border-white/10">
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
              ? t('orgTidy.unlinkedNone')
              : t('orgTidy.unlinkedSummary', { count: items.length })
          }
        />
        <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {filtered.slice(0, MAX_ROWS).map((u) => (
            <li key={u.name}>
              <button
                onClick={() => pick(u)}
                className={`flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left ${
                  u.name === selected ? 'bg-white/20' : 'hover:bg-white/10'
                }`}
              >
                <Building2 size={15} className="shrink-0 text-white/35" />
                <span className="min-w-0 flex-1 truncate text-sm">{u.name}</span>
                <span className="flex shrink-0 items-center gap-1 text-xs text-white/45">
                  <Users size={12} />
                  {u.contact_count}
                </span>
              </button>
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
        {!item ? (
          done ? (
            <div className="mx-auto max-w-xl p-6">
              <p className="mb-3 flex items-center gap-1.5 text-sm text-emerald-300">
                <Check size={15} />
                {t('orgTidy.created', { name: done.name, count: done.count })}
              </p>
              <button
                onClick={() => onOpenOrg(done.id)}
                className="rounded-md border border-white/20 px-3 py-1.5 text-sm text-white/80 hover:bg-white/10"
              >
                {t('orgTidy.openOrg')}
              </button>
            </div>
          ) : (
            <EmptyPane
              text={items.length === 0 ? t('orgTidy.unlinkedNone') : t('orgTidy.pickName')}
            />
          )
        ) : (
          <div className="mx-auto max-w-xl p-6">
            <h2 className="mb-1 text-lg font-semibold">{t('orgTidy.unlinkedTitle')}</h2>
            <p className="mb-4 text-xs text-white/45">{t('orgTidy.unlinkedHint')}</p>

            <span className="mb-1.5 block text-xs text-white/50">{t('orgTidy.variants')}</span>
            <ul className="mb-4 space-y-1">
              {item.variants.map((v) => (
                <li key={v} className="rounded-md bg-white/5 px-3 py-1.5 text-sm">
                  {v}
                </li>
              ))}
            </ul>

            <label className="block">
              <span className="mb-1 block text-xs text-white/50">{t('orgTidy.cardName')}</span>
              <input
                className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                value={name}
                list="org-tidy-variants"
                onChange={(e) => {
                  setName(e.target.value);
                  setConfirming(false);
                }}
              />
              <datalist id="org-tidy-variants">
                {item.variants.map((v) => (
                  <option key={v} value={v} />
                ))}
              </datalist>
            </label>

            {confirming ? (
              <ConfirmBox
                text={t('orgTidy.createConfirm', { name: name.trim(), count: item.contact_count })}
                resent={resent}
                runLabel={t('orgTidy.createRun')}
                busy={busy}
                onRun={create}
                onCancel={() => setConfirming(false)}
              />
            ) : (
              <button
                onClick={askCreate}
                disabled={name.trim() === ''}
                className="mt-4 flex items-center gap-1.5 rounded-md bg-emerald-500/80 px-4 py-2 text-sm font-medium text-white hover:bg-emerald-500 disabled:cursor-not-allowed disabled:opacity-40"
              >
                <Plus size={15} />
                {t('orgTidy.createCard', { count: item.contact_count })}
              </button>
            )}
            {error && <p className="mt-3 text-sm text-red-300">{error}</p>}
          </div>
        )}
      </section>
    </div>
  );
}

/** 2. 組織カードがあるのにつながっていない人 → 選んで「つなぐ」。 */
function LinkSuggestions({ mode, onModeChange, onChanged, onExit, onOpenOrg }: PaneProps) {
  const { t } = useTranslation();
  const [items, setItems] = useState<OrgLinkSuggestion[]>([]);
  const [loading, setLoading] = useState(false);
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<number | null>(null);
  // つなぐ人（既定は誰も選ばない。人が選ぶ）。
  const [picked, setPicked] = useState<Set<number>>(new Set());
  // ドメインだけで一致した候補（会社名は違う）は雑音が多いので、最初は畳んでおく。
  const [showDomainOnly, setShowDomainOnly] = useState(false);
  // 確認欄（null＝出していない）と、その下見（送り直しになる人数。null＝数えている途中）。
  const [confirming, setConfirming] = useState(false);
  const [resent, setResent] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);

  const load = () => {
    if (!isTauri) return;
    setLoading(true);
    organizationLinkSuggestions()
      .then(setItems)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  };
  useEffect(load, []);

  const filtered = useMemo(
    () =>
      items.filter((s) =>
        hit(query, s.org.name, ...s.candidates.map((c) => c.contact.display_name))
      ),
    [items, query]
  );
  const item = items.find((s) => s.org.id === selected) ?? null;
  const total = items.reduce((n, s) => n + s.candidates.length, 0);

  const byName = item ? item.candidates.filter((c) => c.matched_name !== null) : [];
  const domainOnly = item ? item.candidates.filter((c) => c.matched_name === null) : [];
  // 「すべて選ぶ」は見えている候補だけ（畳んだドメインだけの一致は含めない）。
  const visible = showDomainOnly ? [...byName, ...domainOnly] : byName;

  const pick = (id: number) => {
    setSelected(id);
    setPicked(new Set());
    setShowDomainOnly(false);
    setConfirming(false);
    setError(null);
    setDone(null);
  };
  const toggle = (id: number) => {
    setConfirming(false);
    setPicked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const askLink = () => {
    if (!item || picked.size === 0) return;
    setConfirming(true);
    setResent(null);
    organizationLinkContactsImpact(item.org.id, [...picked])
      .then((r) => setResent(r.resent))
      .catch((e) => {
        setConfirming(false);
        setError(String(e));
      });
  };

  const link = async () => {
    if (!item || picked.size === 0 || busy) return;
    setBusy(true);
    setError(null);
    try {
      const org = await organizationLinkContacts(item.org.id, [...picked]);
      // つないだ人を候補から外し、候補が尽きたカードは一覧から外す。
      setItems((prev) =>
        prev
          .map((s) =>
            s.org.id === item.org.id
              ? { ...s, candidates: s.candidates.filter((c) => !picked.has(c.contact.id)) }
              : s
          )
          .filter((s) => s.candidates.length > 0)
      );
      setDone(t('orgTidy.linked', { name: org.name, count: picked.size }));
      setPicked(new Set());
      setConfirming(false);
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full min-h-0">
      <aside className="flex w-80 shrink-0 flex-col border-r border-white/10">
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
              ? t('orgTidy.suggestNone')
              : t('orgTidy.suggestSummary', { orgs: items.length, count: total })
          }
        />
        <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {filtered.slice(0, MAX_ROWS).map((s) => (
            <li key={s.org.id}>
              <button
                onClick={() => pick(s.org.id)}
                className={`flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left ${
                  s.org.id === selected ? 'bg-white/20' : 'hover:bg-white/10'
                }`}
              >
                <Building2 size={15} className="shrink-0 text-white/45" />
                <span className="min-w-0 flex-1 truncate text-sm">{s.org.name}</span>
                <span className="shrink-0 text-xs text-white/45">
                  {t('orgTidy.candidates', { count: s.candidates.length })}
                </span>
              </button>
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
        {!item ? (
          <EmptyPane
            text={done ?? (items.length === 0 ? t('orgTidy.suggestNone') : t('orgTidy.pickOrg'))}
          />
        ) : (
          <div className="mx-auto max-w-2xl p-6">
            <div className="mb-1 flex items-center gap-2">
              <h2 className="min-w-0 flex-1 truncate text-lg font-semibold">{item.org.name}</h2>
              <button
                onClick={() => onOpenOrg(item.org.id)}
                className="shrink-0 rounded-md border border-white/20 px-2.5 py-1 text-xs text-white/70 hover:bg-white/10"
              >
                {t('orgTidy.openOrg')}
              </button>
            </div>
            <p className="mb-4 text-xs text-white/45">
              {t('orgTidy.suggestHint', { count: item.org.member_count })}
            </p>

            <div className="mb-2 flex items-center gap-3 text-xs">
              <button
                onClick={() => {
                  setConfirming(false);
                  setPicked(new Set(visible.map((c) => c.contact.id)));
                }}
                className="text-sky-300 hover:text-sky-200"
              >
                {t('orgTidy.selectAll')}
              </button>
              <button
                onClick={() => {
                  setConfirming(false);
                  setPicked(new Set());
                }}
                className="text-sky-300 hover:text-sky-200"
              >
                {t('orgTidy.selectNone')}
              </button>
            </div>
            {byName.length > 0 ? (
              <ul className="space-y-1.5">
                {byName.map((c) => (
                  <CandidateRow
                    key={c.contact.id}
                    c={c}
                    on={picked.has(c.contact.id)}
                    onToggle={() => toggle(c.contact.id)}
                  />
                ))}
              </ul>
            ) : (
              <p className="rounded-md bg-white/5 px-3 py-2 text-xs text-white/45">
                {t('orgTidy.noNameMatch')}
              </p>
            )}

            {domainOnly.length > 0 && (
              <div className="mt-3">
                <button
                  onClick={() => setShowDomainOnly((v) => !v)}
                  aria-expanded={showDomainOnly}
                  className="flex items-center gap-1 text-xs text-white/60 hover:text-white"
                >
                  {showDomainOnly ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                  {t('orgTidy.domainOnly', { count: domainOnly.length })}
                </button>
                {showDomainOnly && (
                  <ul className="mt-1.5 space-y-1.5">
                    {domainOnly.map((c) => (
                      <CandidateRow
                        key={c.contact.id}
                        c={c}
                        on={picked.has(c.contact.id)}
                        onToggle={() => toggle(c.contact.id)}
                      />
                    ))}
                  </ul>
                )}
              </div>
            )}

            {confirming && (
              <ConfirmBox
                text={t('orgTidy.linkConfirm', { name: item.org.name, count: picked.size })}
                resent={resent}
                runLabel={t('orgTidy.linkRun', { count: picked.size })}
                busy={busy}
                onRun={link}
                onCancel={() => setConfirming(false)}
              />
            )}
            <div className="mt-4 flex flex-wrap items-center gap-3">
              <button
                onClick={askLink}
                disabled={busy || picked.size === 0 || confirming}
                className="flex items-center gap-1.5 rounded-md bg-emerald-500/80 px-4 py-2 text-sm font-medium text-white hover:bg-emerald-500 disabled:cursor-not-allowed disabled:opacity-40"
              >
                <Link2 size={15} />
                {t('orgTidy.linkRun', { count: picked.size })}
              </button>
              {done && <span className="text-sm text-emerald-300">{done}</span>}
            </div>
            {error && <p className="mt-3 text-sm text-red-300">{error}</p>}
          </div>
        )}
      </section>
    </div>
  );
}

/** 「つなぐ」の候補 1 人（選択・名前・会社/メール・一致の理由）。 */
function CandidateRow({
  c,
  on,
  onToggle,
}: {
  c: OrgLinkCandidate;
  on: boolean;
  onToggle: () => void;
}) {
  const { t } = useTranslation();
  return (
    <li>
      <button
        onClick={onToggle}
        aria-pressed={on}
        className={`flex w-full items-center gap-2.5 rounded-lg border px-3 py-2 text-left ${
          on ? 'border-sky-400/40 bg-sky-500/10' : 'border-white/10 bg-white/5 hover:bg-white/10'
        }`}
      >
        <span
          className={`flex h-5 w-5 shrink-0 items-center justify-center rounded ${
            on ? 'bg-sky-500 text-white' : 'border border-white/30'
          }`}
        >
          {on && <Check size={13} />}
        </span>
        <User size={14} className="shrink-0 text-white/40" />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-medium">
            {c.contact.display_name || t('contact.untitled')}
          </span>
          {(c.contact.primary_organization || c.contact.primary_email) && (
            <span className="block truncate text-xs text-white/45">
              {[c.contact.primary_organization, c.contact.primary_email]
                .filter(Boolean)
                .join(' · ')}
            </span>
          )}
        </span>
        <span className="flex shrink-0 flex-col items-end gap-0.5">
          {c.matched_name && (
            <span className="flex items-center gap-1 rounded bg-emerald-400/15 px-1.5 py-0.5 text-[10px] text-emerald-200">
              <Building2 size={10} />
              {t('orgTidy.reasonName', { name: c.matched_name })}
            </span>
          )}
          {c.matched_domain && (
            <span className="flex items-center gap-1 rounded bg-amber-400/15 px-1.5 py-0.5 text-[10px] text-amber-200">
              <AtSign size={10} />
              {t('orgTidy.reasonDomain', { domain: c.matched_domain })}
            </span>
          )}
        </span>
      </button>
    </li>
  );
}
