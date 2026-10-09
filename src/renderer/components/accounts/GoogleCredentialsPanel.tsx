import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { GoogleCredentialsStatus } from '@bindings/GoogleCredentialsStatus';
import { googleSetCredentials } from '../../services/google';
import { isTauri } from './shared';

/**
 * Google の OAuth クライアント（Client ID / Secret）の入力。開発用の欄としてカードの外に置く
 * （docs/ACCOUNTS.md §2-4。製品として配る前にアプリへ組み込み、利用者には見せない）。
 */
export function GoogleCredentialsPanel({
  creds,
  onSaved,
}: {
  creds: GoogleCredentialsStatus | null;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [clientId, setClientId] = useState('');
  const [clientSecret, setClientSecret] = useState('');
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    if (!isTauri || busy) return;
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      await googleSetCredentials(clientId.trim(), clientSecret.trim());
      setClientId('');
      setClientSecret('');
      setMessage(t('settings.gcalSaved'));
      onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    // 設定済みなら畳んでおく（毎回見るものではない）。
    <details
      open={!creds?.configured}
      className="rounded-lg bg-black/40 p-4 text-sm backdrop-blur-sm"
    >
      <summary className="flex cursor-pointer items-center justify-between gap-3 text-white/70">
        <span>{t('account.googleCredentials')}</span>
        <span
          className={`rounded-full px-2 py-0.5 text-xs ${
            creds?.configured ? 'bg-emerald-500/20 text-emerald-200' : 'bg-white/10 text-white/50'
          }`}
        >
          {creds?.configured
            ? `${t('settings.gcalConfigured')}${creds.client_id_hint ? ` (${creds.client_id_hint})` : ''}`
            : t('settings.gcalNotConfigured')}
        </span>
      </summary>
      <div className="mt-3 space-y-3">
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
          onClick={() => void save()}
          disabled={busy || !clientId.trim() || !clientSecret.trim()}
          className="rounded-md bg-white/15 px-3 py-1.5 text-sm font-medium hover:bg-white/25 disabled:opacity-40"
        >
          {busy ? '…' : t('settings.gcalSave')}
        </button>
        <p className="text-xs text-white/40">{t('settings.gcalTestUserNote')}</p>
        {message && <p className="text-xs text-emerald-300">{message}</p>}
        {error && (
          <p className="text-xs text-red-300">{t('settings.gcalError', { message: error })}</p>
        )}
      </div>
    </details>
  );
}
