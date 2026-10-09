import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link2 } from 'lucide-react';
import type { AccountProfile } from '@bindings/AccountProfile';
import type { AccountProvider } from '@bindings/AccountProvider';
import type { GoogleCredentialsStatus } from '@bindings/GoogleCredentialsStatus';
import type { ServerAccountSummary } from '@bindings/ServerAccountSummary';
import { googleConnect, googleSync } from '../../services/google';
import { summarizeGoogleSync } from '../../utils/googleSyncSummary';
import { CALENDAR_SYNCED_EVENT, CONTACTS_SYNCED_EVENT } from '../../hooks/useAutoSync';
import { useSync } from '../SyncProvider';
import { MailAccountForm } from './MailAccountForm';
import { ProviderMark } from './AccountCard';
import { btnCls, inputCls, sameAddress } from './shared';

type Step = 'provider' | 'address' | 'services' | 'mail' | 'google' | 'done';
type Services = { mail: boolean; contacts: boolean; calendar: boolean };

const PROVIDERS: AccountProvider[] = ['google', 'icloud', 'imap'];

/** 提供元ごとに選べるサービス（iCloud の連絡先・カレンダーは後続）。 */
const available = (p: AccountProvider): Services => ({
  mail: true,
  contacts: p === 'google',
  calendar: p === 'google',
});

/**
 * アカウントの追加（docs/ACCOUNTS.md §2-2）。
 * 提供元 → アドレス → 使うサービス → 認証（App 用パスワード → Google はログイン）→ 最初の同期。
 */
