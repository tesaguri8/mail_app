import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { FolderInput, HardDrive, RotateCcw, RefreshCw, Link2, Unlink } from 'lucide-react';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { SpamSettings as SpamSettingsType } from '@bindings/SpamSettings';
import type { DataLocation } from '@bindings/DataLocation';
import { APP } from '../config/appIdentity';
import {
  getFlyAnimation,
  getInlineImages,
  getBubbleHtml,
  getRemoteImageMode,
  setFlyAnimation,
  setInlineImages,
  setBubbleHtml,
  setRemoteImageMode,
  type RemoteImageMode,
  getPhoneRegion,
  setPhoneRegion,
  getPhoneStyle,
  setPhoneStyle,
  getPostalAutoformat,
  setPostalAutoformat,
  getAutoSyncOn,
  setAutoSyncOn,
  getAutoSyncSeconds,
  setAutoSyncSeconds,
  getHomeCountShow,
  setHomeCountShow,
  getHomeCountMode,
  setHomeCountMode,
  getPhoneAutoformat,
  setPhoneAutoformat,
  type HomeCountMode,
} from '../config/prefs';
import { countryOptions } from '../utils/phone';
import { spamSettingsGet, spamSettingsSet } from '../services/spam';
import { dataLocation, dataRelocate, dataResetLocation } from '../services/data';
import {
  trashRetentionGet,
  trashRetentionSet,
  trashPurge,
  mailTrashRetentionGet,
  mailTrashRetentionSet,
  mailTrashPurge,
} from '../services/trash';
import {
  gcalAccounts,
  gcalConnect,
  gcalCredentialsStatus,
  gcalDisconnect,
  gcalSetCredentials,
  gcalSync,
} from '../services/gcal';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import type { GcalCredentialsStatus } from '@bindings/GcalCredentialsStatus';
import { AccountSetup } from './AccountSetup';
import { SignatureManager } from './SignatureManager';
import { TagManager } from './TagManager';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

// 迷惑メール設定の既定値（バックエンド未接続のプレビューでも UI を出せるように）。
// 実値はアプリ起動時に spam_settings_get で上書きする（DB が単一ソース）。
const SPAM_DEFAULTS: SpamSettingsType = { enabled: true, threshold_low: 0.5, threshold_high: 0.9 };

