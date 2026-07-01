import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { SpamSettings as SpamSettingsType } from '@bindings/SpamSettings';
import { APP } from '../config/appIdentity';
import {
  getFlyAnimation,
  getInlineImages,
  setFlyAnimation,
  setInlineImages,
} from '../config/prefs';
import { spamSettingsGet, spamSettingsSet } from '../services/spam';
import { AccountSetup } from './AccountSetup';
import { SignatureManager } from './SignatureManager';
import { TagManager } from './TagManager';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

// 迷惑メール設定の既定値（バックエンド未接続のプレビューでも UI を出せるように）。
// 実値はアプリ起動時に spam_settings_get で上書きする（DB が単一ソース）。
const SPAM_DEFAULTS: SpamSettingsType = { enabled: true, threshold_low: 0.5, threshold_high: 0.9 };

type Section = 'accounts' | 'signatures' | 'tags' | 'display' | 'spam' | 'about';

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

/** 表示設定: インライン画像の自動取得、送信アニメーション（つばめ）など。 */
function DisplaySettings() {
  const { t } = useTranslation();
  const [inline, setInline] = useState(getInlineImages());
  const [fly, setFly] = useState(getFlyAnimation());

  const toggleInline = () => {
    const next = !inline;
    setInline(next);
    setInlineImages(next);
  };

  const toggleFly = () => {
    const next = !fly;
    setFly(next);
    setFlyAnimation(next);
  };

  return (
    <div className="space-y-4">
      <ToggleRow
        label={t('settings.inlineImages')}
        hint={t('settings.inlineImagesHint')}
        checked={inline}
        onToggle={toggleInline}
      />
      <ToggleRow
        label={t('settings.flyAnimation')}
        hint={t('settings.flyAnimationHint')}
        checked={fly}
        onToggle={toggleFly}
      />
    </div>
  );
}

/** ラベル＋説明＋スイッチの1行（設定トグルの共通形）。 */
function ToggleRow({
  label,
  hint,
  checked,
  onToggle,
}: {
  label: string;
  hint: string;
  checked: boolean;
  onToggle: () => void;
}) {
  return (
    <label className="flex cursor-pointer items-start justify-between gap-4">
      <span>
        <span className="block text-sm text-white/85">{label}</span>
        <span className="mt-0.5 block text-xs text-white/45">{hint}</span>
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={onToggle}
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
