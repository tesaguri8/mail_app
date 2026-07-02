import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { FolderInput, HardDrive, RotateCcw } from 'lucide-react';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { SpamSettings as SpamSettingsType } from '@bindings/SpamSettings';
import type { DataLocation } from '@bindings/DataLocation';
import { APP } from '../config/appIdentity';
import {
  getFlyAnimation,
  getInlineImages,
  setFlyAnimation,
  setInlineImages,
  getPhoneRegion,
  setPhoneRegion,
  getPhoneStyle,
  setPhoneStyle,
  getPostalAutoformat,
  setPostalAutoformat,
} from '../config/prefs';
import { countryOptions } from '../utils/phone';
import { spamSettingsGet, spamSettingsSet } from '../services/spam';
import { dataLocation, dataRelocate, dataResetLocation } from '../services/data';
import { AccountSetup } from './AccountSetup';
import { SignatureManager } from './SignatureManager';
import { TagManager } from './TagManager';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

// 迷惑メール設定の既定値（バックエンド未接続のプレビューでも UI を出せるように）。
// 実値はアプリ起動時に spam_settings_get で上書きする（DB が単一ソース）。
const SPAM_DEFAULTS: SpamSettingsType = { enabled: true, threshold_low: 0.5, threshold_high: 0.9 };

type Section = 'accounts' | 'signatures' | 'tags' | 'display' | 'spam' | 'data' | 'about';

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
    { key: 'spam', label: t('settings.spam') },
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
        {section === 'spam' && <SpamSettings />}
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