type Section =
  | 'accounts'
  | 'signatures'
  | 'tags'
  | 'display'
  | 'calendar'
  | 'spam'
  | 'data'
  | 'about';

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
    { key: 'calendar', label: t('settings.calendarSync') },
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
        {section === 'calendar' && <GoogleCalendarSettings />}
        {section === 'spam' && <SpamSettings />}
        {section === 'data' && (
          <div className="space-y-6">
            <DataLocationSettings />
            <div className="border-t border-white/10 pt-5">
              <MailTrashSettings />
            </div>
            <div className="border-t border-white/10 pt-5">
              <TrashSettings />
            </div>
          </div>
        )}
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
        {/* left-0 を明示（button は text-align:center のため、無指定だと静的位置＝中央から translate されてはみ出す） */}
        <span
          className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-white transition-transform ${
            checked ? 'translate-x-4' : ''
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
  const [bubbleHtml, setBubbleHtmlState] = useState(getBubbleHtml());
  const [remoteImgMode, setRemoteImgMode] = useState<RemoteImageMode>(getRemoteImageMode());
  const [fly, setFly] = useState(getFlyAnimation());
  const [region, setRegion] = useState(getPhoneRegion());
  const [style, setStyle] = useState(getPhoneStyle());
  const [postal, setPostal] = useState(getPostalAutoformat());
  // 自動同期: オン/オフのトグル＋間隔（秒・最短10）。入力は文字列で保持し確定時に保存。
  const [syncOn, setSyncOn] = useState(getAutoSyncOn());
  const [syncSec, setSyncSec] = useState(String(getAutoSyncSeconds()));
  // ホームのバッジ: 表示トグル＋種類（未読数/全数）。
  const [countShow, setCountShow] = useState(getHomeCountShow());
  const [countMode, setCountMode] = useState<HomeCountMode>(getHomeCountMode());
  const [phoneFmt, setPhoneFmt] = useState(getPhoneAutoformat());
  const countries = useMemo(() => countryOptions(i18n.language), [i18n.language]);

  const commitSyncSec = () => {
    const n = Number(syncSec);
    setAutoSyncSeconds(Number.isFinite(n) ? n : 30);
    setSyncSec(String(getAutoSyncSeconds())); // クランプ後の実値を表示に反映
  };

  return (
    <div className="max-w-xl space-y-5">
      {/* 自動同期: ホーム/メールモード滞在中の定期同期（画面遷移時は常に同期） */}
      <div>
        <Toggle
          checked={syncOn}
          onChange={() => {
            const next = !syncOn;
            setSyncOn(next);
            setAutoSyncOn(next);
          }}
          title={t('settings.autoSync')}
          hint={t('settings.autoSyncHint')}
        />
        {syncOn && (
          <label className="mt-2 flex items-center gap-2">
            <input
              type="number"
              min={10}
              step={5}
              value={syncSec}
              onChange={(e) => setSyncSec(e.target.value)}
              onBlur={commitSyncSec}
              onKeyDown={(e) => e.key === 'Enter' && commitSyncSec()}
              className="w-24 rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15"
            />
            <span className="text-xs text-white/50">{t('settings.autoSyncUnit')}</span>
          </label>
        )}
      </div>

      {/* ホームのアカウント別バッジ: 表示トグル＋種類（未読数/全数） */}
      <div className="border-t border-white/10 pt-4">
        <Toggle
          checked={countShow}
          onChange={() => {
            const next = !countShow;
            setCountShow(next);
            setHomeCountShow(next);
          }}
          title={t('settings.homeCount')}
          hint={t('settings.homeCountHint')}
        />
        {countShow && (
          <select
            value={countMode}
            onChange={(e) => {
              const m = e.target.value as HomeCountMode;
              setCountMode(m);
              setHomeCountMode(m);
            }}
            className="mt-2 w-48 rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15"
          >
            <option value="unread" className="text-black">
              {t('settings.homeCountUnread')}
            </option>
            <option value="total" className="text-black">
              {t('settings.homeCountTotal')}
            </option>
          </select>
        )}
      </div>

      <div className="border-t border-white/10 pt-4" />
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

      {/* 外部（リモート）画像の既定表示モード。プライバシー既定は「非表示」。メールごとに一時変更可。 */}
      <label className="block">
        <span className="block text-sm text-white/85">{t('settings.remoteImages')}</span>
        <span className="mt-0.5 block text-xs text-white/45">{t('settings.remoteImagesHint')}</span>
        <select
          value={remoteImgMode}
          onChange={(e) => {
            const m = e.target.value as RemoteImageMode;
            setRemoteImgMode(m);
            setRemoteImageMode(m);
          }}
          className="mt-2 w-56 rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15"
        >
          <option value="hidden" className="text-black">
            {t('settings.remoteHidden')}
          </option>
          <option value="thumb" className="text-black">
            {t('settings.remoteThumb')}
          </option>
          <option value="full" className="text-black">
            {t('settings.remoteFull')}
          </option>
        </select>
      </label>

      {/* 会話バブルを HTML 本文で描画（既定オフ）。画像は取得せずプレースホルダのまま＝軽量。 */}
      <Toggle
        checked={bubbleHtml}
        onChange={() => {
          const next = !bubbleHtml;
          setBubbleHtmlState(next);
          setBubbleHtml(next);
        }}
        title={t('settings.bubbleHtml')}
        hint={t('settings.bubbleHtmlHint')}
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
        <Toggle
          checked={phoneFmt}
          onChange={() => {
            const next = !phoneFmt;
            setPhoneFmt(next);
            setPhoneAutoformat(next);
          }}
          title={t('settings.phoneTitle')}
          hint={t('settings.phoneHint')}
        />
        {phoneFmt && (
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
        )}
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
      <Toggle
        checked={settings.enabled}
        onChange={() => save({ ...settings, enabled: !settings.enabled })}
        title={t('settings.spamEnabled')}
        hint={t('settings.spamEnabledHint')}
      />

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

/** 住所録のゴミ箱設定（論理削除の保持日数・今すぐ完全削除）。 */
function TrashSettings() {
  const { t } = useTranslation();
  const [days, setDays] = useState('7');
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (!isTauri) return;
    trashRetentionGet()
      .then((d) => setDays(String(d)))
      .catch(() => undefined);
  }, []);

  const commit = async () => {
    const n = Math.max(0, Math.round(Number(days)) || 0);
    setDays(String(n));
    if (!isTauri) return;
    try {
      await trashRetentionSet(n);
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
    } catch {
      /* noop */
    }
  };

  const purgeNow = async () => {
    if (!isTauri) return;
    if (!window.confirm(t('settings.trashPurgeConfirm'))) return;
    try {
      await trashPurge();
    } catch {
      /* noop */
    }
  };

  return (
    <div>
      <div className="text-sm font-semibold text-white">{t('settings.trashTitle')}</div>
      <p className="mt-0.5 text-xs text-white/45">{t('settings.trashHint')}</p>
      <div className="mt-3 flex items-end gap-3">
        <label className="block">
          <span className="mb-1 block text-xs text-white/50">{t('settings.trashRetention')}</span>
          <div className="flex items-center gap-2">
            <input
              type="number"
              min={0}
              value={days}
              onChange={(e) => setDays(e.target.value)}
              onBlur={commit}
              className="w-24 rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15"
            />
            <span className="text-sm text-white/60">{t('settings.trashDaysUnit')}</span>
          </div>
        </label>
        {saved && <span className="pb-1.5 text-xs text-emerald-300">{t('contact.saved')}</span>}
      </div>
      <button
        onClick={purgeNow}
        className="mt-3 rounded-md border border-white/20 px-3 py-1.5 text-sm text-white/70 hover:bg-white/10"
      >
        {t('settings.trashPurgeNow')}
      </button>
    </div>
  );
}