export function AddAccountFlow({
  profiles,
  servers,
  creds,
  onChanged,
  onClose,
}: {
  profiles: AccountProfile[];
  servers: ServerAccountSummary[];
  creds: GoogleCredentialsStatus | null;
  onChanged: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const sync = useSync();
  const [step, setStep] = useState<Step>('provider');
  const [provider, setProvider] = useState<AccountProvider>('google');
  const [email, setEmail] = useState('');
  const [services, setServices] = useState<Services>(available('google'));
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string[]>([]);
  const [error, setError] = useState('');

  const wantsGoogle = provider === 'google' && (services.contacts || services.calendar);
  const existing = profiles.find((p) => sameAddress(p.email, email));

  const pickProvider = (p: AccountProvider) => {
    setProvider(p);
    setServices(available(p));
    setStep('address');
  };

  const confirmAddress = () => {
    setError('');
    // 同じアドレスのカードがあれば、追加ではなくそのカードのスイッチから足してもらう。
    if (existing) {
      setError(t('account.addExists', { email: existing.email }));
      return;
    }
    // その他（IMAP）はメールだけなので、サービスの選択を飛ばす。
    setStep(provider === 'imap' ? 'mail' : 'services');
  };

  const afterMail = () => setStep(wantsGoogle ? 'google' : 'done');

  const loginGoogle = async () => {
    if (!creds?.configured) {
      setError(t('settings.gcalNeedCredentials'));
      return;
    }
    setBusy(true);
    setError('');
    try {
      const a = await googleConnect(services.calendar, services.contacts, email);
      onChanged();
      const lines = [t('account.addGoogleDone', { email: a.email })];
      if (!sameAddress(a.email, email))
        lines.push(t('account.googleOtherAddress', { email: a.email }));
      // 最初の同期。連絡先は今の「反映」の規則（高確信だけつなぎ、迷ったら重複整理）で住所録へ。
      const r = await googleSync(a.id, a.sync_contacts);
      const summary = summarizeGoogleSync(r, t);
      if (summary) lines.push(summary);
      [r.calendar_error, r.contacts_error].forEach((e) => e && lines.push(`✕ ${e}`));
      window.dispatchEvent(new Event(CALENDAR_SYNCED_EVENT));
      if (a.sync_contacts) window.dispatchEvent(new Event(CONTACTS_SYNCED_EVENT));
      setDone((d) => [...d, ...lines]);
      setStep('done');
    } catch (e) {
      setError('✕ ' + String(e));
    } finally {
      setBusy(false);
      onChanged();
    }
  };

  const serviceKeys: (keyof Services)[] = ['mail', 'contacts', 'calendar'];

  return (
    <div className="space-y-3 rounded-lg bg-black/40 p-4 text-sm backdrop-blur-sm">
      <div className="text-sm font-semibold text-white/85">{t('account.addAccount')}</div>

      {step === 'provider' && (
        <div className="space-y-2">
          <p className="text-xs text-white/50">{t('account.addPickProvider')}</p>
          {PROVIDERS.map((p) => (
            <button
              key={p}
              onClick={() => pickProvider(p)}
              className="flex w-full items-center gap-3 rounded-md bg-white/10 px-3 py-2.5 text-left hover:bg-white/15"
            >
              <ProviderMark provider={p} />
              <span>
                <span className="block text-white/90">{t(`account.provider.${p}`)}</span>
                <span className="block text-xs text-white/45">
                  {t(`account.providerHint.${p}`)}
                </span>
              </span>
            </button>
          ))}
        </div>
      )}

      {step === 'address' && (
        <div className="space-y-2">
          <div className="flex items-center gap-2 text-xs text-white/55">
            <ProviderMark provider={provider} />
            {t(`account.provider.${provider}`)}
          </div>
          <input
            className={inputCls}
            type="email"
            autoFocus
            placeholder={t('account.email')}
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && email.includes('@') && confirmAddress()}
          />
          <div className="flex gap-2">
            <button className={btnCls} disabled={!email.includes('@')} onClick={confirmAddress}>
              {t('account.next')}
            </button>
            <button className={btnCls} onClick={() => setStep('provider')}>
              {t('account.back')}
            </button>
          </div>
        </div>
      )}

      {step === 'services' && (
        <div className="space-y-2">
          <p className="text-xs text-white/50">{t('account.addPickServices', { email })}</p>
          {serviceKeys.map((k) => {
            const ok = available(provider)[k];
            return (
              <label
                key={k}
                className={`flex items-start gap-2 rounded-md px-2 py-1.5 ${ok ? 'cursor-pointer hover:bg-white/5' : 'opacity-40'}`}
              >
                <input
                  type="checkbox"
                  className="mt-1"
                  checked={services[k]}
                  disabled={!ok}
                  onChange={(e) => setServices({ ...services, [k]: e.target.checked })}
                />
                <span>
                  <span className="block text-white/90">
                    {t(`account.service${k[0].toUpperCase()}${k.slice(1)}`)}
                  </span>
                  <span className="block text-xs text-white/45">
                    {!ok
                      ? t('account.comingSoon')
                      : k === 'mail'
                        ? t('account.authAppPassword')
                        : t('account.authGoogle')}
                  </span>
                </span>
              </label>
            );
          })}
          <div className="flex gap-2">
            <button
              className={btnCls}
              disabled={!services.mail && !wantsGoogle}
              onClick={() => (services.mail ? setStep('mail') : setStep('google'))}
            >
              {t('account.next')}
            </button>
            <button className={btnCls} onClick={() => setStep('address')}>
              {t('account.back')}
            </button>
          </div>
        </div>
      )}

      {step === 'mail' && (
        <MailAccountForm
          email={email}
          provider={provider}
          servers={servers}
          onAdded={(a) => {
            onChanged();
            // 最初の同期（バックグラウンド。進捗は共通の表示）。
            sync.start(a.id, a.email, 'sync');
            setDone((d) => [...d, t('account.addMailDone', { email: a.email })]);
            afterMail();
          }}
          onCancel={onClose}
        />
      )}

      {step === 'google' && (
        <div className="space-y-2">
          {done.map((line) => (
            <p key={line} className="text-xs text-emerald-300">
              {line}
            </p>
          ))}
          <p className="text-xs text-white/50">{t('account.addGoogleLogin')}</p>
          <div className="flex gap-2">
            <button
              onClick={() => void loginGoogle()}
              disabled={busy}
              className="flex items-center gap-1.5 rounded-md bg-sky-500/90 px-3 py-2 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-40"
            >
              <Link2 size={15} />
              {busy ? t('settings.gcalConnecting') : t('account.googleLogin')}
            </button>
            <button className={btnCls} disabled={busy} onClick={() => setStep('done')}>
              {t('account.later')}
            </button>
          </div>
        </div>
      )}

      {step === 'done' && (
        <div className="space-y-2">
          {done.map((line) => (
            <p key={line} className="text-xs text-emerald-300">
              {line}
            </p>
          ))}
          <button className={btnCls} onClick={onClose}>
            {t('account.close')}
          </button>
        </div>
      )}

      {error && <p className="text-xs text-red-300">{error}</p>}

      {step !== 'mail' && step !== 'done' && (
        <button className="text-xs text-white/40 hover:text-white/70" onClick={onClose}>
          {t('account.cancel')}
        </button>
      )}
    </div>
  );
}
