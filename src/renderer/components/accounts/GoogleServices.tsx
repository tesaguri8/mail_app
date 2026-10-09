import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CalendarDays, Link2, RefreshCw, Unlink, Users } from 'lucide-react';
import type { AccountProfile } from '@bindings/AccountProfile';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import type { GoogleCredentialsStatus } from '@bindings/GoogleCredentialsStatus';
import type { GoogleService } from '@bindings/GoogleService';
import {
  googleConnect,
  googleDisconnect,
  googleSetService,
  googleSync,
} from '../../services/google';
import { summarizeGoogleSync } from '../../utils/googleSyncSummary';
import { CALENDAR_SYNCED_EVENT, CONTACTS_SYNCED_EVENT } from '../../hooks/useAutoSync';
import { GoogleDisconnectDialog } from './GoogleDisconnectDialog';
import { ServiceRow } from './ServiceRow';
import { isTauri, localTime, sameAddress } from './shared';

type Busy = 'idle' | 'connecting' | 'switching' | 'syncing' | 'disconnecting';

/**
 * Google のカードの「連絡先」「カレンダー」と、連携の操作（今すぐ同期・解除・再接続・
 * 新しい連絡先の既定のチェック）。以前の設定の「同期」の Google 欄をカードの中へ移したもの
 * （docs/ACCOUNTS.md §2-1）。
 *
 * スイッチをオンにするとき、権限が無ければ Google でログイン（OAuth）してから同期する。
 * オフは同期を止めるだけで、取り込んだ連絡先・予定と連携は残る（記録ごと外すのは「解除」）。
 */
