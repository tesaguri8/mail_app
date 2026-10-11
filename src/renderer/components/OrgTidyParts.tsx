// 組織タブの「整理」の画面で共有する部品（切替・左ペインの頭・確認欄・空の表示）。
// 整理の各画面は OrgTidy.tsx と OrgTidyCompanyOnly.tsx。docs/ORG_CONTACTS.md §4-3。

import { useTranslation } from 'react-i18next';
import { ArrowLeft, Building2, RefreshCw, Search, X } from 'lucide-react';

/** 左の一覧に一度に出す件数（同期で何千件も入るため。超えたら検索で絞ってもらう）。 */
export const MAX_ROWS = 300;

export type PaneProps = {
  mode: OrgTidyMode;
  onModeChange: (m: OrgTidyMode) => void;
  onChanged: () => void;
  onExit: () => void;
  onOpenOrg: (id: number) => void;
};

export type OrgTidyMode = 'unlinked' | 'suggest' | 'dupes' | 'companyOnly';

/** 整理の種類の切替（組織にする／つなぐ／会社名だけ／重複）。 */
export function OrgTidyToggle({
  mode,
  onChange,
}: {
  mode: OrgTidyMode;
  onChange: (m: OrgTidyMode) => void;
}) {
  const { t } = useTranslation();
  const modes: OrgTidyMode[] = ['unlinked', 'suggest', 'companyOnly', 'dupes'];
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
export function TidyHeader({
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
export function ConfirmBox({
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

export function EmptyPane({ text }: { text: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-1 text-center">
      <Building2 size={40} className="text-white/25" />
      <p className="text-sm text-white/45">{text}</p>
    </div>
  );
}

/** 検索語に合うか（小文字の部分一致）。 */
export const hit = (q: string, ...texts: (string | null | undefined)[]) => {
  const n = q.trim().toLowerCase();
  return n === '' || texts.some((s) => (s ?? '').toLowerCase().includes(n));
};
