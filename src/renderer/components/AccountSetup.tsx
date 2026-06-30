import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountSummary } from '@bindings/AccountSummary';
import {
  accountAdd,
  accountAutoconfig,
  accountList,
  accountTestConnection,
} from '../services/accounts';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const inputCls =
  'w-full rounded-md bg-white/10 px-3 py-2 text-sm text-white placeholder-white/40 outline-none focus:bg-white/20';
const btnCls = 'rounded-md bg-white/15 px-3 py-2 text-sm hover:bg-white/25 disabled:opacity-40';

export function AccountSetup() {
  const { t } = useTranslation();
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  const [adding, setAdding] = useState(false);

  // form state
  const [email, setEmail] = useState('');
  const [username, setUsername] = useState('');
  const [usernameEdited, setUsernameEdited] = useState(false);
  const [password, setPassword] = useState('');
  const [imapHost, setImapHost] = useState('');
  const [imapPort, setImapPort] = useState(993);
  const [smtpHost, setSmtpHost] = useState('');
  const [smtpPort, setSmtpPort] = useState(587);
  const [note, setNote] = useState('');
  const [status, setStatus] = useState('');
  const [busy, setBusy] = useState(false);

  const refresh = () => {
    if (!isTauri) return;
    accountList()
      .then(setAccounts)
      .catch(() => undefined);
  };
  useEffect(refresh, []);

  const onAutoconfig = async () => {
    if (!email) {
      setStatus(t('account.needEmailFirst'));
      return;
    }
    const r = await accountAutoconfig(email);
    setImapHost(r.imap_host);
    setImapPort(r.imap_port);
    setSmtpHost(r.smtp_host);
    setSmtpPort(r.smtp_port);
    setNote(r.note ?? '');
    setStatus('');
  };

  const onTest = async () => {
    setBusy(true);
    setStatus(t('account.testing'));
    try {
      await accountTestConnection(imapHost, imapPort);
      setStatus('✓ ' + t('account.testOk'));
    } catch (e) {
      setStatus('✕ ' + t('account.testFail') + ': ' + String(e));
    } finally {
      setBusy(false);
    }
  };

  const onAdd = async () => {
    setBusy(true);
    setStatus(t('account.adding'));
    try {
      await accountAdd(
        {
          email,
          display_name: null,
          username: username || email,
          imap_host: imapHost,
          imap_port: imapPort,
          smtp_host: smtpHost,
          smtp_port: smtpPort,
        },
        password
      );
      setAdding(false);
      setEmail('');
      setUsername('');
      setUsernameEdited(false);
      setPassword('');
      setNote('');
      setStatus('');
      refresh();
    } catch (e) {
      setStatus('✕ ' + String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="w-[440px] max-w-full rounded-xl bg-black/25 p-5 text-left backdrop-blur">
      <div className="mb-3 flex items-center justify-between">
        <h2 className="text-sm font-semibold text-white/90">{t('account.title')}</h2>
        {!adding && (
          <button className={btnCls} onClick={() => setAdding(true)}>
            {t('account.addAccount')}
          </button>
        )}
      </div>

      {!adding && accounts.length === 0 && (
        <p className="text-sm text-white/60">{t('account.none')}</p>
      )}

      {!adding && accounts.length > 0 && (
        <ul className="space-y-2">
          {accounts.map((a) => (
            <li key={a.id} className="rounded-md bg-white/10 px-3 py-2 text-sm">
              <div className="font-medium">{a.email}</div>
              <div className="text-xs text-white/50">
                IMAP {a.imap_host} · SMTP {a.smtp_host}
              </div>
            </li>
          ))}
        </ul>
      )}

      {adding && (
        <div className="space-y-2">
          <div className="flex gap-2">
            <input
              className={inputCls}
              type="email"
              placeholder={t('account.email')}
              value={email}
              onChange={(e) => {
                setEmail(e.target.value);
                if (!usernameEdited) setUsername(e.target.value);
              }}
            />
            <button className={btnCls} onClick={onAutoconfig}>
              {t('account.autoconfig')}
            </button>
          </div>
          <input
            className={inputCls}
            placeholder={t('account.username')}
            value={username}
            onChange={(e) => {
              setUsername(e.target.value);
              setUsernameEdited(true);
            }}
          />
          <input
            className={inputCls}
            type="password"
            placeholder={t('account.password')}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          <div className="flex gap-2">
            <input
              className={inputCls}
              placeholder={t('account.imapHost')}
              value={imapHost}
              onChange={(e) => setImapHost(e.target.value)}
            />
            <input
              className="w-24 rounded-md bg-white/10 px-3 py-2 text-sm outline-none focus:bg-white/20"
              type="number"
              value={imapPort}
              onChange={(e) => setImapPort(Number(e.target.value))}
            />
          </div>
          <div className="flex gap-2">
            <input
              className={inputCls}
              placeholder={t('account.smtpHost')}
              value={smtpHost}
              onChange={(e) => setSmtpHost(e.target.value)}
            />
            <input
              className="w-24 rounded-md bg-white/10 px-3 py-2 text-sm outline-none focus:bg-white/20"
              type="number"
              value={smtpPort}
              onChange={(e) => setSmtpPort(Number(e.target.value))}
            />
          </div>

          {note && <p className="text-xs text-amber-200/80">{note}</p>}
          {status && <p className="text-xs text-white/70">{status}</p>}

          <div className="flex gap-2 pt-1">
            <button className={btnCls} onClick={onTest} disabled={busy || !imapHost}>
              {t('account.test')}
            </button>
            <button
              className={btnCls}
              onClick={onAdd}
              disabled={busy || !email || !password || !imapHost || !smtpHost}
            >
              {t('account.add')}
            </button>
            <button className={btnCls} onClick={() => setAdding(false)} disabled={busy}>
              {t('account.cancel')}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
