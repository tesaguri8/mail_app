import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Cloud, Link2, RefreshCw, Unlink } from 'lucide-react';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import type { GoogleCredentialsStatus } from '@bindings/GoogleCredentialsStatus';
import {
  googleAccounts,
  googleConnect,
  googleCredentialsStatus,
  googleDisconnect,
  googleSetCredentials,
  googleSync,
} from '../services/google';
import { gcontactsSetPushNew } from '../services/gcontacts';
import { summarizeGoogleSync } from '../utils/googleSyncSummary';
import { ConfirmDialog } from './ConfirmDialog';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 設定の「同期」。外部サービスとの同期（連絡先・カレンダー）を、サービスごとの欄に分けて並べる。
 * メールのアカウント（iCloud メールなど）はここではなく「アカウント」に置く。
 */
export function SyncSettings() {
  const { t } = useTranslation();
  return (
    <div className="max-w-xl space-y-8">
      <div>
        <div className="text-base font-semibold text-white">{t('settings.syncTitle')}</div>
        <p className="mt-0.5 text-xs text-white/45">{t('settings.syncIntro')}</p>
      </div>
      <GoogleSyncSection />
      <div className="border-t border-white/10 pt-6">
        <ICloudSyncSection />
      </div>
    </div>
  );
}

/** 欄の見出し（サービス名と、その欄で同期するもの）。 */
function SectionHeading({
  icon,
  title,
  hint,
}: {
  icon: React.ReactNode;
  title: string;
  hint: string;
}) {
  return (
    <div>
      <div className="flex items-center gap-2 text-sm font-semibold text-white/90">
        {icon}
        {title}
      </div>
      <p className="mt-0.5 text-xs text-white/45">{hint}</p>
    </div>
  );
}

/** SQLite の CURRENT_TIMESTAMP（'YYYY-MM-DD HH:MM:SS'・UTC）を手元の時刻表記へ。 */
const localTime = (utc: string) => new Date(utc.replace(' ', 'T') + 'Z').toLocaleString();

/**
 * iCloud の欄。同期（CardDAV / CalDAV）を実装するまでは案内だけを出す（押せるものは置かない）。
 */
function ICloudSyncSection() {
  const { t } = useTranslation();
  return (
    <div className="space-y-3">
      <SectionHeading
        icon={<Cloud size={16} className="text-sky-200" />}
        title={t('settings.icloudTitle')}
        hint={t('settings.icloudHint')}
      />
      <p className="rounded-lg bg-white/5 px-4 py-3 text-sm text-white/55">
        {t('settings.icloudComingSoon')}
      </p>
    </div>
  );
}

/**
 * Google の欄（docs/CALENDAR_SYNC.md・docs/CONTACTS_SYNC.md）。
 * OAuth クライアント認証情報 → アカウント連携（ブラウザ同意）→ 今すぐ同期／解除。
 * 「今すぐ同期」1 回でカレンダーと連絡先を同期し、取り込んだ連絡先を住所録まで反映する。
 */