export function GoogleServices({
  profile,
  google,
  creds,
  onChanged,
}: {
  profile: AccountProfile;
  google: GoogleAccount | undefined;
  creds: GoogleCredentialsStatus | null;
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState<Busy>('idle');
  // どのスイッチを操作中か（そのスイッチだけを点滅させる）。
  const [pending, setPending] = useState<GoogleService | null>(null);
  const [disconnecting, setDisconnecting] = useState<{ purge: boolean } | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 結果・エラーはボタンから離れた下に出るので、出たら画面内へ寄せる（押しても何も起きない
  // ように見えるのを防ぐ。下端には背景操作のバーが重なるので中央へ）。
  const statusRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (message || error)
      statusRef.current?.scrollIntoView({ behavior: 'smooth', block: 'center' });
  }, [message, error]);

  const connected = google != null && google.disconnected_at == null;
  const disconnected = google != null && google.disconnected_at != null;

  const run = async (kind: Busy, work: () => Promise<void>) => {
    if (!isTauri || busy !== 'idle') return;
    setBusy(kind);
    setError(null);
    setMessage(null);
    try {
      await work();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy('idle');
      setPending(null);
      onChanged();
    }
  };

  // 同期して結果を出す。片方だけ失敗しても、もう片方の結果は出す。
  const syncNow = async (id: number, contacts: boolean) => {
    const r = await googleSync(id, { calendar: true, contacts });
    setMessage(summarizeGoogleSync(r, t) || null);
    const errors = [r.calendar_error, r.contacts_error].filter((e): e is string => !!e);
    if (errors.length > 0) setError(errors.join(' / '));
    // カレンダー・住所録の表示に読み直しを促す（自動同期と同じ合図）。
    window.dispatchEvent(new Event(CALENDAR_SYNCED_EVENT));
    if (contacts) window.dispatchEvent(new Event(CONTACTS_SYNCED_EVENT));
  };

  // Google でログイン（連携・追加の同意・再接続）。カードのアドレスを先に選んでおく。
  const login = async (calendar: boolean, contacts: boolean) => {
    if (!creds?.configured) {
      setError(t('settings.gcalNeedCredentials'));
      return;
    }
    const a = await googleConnect(calendar, contacts, profile.email);
    if (!sameAddress(a.email, profile.email)) {
      // 別のアカウントを選んだ。そのアドレスのカードに入る（このカードは変わらない）。
      setMessage(t('account.googleOtherAddress', { email: a.email }));
      return;
    }
    await syncNow(a.id, a.sync_contacts);
  };

  const toggle = (service: GoogleService, on: boolean) => {
    setPending(service);
    void run(on ? 'connecting' : 'switching', async () => {
      if (!on) {
        if (google) await googleSetService(google.id, service, false);
        return;
      }
      const granted = service === 'calendar' ? google?.calendar_granted : google?.contacts_granted;
      if (google && connected && granted) {
        await googleSetService(google.id, service, true);
        await syncNow(google.id, service === 'contacts');
      } else {
        // 未連携・解除中・権限が無い → ログインし直す（もともとオンのサービスはそのまま残る）。
        await login(service === 'calendar', service === 'contacts');
      }
    });
  };

  const reconnect = () =>
    run('connecting', () =>
      // 何もオンでなかったなら、カレンダーをオンにして戻す（どちらかは要る）。
      login(!google?.sync_contacts || !!google?.sync_calendar, !!google?.sync_contacts)
    );

  const disconnect = () => {
    if (!google || !disconnecting) return;
    const { purge } = disconnecting;
    void run('disconnecting', async () => {
      const r = await googleDisconnect(google.id, purge);
      setDisconnecting(null);
      setMessage(
        purge
          ? t('settings.gcalPurged', { email: google.email })
          : t('settings.gcalDisconnected', { email: google.email })
      );
      // 許可の取り消しに失敗しても解除は済んでいる。理由を添えて知らせる。
      if (r.revoke_error) setError(t('settings.gcalRevokeFailed', { message: r.revoke_error }));
    });
  };

  const disconnectedBadge = disconnected && (
    <span
      className="rounded bg-amber-400/20 px-1.5 py-0.5 text-[10px] text-amber-200"
      title={t('settings.gcalDisconnectedHint')}
    >
      {t('settings.gcalDisconnectedBadge')}
    </span>
  );
  const lastSync = (at: string | null | undefined, done: string, never: string) =>
    at ? t(done, { when: localTime(at) }) : t(never);

  return (
    <>
      <ServiceRow
        icon={<Users size={16} />}
        label={t('account.serviceContacts')}
        hint={
          connected && google.sync_contacts
            ? lastSync(
                google.last_contacts_sync_at,
                'settings.gcontactsLastSync',
                'settings.gcontactsNeverSynced'
              )
            : t('account.authGoogle')
        }
        badge={google?.sync_contacts ? disconnectedBadge : undefined}
        checked={connected && !!google.sync_contacts}
        busy={pending === 'contacts'}
        disabled={busy !== 'idle'}
        onToggle={() => toggle('contacts', !(connected && google.sync_contacts))}
      />
      <ServiceRow
        icon={<CalendarDays size={16} />}
        label={t('account.serviceCalendar')}
        hint={
          connected && google.sync_calendar
            ? lastSync(
                google.last_calendar_sync_at,
                'settings.gcalLastSync',
                'settings.gcalNeverSynced'
              )
            : t('account.authGoogle')
        }
        badge={google?.sync_calendar ? disconnectedBadge : undefined}
        checked={connected && !!google.sync_calendar}
        busy={pending === 'calendar'}
        disabled={busy !== 'idle'}
        onToggle={() => toggle('calendar', !(connected && google.sync_calendar))}
      />

      {google && (
        <div className="flex flex-wrap items-center gap-2 pt-2">
          {disconnected ? (
            // 解除中: 同期はせず、再接続だけを出す（同じアカウントを選べば記録を使い直す）。
            <button
              onClick={() => void reconnect()}
              disabled={busy !== 'idle'}
              className="flex items-center gap-1 rounded-md bg-sky-500/70 px-2.5 py-1.5 text-xs font-medium hover:bg-sky-500 disabled:opacity-40"
            >
              <Link2 size={13} />
              {busy === 'connecting' ? t('settings.gcalConnecting') : t('settings.gcalReconnect')}
            </button>
          ) : (
            (google.sync_calendar || google.sync_contacts) && (
              <button
                onClick={() => void run('syncing', () => syncNow(google.id, true))}
                disabled={busy !== 'idle'}
                className="flex items-center gap-1 rounded-md bg-white/15 px-2.5 py-1.5 text-xs font-medium hover:bg-white/25 disabled:opacity-40"
              >
                <RefreshCw size={13} className={busy === 'syncing' ? 'animate-spin' : ''} />
                {busy === 'syncing' ? t('settings.gcalSyncing') : t('settings.gcalSyncNow')}
              </button>
            )
          )}
          <button
            // 解除中なら残る選択肢は「完全に解除」だけ。
            onClick={() => setDisconnecting({ purge: disconnected })}
            disabled={busy !== 'idle'}
            className="flex items-center gap-1 rounded-md border border-white/20 px-2.5 py-1.5 text-xs text-white/70 hover:bg-white/10 disabled:opacity-40"
          >
            <Unlink size={13} />
            {t('account.googleDisconnect')}
          </button>
        </div>
      )}

      <div ref={statusRef} className="space-y-1 empty:hidden">
        {busy === 'connecting' && (
          <p className="pt-1 text-xs text-white/60">{t('settings.gcalConnecting')}</p>
        )}
        {message && <p className="pt-1 text-xs text-emerald-300">{message}</p>}
        {error && (
          <p className="pt-1 text-xs text-red-300">{t('settings.gcalError', { message: error })}</p>
        )}
      </div>

      {google && disconnecting && (
        <GoogleDisconnectDialog
          account={google}
          purge={disconnecting.purge}
          busy={busy === 'disconnecting'}
          onPurgeChange={(purge) => setDisconnecting({ purge })}
          onConfirm={disconnect}
          onCancel={() => setDisconnecting(null)}
        />
      )}
    </>
  );
}