/**
 * Google カレンダー連携（双方向同期。docs/CALENDAR_SYNC.md）。
 * OAuth クライアント認証情報の入力 → アカウント連携（ブラウザ同意）→ 今すぐ同期／解除。
 */
function GoogleCalendarSettings() {
  const { t } = useTranslation();
  const [creds, setCreds] = useState<GcalCredentialsStatus | null>(null);
  const [clientId, setClientId] = useState('');
  const [clientSecret, setClientSecret] = useState('');
  const [accounts, setAccounts] = useState<GoogleAccount[]>([]);
  const [busy, setBusy] = useState<'idle' | 'saving' | 'connecting' | 'syncing'>('idle');
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => {
    if (!isTauri) return;
    gcalCredentialsStatus().then(setCreds).catch(() => undefined);
    gcalAccounts().then(setAccounts).catch(() => setAccounts([]));
  };
  useEffect(refresh, []);

  const saveCreds = async () => {
    if (!isTauri || busy !== 'idle') return;
    setBusy('saving');
    setError(null);
    setMessage(null);
    try {
      await gcalSetCredentials(clientId.trim(), clientSecret.trim());
      setClientId('');
      setClientSecret('');
      setMessage(t('settings.gcalSaved'));
      gcalCredentialsStatus().then(setCreds).catch(() => undefined);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy('idle');
    }
  };

  const connect = async () => {
    if (!isTauri || busy !== 'idle') return;
    if (!creds?.configured) {
      setError(t('settings.gcalNeedCredentials'));
      return;
    }
    setBusy('connecting');
    setError(null);
    setMessage(null);
    try {
      await gcalConnect();
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy('idle');
    }
  };

  const syncNow = async (id: number) => {
    if (!isTauri || busy !== 'idle') return;
    setBusy('syncing');
    setError(null);
    setMessage(null);
    try {
      const r = await gcalSync(id);
      setMessage(
        t('settings.gcalSyncDone', {
          pulled: r.pulled,
          pushed: r.pushed,
          deletedIn: r.deleted_in,
          deletedOut: r.deleted_out,
        }),
      );
      gcalAccounts().then(setAccounts).catch(() => undefined);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy('idle');
    }
  };

  const disconnect = async (id: number) => {
    if (!isTauri || busy !== 'idle') return;
    if (!window.confirm(t('settings.gcalDisconnectConfirm'))) return;
    setError(null);
    setMessage(null);
    try {
      await gcalDisconnect(id);
      refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="max-w-xl space-y-6">
      <div>
        <div className="text-base font-semibold text-white">{t('settings.gcalTitle')}</div>
        <p className="mt-0.5 text-xs text-white/45">{t('settings.gcalIntro')}</p>
      </div>

      {/* OAuth クライアント認証情報 */}
      <div className="space-y-3 rounded-lg bg-white/5 p-4">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-white/85">{t('settings.gcalCredentials')}</span>
          <span
            className={`rounded-full px-2 py-0.5 text-xs ${
              creds?.configured ? 'bg-emerald-500/20 text-emerald-200' : 'bg-white/10 text-white/50'
            }`}
          >
            {creds?.configured
              ? `${t('settings.gcalConfigured')}${creds.client_id_hint ? ` (${creds.client_id_hint})` : ''}`
              : t('settings.gcalNotConfigured')}
          </span>
        </div>
        <p className="text-xs text-white/45">{t('settings.gcalCredentialsHint')}</p>
        <label className="block">
          <span className="mb-1 block text-xs text-white/50">{t('settings.gcalClientId')}</span>
          <input
            type="text"
            value={clientId}
            onChange={(e) => setClientId(e.target.value)}
            placeholder="xx…apps.googleusercontent.com"
            className="w-full rounded bg-white/10 px-2 py-1.5 font-mono text-xs outline-none focus:bg-white/15"
          />
        </label>
        <label className="block">
          <span className="mb-1 block text-xs text-white/50">{t('settings.gcalClientSecret')}</span>
          <input
            type="password"
            value={clientSecret}
            onChange={(e) => setClientSecret(e.target.value)}
            placeholder="GOCSPX-…"
            className="w-full rounded bg-white/10 px-2 py-1.5 font-mono text-xs outline-none focus:bg-white/15"
          />
        </label>
        <button
          onClick={saveCreds}
          disabled={busy !== 'idle' || !clientId.trim() || !clientSecret.trim()}
          className="rounded-md bg-white/15 px-3 py-1.5 text-sm font-medium hover:bg-white/25 disabled:opacity-40"
        >
          {busy === 'saving' ? '…' : t('settings.gcalSave')}
        </button>
      </div>

      {/* 連携ボタン */}
      <div>
        <button
          onClick={connect}
          disabled={busy !== 'idle' || !creds?.configured}
          className="flex items-center gap-1.5 rounded-md bg-sky-500/90 px-3 py-2 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-40"
        >
          <Link2 size={15} />
          {busy === 'connecting' ? t('settings.gcalConnecting') : t('settings.gcalConnect')}
        </button>
        <p className="mt-2 text-xs text-white/40">{t('settings.gcalTestUserNote')}</p>
      </div>

      {/* 連携中アカウント一覧 */}
      <div>
        <div className="mb-2 text-sm font-medium text-white/85">{t('settings.gcalAccounts')}</div>
        {accounts.length === 0 ? (
          <p className="text-xs text-white/40">{t('settings.gcalNoAccounts')}</p>
        ) : (
          <ul className="space-y-2">
            {accounts.map((a) => (
              <li
                key={a.id}
                className="flex items-center justify-between gap-3 rounded-lg bg-white/5 px-3 py-2"
              >
                <div className="min-w-0">
                  <div className="truncate text-sm text-white/90">{a.email}</div>
                  <div className="text-xs text-white/40">
                    {a.last_sync_at
                      ? t('settings.gcalLastSync', {
                          // SQLite の CURRENT_TIMESTAMP は 'YYYY-MM-DD HH:MM:SS'(UTC)。ISO 化して解釈。
                          when: new Date(a.last_sync_at.replace(' ', 'T') + 'Z').toLocaleString(),
                        })
                      : t('settings.gcalNeverSynced')}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <button
                    onClick={() => syncNow(a.id)}
                    disabled={busy !== 'idle'}
                    className="flex items-center gap-1 rounded-md bg-white/15 px-2.5 py-1.5 text-xs font-medium hover:bg-white/25 disabled:opacity-40"
                  >
                    <RefreshCw size={13} className={busy === 'syncing' ? 'animate-spin' : ''} />
                    {busy === 'syncing' ? t('settings.gcalSyncing') : t('settings.gcalSyncNow')}
                  </button>
                  <button
                    onClick={() => disconnect(a.id)}
                    disabled={busy !== 'idle'}
                    className="flex items-center gap-1 rounded-md border border-white/20 px-2.5 py-1.5 text-xs text-white/70 hover:bg-white/10 disabled:opacity-40"
                  >
                    <Unlink size={13} />
                    {t('settings.gcalDisconnect')}
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>

      {message && <p className="text-sm text-emerald-300">{message}</p>}
      {error && <p className="text-sm text-red-300">{t('settings.gcalError', { message: error })}</p>}
      {!isTauri && <p className="text-xs text-white/40">{t('settings.spamPreviewNote')}</p>}
    </div>
  );
}

/** メールのゴミ箱設定（自動削除日数・無期限・今すぐ空にする）。連絡先用とは別系統。 */
function MailTrashSettings() {
  const { t } = useTranslation();
  const [days, setDays] = useState('30');
  const [unlimited, setUnlimited] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (!isTauri) return;
    mailTrashRetentionGet()
      .then((d) => {
        setUnlimited(d === 0);
        // 無期限（0）でも数値欄には目安として 30 を残す。
        setDays(d === 0 ? '30' : String(d));
      })
      .catch(() => undefined);
  }, []);

  const persist = async (value: number) => {
    if (!isTauri) return;
    try {
      await mailTrashRetentionSet(value);
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
    } catch {
      /* noop */
    }
  };

  const commitDays = async () => {
    const n = Math.max(1, Math.round(Number(days)) || 30);
    setDays(String(n));
    if (unlimited) return; // 無期限中は数値の変更を保存しない
    await persist(n);
  };

  const toggleUnlimited = async (on: boolean) => {
    setUnlimited(on);
    if (on) {
      await persist(0); // 0 = 無期限（自動削除しない）
    } else {
      const n = Math.max(1, Math.round(Number(days)) || 30);
      setDays(String(n));
      await persist(n);
    }
  };

  const purgeNow = async () => {
    if (!isTauri) return;
    if (!window.confirm(t('settings.mailTrashPurgeConfirm'))) return;
    try {
      await mailTrashPurge();
    } catch {
      /* noop */
    }
  };

  return (
    <div>
      <div className="text-sm font-semibold text-white">{t('settings.mailTrashTitle')}</div>
      <p className="mt-0.5 text-xs text-white/45">{t('settings.mailTrashHint')}</p>
      <div className="mt-3 flex items-end gap-3">
        <label className="block">
          <span className="mb-1 block text-xs text-white/50">
            {t('settings.mailTrashRetention')}
          </span>
          <div className="flex items-center gap-2">
            <input
              type="number"
              min={1}
              value={days}
              disabled={unlimited}
              onChange={(e) => setDays(e.target.value)}
              onBlur={commitDays}
              className="w-24 rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15 disabled:opacity-40"
            />
            <span className="text-sm text-white/60">{t('settings.trashDaysUnit')}</span>
          </div>
        </label>
        {saved && <span className="pb-1.5 text-xs text-emerald-300">{t('contact.saved')}</span>}
      </div>
      <label className="mt-2 flex items-center gap-2 text-sm text-white/70">
        <input
          type="checkbox"
          checked={unlimited}
          onChange={(e) => toggleUnlimited(e.target.checked)}
        />
        {t('settings.mailTrashUnlimited')}
      </label>
      <button
        onClick={purgeNow}
        className="mt-3 rounded-md border border-white/20 px-3 py-1.5 text-sm text-white/70 hover:bg-white/10"
      >
        {t('settings.mailTrashPurgeNow')}
      </button>
    </div>
  );
}