function GoogleSyncSection() {
  const { t } = useTranslation();
  const [creds, setCreds] = useState<GoogleCredentialsStatus | null>(null);
  const [clientId, setClientId] = useState('');
  const [clientSecret, setClientSecret] = useState('');
  const [accounts, setAccounts] = useState<GoogleAccount[]>([]);
  const [busy, setBusy] = useState<'idle' | 'saving' | 'connecting' | 'syncing' | 'disconnecting'>(
    'idle'
  );
  // 解除の確認ダイアログ（null＝出していない）と、選んだ解除の種類。
  const [disconnecting, setDisconnecting] = useState<{
    account: GoogleAccount;
    purge: boolean;
  } | null>(null);
  // 連携時に連絡先スコープも要求するか（既定は off。カレンダーだけの利用者に権限を求めない）。
  const [withContacts, setWithContacts] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 結果・エラーの置き場。ボタンから離れた下に出るので、出たら画面内へ寄せる
  // （寄せないと、押しても何も起きないように見える）。
  const statusRef = useRef<HTMLDivElement>(null);
  const syncing = busy === 'syncing';
  useEffect(() => {
    if (message || error || syncing) {
      // 画面の下端には背景操作のバーが重なるので、端ではなく中央へ寄せる。
      statusRef.current?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
  }, [message, error, syncing]);

  const refresh = () => {
    if (!isTauri) return;
    googleCredentialsStatus()
      .then(setCreds)
      .catch(() => undefined);
    googleAccounts()
      .then(setAccounts)
      .catch(() => setAccounts([]));
  };
  useEffect(refresh, []);

  const saveCreds = async () => {
    if (!isTauri || busy !== 'idle') return;
    setBusy('saving');
    setError(null);
    setMessage(null);
    try {
      await googleSetCredentials(clientId.trim(), clientSecret.trim());
      setClientId('');
      setClientSecret('');
      setMessage(t('settings.gcalSaved'));
      googleCredentialsStatus()
        .then(setCreds)
        .catch(() => undefined);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy('idle');
    }
  };

  // 連携（再接続も同じ。同じアカウントを選べば解除中の記録を使い直す）。
  const connect = async (contacts: boolean = withContacts) => {
    if (!isTauri || busy !== 'idle') return;
    if (!creds?.configured) {
      setError(t('settings.gcalNeedCredentials'));
      return;
    }
    setBusy('connecting');
    setError(null);
    setMessage(null);
    try {
      await googleConnect(true, contacts);
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy('idle');
    }
  };

  // 今すぐ同期: カレンダーと連絡先（push → pull）を同期し、取り込んだ連絡先を住所録へ反映する。
  const syncNow = async (id: number) => {
    if (!isTauri || busy !== 'idle') return;
    setBusy('syncing');
    setError(null);
    setMessage(null);
    try {
      const r = await googleSync(id, true);
      setMessage(summarizeGoogleSync(r, t) || null);
      // 片方だけ失敗しても、もう片方の結果は出す。
      const errors = [r.calendar_error, r.contacts_error].filter((e): e is string => !!e);
      if (errors.length > 0) setError(errors.join(' / '));
      googleAccounts()
        .then(setAccounts)
        .catch(() => undefined);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy('idle');
    }
  };

  const togglePushNew = async (id: number, enabled: boolean) => {
    if (!isTauri || busy !== 'idle') return;
    setError(null);
    try {
      await gcontactsSetPushNew(id, enabled);
      setAccounts(await googleAccounts());
    } catch (e) {
      setError(String(e));
    }
  };

  // 解除は画面内で確認し、「一時的に解除（記録を残す・既定）」か「完全に解除」かを選ぶ。
  const disconnect = async () => {
    if (!isTauri || busy !== 'idle' || !disconnecting) return;
    const { account, purge } = disconnecting;
    setBusy('disconnecting');
    setError(null);
    setMessage(null);
    try {
      const r = await googleDisconnect(account.id, purge);
      setDisconnecting(null);
      setMessage(
        purge
          ? t('settings.gcalPurged', { email: account.email })
          : t('settings.gcalDisconnected', { email: account.email })
      );
      // 許可の取り消しに失敗しても解除は済んでいる。理由を添えて知らせる。
      if (r.revoke_error) setError(t('settings.gcalRevokeFailed', { message: r.revoke_error }));
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy('idle');
    }
  };

  return (
    <div className="space-y-6">
      <SectionHeading
        icon={
          <span className="flex h-4 w-4 items-center justify-center rounded-full bg-sky-400/25 text-[10px] font-bold text-sky-100">
            G
          </span>
        }
        title={t('settings.googleTitle')}
        hint={t('settings.googleHint')}
      />

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
        <label className="mb-2 flex items-start gap-2 text-sm text-white/85">
          <input
            type="checkbox"
            checked={withContacts}
            onChange={(e) => setWithContacts(e.target.checked)}
            className="mt-0.5"
          />
          <span>
            {t('settings.gcontactsOptIn')}
            <span className="mt-0.5 block text-xs text-white/40">
              {t('settings.gcontactsOptInHint')}
            </span>
          </span>
        </label>
        <button
          onClick={() => void connect()}
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
            {accounts.map((a) => {
              const disconnected = a.disconnected_at != null;
              return (
                <li key={a.id} className="space-y-2 rounded-lg bg-white/5 px-3 py-2">
                  <div className="flex items-center justify-between gap-3">
                    <div className="min-w-0">
                      <div className="flex items-center gap-1.5">
                        <span className="truncate text-sm text-white/90">{a.email}</span>
                        {disconnected && (
                          <span
                            className="shrink-0 rounded bg-amber-400/20 px-1.5 py-0.5 text-[10px] text-amber-200"
                            title={t('settings.gcalDisconnectedHint')}
                          >
                            {t('settings.gcalDisconnectedBadge')}
                          </span>
                        )}
                      </div>
                      <div className="text-xs text-white/40">
                        {a.last_calendar_sync_at
                          ? t('settings.gcalLastSync', { when: localTime(a.last_calendar_sync_at) })
                          : t('settings.gcalNeverSynced')}
                      </div>
                      {a.sync_contacts && (
                        <div className="text-xs text-white/40">
                          {a.last_contacts_sync_at
                            ? t('settings.gcontactsLastSync', {
                                when: localTime(a.last_contacts_sync_at),
                              })
                            : t('settings.gcontactsNeverSynced')}
                        </div>
                      )}
                    </div>
                    <div className="flex shrink-0 items-center gap-2">
                      {disconnected ? (
                        // 解除中: 同期はせず、再接続だけを出す（同じアカウントを選べば記録を使い直す）。
                        <button
                          onClick={() => void connect(a.sync_contacts)}
                          disabled={busy !== 'idle'}
                          className="flex items-center gap-1 rounded-md bg-sky-500/70 px-2.5 py-1.5 text-xs font-medium hover:bg-sky-500 disabled:opacity-40"
                        >
                          <Link2 size={13} />
                          {busy === 'connecting'
                            ? t('settings.gcalConnecting')
                            : t('settings.gcalReconnect')}
                        </button>
                      ) : (
                        <button
                          onClick={() => void syncNow(a.id)}
                          disabled={busy !== 'idle'}
                          className="flex items-center gap-1 rounded-md bg-white/15 px-2.5 py-1.5 text-xs font-medium hover:bg-white/25 disabled:opacity-40"
                        >
                          <RefreshCw size={13} className={syncing ? 'animate-spin' : ''} />
                          {syncing ? t('settings.gcalSyncing') : t('settings.gcalSyncNow')}
                        </button>
                      )}
                      <button
                        onClick={() =>
                          // 解除中なら残る選択肢は「完全に解除」だけ。
                          setDisconnecting({ account: a, purge: disconnected })
                        }
                        disabled={busy !== 'idle'}
                        className="flex items-center gap-1 rounded-md border border-white/20 px-2.5 py-1.5 text-xs text-white/70 hover:bg-white/10 disabled:opacity-40"
                      >
                        <Unlink size={13} />
                        {t('settings.gcalDisconnect')}
                      </button>
                    </div>
                  </div>
                  {/* 住所録を Google へ上げるかは利用者が決めることなので、既定は無効。 */}
                  {a.sync_contacts && !disconnected && (
                    <label className="flex items-center gap-2 text-xs text-white/60">
                      <input
                        type="checkbox"
                        checked={a.push_new_contacts}
                        onChange={(e) => togglePushNew(a.id, e.target.checked)}
                        disabled={busy !== 'idle'}
                      />
                      {t('settings.gcontactsPushNew')}
                    </label>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>

      <div ref={statusRef} className="space-y-2">
        {syncing && <p className="text-sm text-white/70">{t('settings.gcalSyncing')}</p>}
        {message && <p className="text-sm text-emerald-300">{message}</p>}
        {error && (
          <p className="text-sm text-red-300">{t('settings.gcalError', { message: error })}</p>
        )}
      </div>
      {!isTauri && <p className="text-xs text-white/40">{t('settings.spamPreviewNote')}</p>}

      {disconnecting && (
        <ConfirmDialog
          title={t('settings.gcalDisconnectTitle', { email: disconnecting.account.email })}
          body={t('settings.gcalDisconnectBody')}
          notes={[
            disconnecting.purge
              ? t('settings.gcalPurgeNote')
              : t('settings.gcalDisconnectKeepNote'),
          ]}
          confirmLabel={
            disconnecting.purge ? t('settings.gcalPurgeRun') : t('settings.gcalDisconnectRun')
          }
          danger={disconnecting.purge}
          busy={busy === 'disconnecting'}
          onConfirm={() => void disconnect()}
          onCancel={() => setDisconnecting(null)}
        >
          <div className="mt-3 space-y-1.5" role="radiogroup">
            {[false, true].map((purge) => {
              // 解除中のアカウントは、もう一時的な解除を選べない。
              const unavailable = !purge && disconnecting.account.disconnected_at != null;
              return (
                <label
                  key={String(purge)}
                  className={`flex items-start gap-2 rounded-md px-2.5 py-2 text-sm ${
                    disconnecting.purge === purge ? 'bg-white/10' : 'hover:bg-white/5'
                  } ${unavailable ? 'opacity-40' : 'cursor-pointer'}`}
                >
                  <input
                    type="radio"
                    name="gcal-disconnect-kind"
                    className="mt-1"
                    checked={disconnecting.purge === purge}
                    disabled={unavailable}
                    onChange={() => setDisconnecting({ ...disconnecting, purge })}
                  />
                  <span>
                    <span className="block text-white/90">
                      {purge ? t('settings.gcalPurgeOption') : t('settings.gcalDisconnectOption')}
                    </span>
                    <span className="block text-xs text-white/45">
                      {purge
                        ? t('settings.gcalPurgeOptionHint')
                        : t('settings.gcalDisconnectOptionHint')}
                    </span>
                  </span>
                </label>
              );
            })}
          </div>
        </ConfirmDialog>
      )}
    </div>
  );
}
