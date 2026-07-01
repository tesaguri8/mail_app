import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { FolderInput, HardDrive, RotateCcw } from 'lucide-react';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { DataLocation } from '@bindings/DataLocation';
import { APP } from '../config/appIdentity';
import { getInlineImages, setInlineImages } from '../config/prefs';
import { dataLocation, dataRelocate, dataResetLocation } from '../services/data';
import { AccountSetup } from './AccountSetup';
import { SignatureManager } from './SignatureManager';
import { TagManager } from './TagManager';

type Section = 'accounts' | 'signatures' | 'tags' | 'display' | 'data' | 'about';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** バイト数を読みやすい単位に整形。 */
function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

/**
 * 設定ページ: 左サイドバー（項目）＋右コンテンツの2カラム。
 */
export function Settings({
  accounts,
  onChanged,
}: {
  accounts: AccountSummary[];
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [section, setSection] = useState<Section>('accounts');

  const items: { key: Section; label: string }[] = [
    { key: 'accounts', label: t('settings.accounts') },
    { key: 'signatures', label: t('settings.signatures') },
    { key: 'tags', label: t('settings.tags') },
    { key: 'display', label: t('settings.display') },
    { key: 'data', label: t('settings.data') },
    { key: 'about', label: t('settings.about') },
  ];

  return (
    <div className="grid h-full min-h-0 grid-cols-[200px_1fr] overflow-hidden">
      <nav className="min-h-0 space-y-1 overflow-y-auto border-r border-white/10 p-2">
        <div className="px-2 py-1 text-sm font-semibold text-white/80">{t('settings.title')}</div>
        {items.map((it) => (
          <button
            key={it.key}
            onClick={() => setSection(it.key)}
            className={`block w-full rounded-md px-3 py-2 text-left text-sm hover:bg-white/10 ${
              section === it.key ? 'bg-white/15 text-white' : 'text-white/70'
            }`}
          >
            {it.label}
          </button>
        ))}
      </nav>

      <div className="min-h-0 overflow-y-auto p-5">
        {section === 'accounts' && <AccountSetup accounts={accounts} onChanged={onChanged} />}
        {section === 'signatures' && <SignatureManager />}
        {section === 'tags' && <TagManager />}
        {section === 'display' && <DisplaySettings />}
        {section === 'data' && <DataLocationSettings />}
        {section === 'about' && (
          <div className="space-y-1 text-sm text-white/70">
            <div className="text-base font-semibold text-white">{APP.productName}</div>
            <div>{t('app.tagline')}</div>
            <div className="text-xs text-white/40">
              {APP.identifier} · {APP.channel}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

/** 表示設定: インライン画像の自動取得など。 */
function DisplaySettings() {
  const { t } = useTranslation();
  const [inline, setInline] = useState(getInlineImages());

  const toggleInline = () => {
    const next = !inline;
    setInline(next);
    setInlineImages(next);
  };

  return (
    <div className="space-y-4">
      <label className="flex cursor-pointer items-start justify-between gap-4">
        <span>
          <span className="block text-sm text-white/85">{t('settings.inlineImages')}</span>
          <span className="mt-0.5 block text-xs text-white/45">
            {t('settings.inlineImagesHint')}
          </span>
        </span>
        <button
          type="button"
          role="switch"
          aria-checked={inline}
          onClick={toggleInline}
          className={`relative mt-0.5 h-5 w-9 shrink-0 rounded-full transition-colors ${
            inline ? 'bg-sky-500' : 'bg-white/20'
          }`}
        >
          <span
            className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-transform ${
              inline ? 'translate-x-4' : 'translate-x-0.5'
            }`}
          />
        </button>
      </label>
    </div>
  );
}

/** データの保存先: 現在地・使用量の表示と、別フォルダへの移動／既定に戻す。 */
function DataLocationSettings() {
  const { t } = useTranslation();
  const [loc, setLoc] = useState<DataLocation | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri) return;
    dataLocation()
      .then(setLoc)
      .catch(() => undefined);
  }, []);

  const change = async () => {
    if (busy) return;
    const dir = await open({ directory: true, multiple: false }).catch(() => null);
    if (typeof dir !== 'string') return;
    setBusy(true);
    setError(null);
    try {
      setLoc(await dataRelocate(dir));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const reset = async () => {
    if (busy || loc?.is_default) return;
    setBusy(true);
    setError(null);
    try {
      setLoc(await dataResetLocation());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const total = loc ? loc.db_bytes + loc.attachments_bytes : 0;

  return (
    <div className="max-w-xl space-y-4">
      <div>
        <div className="flex items-center gap-2 text-sm text-white/85">
          <HardDrive size={16} />
          {t('dataloc.title')}
        </div>
        <p className="mt-0.5 text-xs text-white/45">{t('dataloc.hint')}</p>
      </div>

      <div className="rounded-lg bg-white/5 p-3">
        <div className="mb-1 text-xs text-white/45">
          {t('dataloc.current')}
          {loc?.is_default && <span className="ml-2 text-white/35">({t('dataloc.default')})</span>}
        </div>
        <div className="break-all font-mono text-xs text-white/80">
          {loc ? loc.dir : '…'}
        </div>
        {loc && (
          <div className="mt-2 flex flex-wrap gap-x-4 gap-y-0.5 text-xs text-white/50">
            <span>{t('dataloc.total', { size: formatBytes(total) })}</span>
            <span>{t('dataloc.db', { size: formatBytes(loc.db_bytes) })}</span>
            <span>{t('dataloc.attachments', { size: formatBytes(loc.attachments_bytes) })}</span>
          </div>
        )}
      </div>

      <div className="flex items-center gap-3">
        <button
          onClick={change}
          disabled={busy}
          className="flex items-center gap-1.5 rounded-md bg-white/15 px-3 py-2 text-sm font-medium hover:bg-white/25 disabled:opacity-40"
        >
          <FolderInput size={15} />
          {busy ? t('dataloc.moving') : t('dataloc.change')}
        </button>
        <button
          onClick={reset}
          disabled={busy || !loc || loc.is_default}
          className="flex items-center gap-1.5 rounded-md border border-white/20 px-3 py-2 text-sm text-white/70 hover:bg-white/10 disabled:opacity-40"
        >
          <RotateCcw size={15} />
          {t('dataloc.reset')}
        </button>
      </div>

      {error && <p className="text-sm text-red-300">{error}</p>}
      <p className="text-xs text-white/40">{t('dataloc.note')}</p>
    </div>
  );
}
