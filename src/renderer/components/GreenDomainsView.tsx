import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, Building2, LeafyGreen, Plus, RotateCcw, UserRound } from 'lucide-react';
import type { GreenDomainEntry } from '@bindings/GreenDomainEntry';
import {
  greenDomainAdd,
  greenDomainClear,
  greenDomainList,
  greenDomainWarn,
} from '../services/green';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 住所録の「グリーンドメイン」タブ。ユーザーが認めた安全な差出人ドメインの管理。
 * グリーン（手動認定＋住所録由来を区別）／警告（意図的な除外）を一覧・追加・切替する。
 * docs/GREEN_DOMAINS.md。
 */
export function GreenDomainsView() {
  const { t } = useTranslation();
  const [items, setItems] = useState<GreenDomainEntry[]>([]);
  const [input, setInput] = useState('');
  // 追加できなかった理由（フリーメールはドメイン単位で認定不可）。
  const [note, setNote] = useState('');

  const load = useCallback(() => {
    if (!isTauri) return;
    greenDomainList()
      .then(setItems)
      .catch(() => undefined);
  }, []);
  useEffect(load, [load]);

  const green = items.filter((e) => e.kind === 'green');
  const warning = items.filter((e) => e.kind === 'warning');
  // 追加フィールドは検索も兼ねる: 入力に一致するドメインだけ表示（部分一致）。
  const q = input.trim().toLowerCase();
  const greenShown = q ? green.filter((e) => e.domain.includes(q)) : green;
  const warningShown = q ? warning.filter((e) => e.domain.includes(q)) : warning;
  // 入力ドメインが既にグリーンにあるか（あれば「追加」は無効化）。
  const normalized = input.trim().toLowerCase().replace(/^.*@/, '').replace(/\/.*$/, '');
  const alreadyGreen = green.some((e) => e.domain === normalized);

  // メールを貼っても OK（@ より後ろをドメインとして拾う）。
  const normalize = (s: string) => s.trim().toLowerCase().replace(/^.*@/, '').replace(/\/.*$/, '');

  const addManual = async () => {
    const d = normalize(input);
    if (!d || !isTauri) return;
    setNote('');
    try {
      if (!(await greenDomainAdd(d))) {
        setNote(t('green.freemail', { domain: d }));
        return;
      }
      setInput('');
      load();
    } catch {
      /* noop */
    }
  };
  const run = (p: Promise<unknown>) => p.then(load).catch(() => undefined);

  return (
    <section className="flex h-full min-h-0 flex-col">
      {/* 固定ヘッダ（タイトル・説明・ドメイン追加） */}
      <div className="mx-auto w-full max-w-2xl shrink-0 px-6 pt-5">
        <div className="mb-1 flex items-center gap-2">
          <LeafyGreen size={20} className="text-emerald-400" />
          <h2 className="text-lg font-semibold">{t('green.title')}</h2>
        </div>
        <p className="mb-4 text-xs text-white/45">{t('green.hint')}</p>

        {/* ドメイン追加 */}
        <div className="mb-6 flex gap-2">
          <input
            className="min-w-0 flex-1 rounded-md bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
            placeholder={t('green.addPlaceholder')}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                void addManual();
              }
            }}
          />
          <button
            onClick={addManual}
            disabled={normalized === '' || alreadyGreen}
            title={alreadyGreen ? t('green.alreadyGreen') : t('green.add')}
            className="flex shrink-0 items-center gap-1.5 rounded-md bg-emerald-500/80 px-3.5 py-1.5 text-sm font-medium text-white hover:bg-emerald-500 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Plus size={15} />
            {t('green.add')}
          </button>
        </div>
        {note && (
          <p className="-mt-4 mb-4 rounded-md bg-amber-500/10 px-3 py-2 text-xs text-amber-100">
            {note}
          </p>
        )}
      </div>

      {/* スクロールするリスト領域（ドメイン一覧だけスクロール） */}
      <div className="mx-auto min-h-0 w-full max-w-2xl flex-1 overflow-y-auto px-6 pb-6">
        {/* グリーン */}
        <div className="mb-2 flex items-center gap-1.5 text-xs font-medium text-emerald-300">
          <LeafyGreen size={14} />
          {t('green.greenSection', { count: greenShown.length })}
        </div>
        {green.length === 0 ? (
          <p className="mb-6 rounded-md bg-white/5 px-3 py-2 text-sm text-white/40">
            {t('green.greenEmpty')}
          </p>
        ) : greenShown.length === 0 ? (
          <p className="mb-6 rounded-md bg-white/5 px-3 py-2 text-sm text-white/40">
            {t('green.noMatch')}
          </p>
        ) : (
          <ul className="mb-6 space-y-1.5">
            {greenShown.map((e) => (
              <li
                key={`g-${e.domain}`}
                className="flex items-center gap-2.5 rounded-md border border-emerald-400/20 bg-emerald-400/5 px-3 py-2"
              >
                <LeafyGreen size={15} className="shrink-0 text-emerald-400" />
                <span className="min-w-0 flex-1 truncate text-sm">{e.domain}</span>
                {e.auto ? (
                  <span className="flex shrink-0 items-center gap-1 rounded-full bg-white/10 px-2 py-0.5 text-[10px] text-white/60">
                    <UserRound size={11} />
                    {t('green.fromContacts', { count: e.contact_count })}
                  </span>
                ) : (
                  <span className="shrink-0 rounded-full bg-white/10 px-2 py-0.5 text-[10px] text-white/60">
                    {t('green.manual')}
                  </span>
                )}
                <button
                  onClick={() => run(greenDomainWarn(e.domain))}
                  title={t('green.exclude')}
                  className="flex shrink-0 items-center gap-1 rounded-md border border-white/20 px-2 py-1 text-xs text-white/70 hover:border-amber-400/60 hover:bg-amber-500/20 hover:text-amber-100"
                >
                  <AlertTriangle size={12} />
                  {t('green.exclude')}
                </button>
              </li>
            ))}
          </ul>
        )}

        {/* 警告（除外） */}
        {warningShown.length > 0 && (
          <>
            <div className="mb-2 flex items-center gap-1.5 text-xs font-medium text-amber-300">
              <AlertTriangle size={14} />
              {t('green.warnSection', { count: warningShown.length })}
            </div>
            <p className="mb-2 text-[11px] text-white/40">{t('green.warnHint')}</p>
            <ul className="space-y-1.5">
              {warningShown.map((e) => (
                <li
                  key={`w-${e.domain}`}
                  className="flex items-center gap-2.5 rounded-md border border-amber-300/20 bg-amber-300/5 px-3 py-2"
                >
                  <AlertTriangle size={15} className="shrink-0 text-amber-300" />
                  <span className="min-w-0 flex-1 truncate text-sm text-amber-100/90">
                    {e.domain}
                  </span>
                  {e.auto && (
                    <span className="flex shrink-0 items-center gap-1 rounded-full bg-white/10 px-2 py-0.5 text-[10px] text-white/50">
                      <Building2 size={11} />
                      {t('green.fromContacts', { count: e.contact_count })}
                    </span>
                  )}
                  <button
                    onClick={() => run(greenDomainAdd(e.domain))}
                    title={t('green.makeGreen')}
                    className="flex shrink-0 items-center gap-1 rounded-md border border-white/20 px-2 py-1 text-xs text-white/70 hover:border-emerald-400/60 hover:bg-emerald-500/20 hover:text-emerald-100"
                  >
                    <LeafyGreen size={12} />
                    {t('green.makeGreen')}
                  </button>
                  <button
                    onClick={() => run(greenDomainClear(e.domain))}
                    title={t('green.restore')}
                    aria-label={t('green.restore')}
                    className="flex shrink-0 items-center justify-center rounded-md border border-white/20 px-2 py-1 text-white/60 hover:bg-white/10"
                  >
                    <RotateCcw size={13} />
                  </button>
                </li>
              ))}
            </ul>
          </>
        )}
      </div>
    </section>
  );
}