/** オン/オフのトグルスイッチ。 */
function Toggle({
  checked,
  onChange,
  title,
  hint,
}: {
  checked: boolean;
  onChange: () => void;
  title: string;
  hint: string;
}) {
  return (
    <label className="flex cursor-pointer items-start justify-between gap-4">
      <span>
        <span className="block text-sm text-white/85">{title}</span>
        <span className="mt-0.5 block text-xs text-white/45">{hint}</span>
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={onChange}
        className={`relative mt-0.5 h-5 w-9 shrink-0 rounded-full transition-colors ${
          checked ? 'bg-sky-500' : 'bg-white/20'
        }`}
      >
        <span
          className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-transform ${
            checked ? 'translate-x-4' : 'translate-x-0.5'
          }`}
        />
      </button>
    </label>
  );
}

/** 表示設定: インライン画像の自動取得・送信アニメーション・電話/郵便番号の整形など。 */
function DisplaySettings() {
  const { t, i18n } = useTranslation();
  const [inline, setInline] = useState(getInlineImages());
  const [fly, setFly] = useState(getFlyAnimation());
  const [region, setRegion] = useState(getPhoneRegion());
  const [style, setStyle] = useState(getPhoneStyle());
  const [postal, setPostal] = useState(getPostalAutoformat());
  const countries = useMemo(() => countryOptions(i18n.language), [i18n.language]);

  return (
    <div className="max-w-xl space-y-5">
      <Toggle
        checked={inline}
        onChange={() => {
          const next = !inline;
          setInline(next);
          setInlineImages(next);
        }}
        title={t('settings.inlineImages')}
        hint={t('settings.inlineImagesHint')}
      />
      <Toggle
        checked={fly}
        onChange={() => {
          const next = !fly;
          setFly(next);
          setFlyAnimation(next);
        }}
        title={t('settings.flyAnimation')}
        hint={t('settings.flyAnimationHint')}
      />

      <div className="border-t border-white/10 pt-4">
        <div className="text-sm text-white/85">{t('settings.phoneTitle')}</div>
        <p className="mt-0.5 text-xs text-white/45">{t('settings.phoneHint')}</p>
        <div className="mt-3 grid grid-cols-2 gap-3">
          <label className="block">
            <span className="mb-1 block text-xs text-white/50">{t('settings.phoneRegion')}</span>
            <select
              value={region}
              onChange={(e) => {
                setRegion(e.target.value);
                setPhoneRegion(e.target.value);
              }}
              className="w-full rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15"
            >
              {countries.map((c) => (
                <option key={c.region} value={c.region} className="text-black">
                  {c.name} (+{c.calling})
                </option>
              ))}
            </select>
          </label>
          <label className="block">
            <span className="mb-1 block text-xs text-white/50">{t('settings.phoneStyle')}</span>
            <select
              value={style}
              onChange={(e) => {
                const s = e.target.value as 'national' | 'international';
                setStyle(s);
                setPhoneStyle(s);
              }}
              className="w-full rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15"
            >
              <option value="national" className="text-black">
                {t('settings.phoneStyleNational')}
              </option>
              <option value="international" className="text-black">
                {t('settings.phoneStyleInternational')}
              </option>
            </select>
          </label>
        </div>
      </div>

      <div className="border-t border-white/10 pt-4">
        <Toggle
          checked={postal}
          onChange={() => {
            const next = !postal;
            setPostal(next);
            setPostalAutoformat(next);
          }}
          title={t('settings.postalAutoformat')}
          hint={t('settings.postalAutoformatHint')}
        />
      </div>
    </div>
  );
}

/** 迷惑メール設定: オン/オフと隔離しきい値（docs/SPAM.md §9）。DB を単一ソースにする。 */
function SpamSettings() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<SpamSettingsType>(SPAM_DEFAULTS);

  useEffect(() => {
    if (!isTauri) return;
    spamSettingsGet()
      .then(setSettings)
      .catch(() => undefined);
  }, []);

  // 変更は即保存（ハードコードせず DB に単一ソースで持つ。§9.2）。
  const save = (next: SpamSettingsType) => {
    setSettings(next);
    if (isTauri) spamSettingsSet(next).catch(() => undefined);
  };

  return (
    <div className="max-w-[460px] space-y-4">
      <label className="flex cursor-pointer items-start justify-between gap-4">
        <span>
          <span className="block text-sm text-white/85">{t('settings.spamEnabled')}</span>
          <span className="mt-0.5 block text-xs text-white/45">{t('settings.spamEnabledHint')}</span>
        </span>
        <button
          type="button"
          role="switch"
          aria-checked={settings.enabled}
          onClick={() => save({ ...settings, enabled: !settings.enabled })}
          className={`relative mt-0.5 h-5 w-9 shrink-0 rounded-full transition-colors ${
            settings.enabled ? 'bg-sky-500' : 'bg-white/20'
          }`}
        >
          <span
            className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-transform ${
              settings.enabled ? 'translate-x-4' : 'translate-x-0.5'
            }`}
          />
        </button>
      </label>

      {settings.enabled && (
        <div className="space-y-4 border-t border-white/10 pt-4">
          <ThresholdSlider
            label={t('settings.spamThresholdHigh')}
            hint={t('settings.spamThresholdHighHint')}
            value={settings.threshold_high}
            onChange={(v) => save({ ...settings, threshold_high: v })}
          />
          <ThresholdSlider
            label={t('settings.spamThresholdLow')}
            hint={t('settings.spamThresholdLowHint')}
            value={settings.threshold_low}
            onChange={(v) => save({ ...settings, threshold_low: v })}
          />
        </div>
      )}

      {!isTauri && <p className="text-xs text-white/40">{t('settings.spamPreviewNote')}</p>}
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

/** 0..1 のしきい値スライダー（％表示つき）。 */
function ThresholdSlider({
  label,
  hint,
  value,
  onChange,
}: {
  label: string;
  hint: string;
  value: number;
  onChange: (v: number) => void;
}) {
  return (
    <label className="block">
      <span className="flex items-center justify-between text-sm text-white/85">
        <span>{label}</span>
        <span className="text-xs text-white/50">{Math.round(value * 100)}%</span>
      </span>
      <input
        type="range"
        min={0}
        max={1}
        step={0.05}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="mt-1 w-full"
      />
      <span className="mt-0.5 block text-xs text-white/45">{hint}</span>
    </label>
  );
}
