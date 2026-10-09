import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountProvider } from '@bindings/AccountProvider';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { ServerAccountSummary } from '@bindings/ServerAccountSummary';
import { accountAdd, accountAutoconfig, accountTestLogin } from '../../services/accounts';
import { btnCls, inputCls, isTauri } from './shared';

/**
 * メール（IMAP / SMTP）の接続の入力。追加の流れと、メールを使っていないカードで「メール」を
 * オンにするときに使う。アドレスはカード（または追加の流れ）で決まっているので、ここでは変えない。
 *
 * サーバー設定は開いたときに自動設定で埋める（Google・iCloud は App 用パスワードだけ入れればよい）。
 * 既存のサーバー設定（ほかのアドレスと共有しているもの）を選んで使うこともできる。
 */
export function MailAccountForm({
  email,
  provider,
  servers,
  onAdded,
  onCancel,
}: {
  email: string;
  provider: AccountProvider;
  servers: ServerAccountSummary[];
  onAdded: (account: AccountSummary) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [username, setUsername] = useState(email);
  const [password, setPassword] = useState('');
  const [imapHost, setImapHost] = useState('');
  const [imapPort, setImapPort] = useState(993);
  const [smtpHost, setSmtpHost] = useState('');
  const [smtpPort, setSmtpPort] = useState(587);
  const [note, setNote] = useState('');
  const [status, setStatus] = useState('');
  const [busy, setBusy] = useState(false);

  // 開いたら自動設定でサーバーを埋める（今の autoconfig）。
  useEffect(() => {
    if (!isTauri || !email) return;
    let alive = true;
    accountAutoconfig(email)
      .then((r) => {
        if (!alive) return;
        setImapHost(r.imap_host);
        setImapPort(r.imap_port);
        setSmtpHost(r.smtp_host);
        setSmtpPort(r.smtp_port);
        setNote(r.note ?? '');
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [email]);

  const pickServer = (id: string) => {
    const s = servers.find((x) => String(x.id) === id);
    if (!s) return;
    setImapHost(s.imap_host);
    setImapPort(s.imap_port);
    setSmtpHost(s.smtp_host);
    setSmtpPort(s.smtp_port);
    setUsername(s.username);
    setNote('');
  };

  const test = async () => {
    setBusy(true);
    setStatus(t('account.testing'));
    try {
      // 本物の IMAP ログインで認証まで検証する
      await accountTestLogin(imapHost, imapPort, username || email, password);
      setStatus('✓ ' + t('account.testOk'));
    } catch (e) {
      setStatus('✕ ' + t('account.testFail') + ': ' + String(e));
    } finally {
      setBusy(false);
    }
  };

  const add = async () => {
    setBusy(true);
    setStatus(t('account.adding'));
    try {
      const account = await accountAdd(
        {
          email,
          display_name: null,
          username: username || email,
          imap_host: imapHost,
          imap_port: imapPort,
          smtp_host: smtpHost,
          smtp_port: smtpPort,
          provider,
        },
        password
      );
      setStatus('');
      onAdded(account);
    } catch (e) {
      setStatus('✕ ' + String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-2">
      <p className="text-xs text-white/50">
        {provider === 'imap' ? t('account.mailPasswordHint') : t('account.appPasswordHint')}
      </p>
      <input
        className={inputCls}
        type="password"
        placeholder={t('account.password')}
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        autoFocus
      />
      {/* サーバー設定は自動設定で埋まる。違うときだけ開いて直す。 */}
      <details className="rounded-md border border-white/10 px-3 py-2">
        <summary className="cursor-pointer text-xs text-white/55">
          {t('account.serverAccount')}
          {imapHost && <span className="ml-2 text-white/35">{imapHost}</span>}
        </summary>
        <div className="mt-2 space-y-2">
          {servers.length > 0 && (
            <select
              className={inputCls}
              defaultValue=""
              onChange={(e) => pickServer(e.target.value)}
            >
              <option value="">{t('account.useExistingServer')}</option>
              {servers.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.imap_host}（{s.username}）
                </option>
              ))}
            </select>
          )}
          <input
            className={inputCls}
            placeholder={t('account.username')}
            value={username}
            onChange={(e) => setUsername(e.target.value)}
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
        </div>
      </details>

      {note && <p className="text-xs text-amber-200/80">{note}</p>}
      {status && <p className="text-xs text-white/70">{status}</p>}

      <div className="flex gap-2 pt-1">
        <button
          className={btnCls}
          onClick={() => void test()}
          disabled={busy || !imapHost || !password}
        >
          {t('account.test')}
        </button>
        <button
          className={btnCls}
          onClick={() => void add()}
          disabled={busy || !password || !imapHost || !smtpHost}
        >
          {t('account.add')}
        </button>
        <button className={btnCls} onClick={onCancel} disabled={busy}>
          {t('account.cancel')}
        </button>
      </div>
    </div>
  );
}
