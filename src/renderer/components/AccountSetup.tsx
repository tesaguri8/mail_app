import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, Minus, Plus } from 'lucide-react';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { ServerAccountSummary } from '@bindings/ServerAccountSummary';
import type { SignatureSummary } from '@bindings/SignatureSummary';
import {
  accountAdd,
  accountAutoconfig,
  accountCheck,
  accountDelete,
  accountTestLogin,
  accountUpdate,
  serverAccountList,
} from '../services/accounts';
import { signatureList } from '../services/signatures';
import {
  accountSetStorageLimit,
  accountSetSyncWindow,
  accountStorageInfo,
  mailResync,
  storageOptimize,
} from '../services/mail';
import type { StorageInfo } from '@bindings/StorageInfo';

type ConnState = { state: 'checking' | 'ok' | 'error'; msg?: string };

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const WINDOWS = ['n50', 'n200', '3d', '7d', '30d', '3m', '6m', 'all'] as const;

const GB = 1024 * 1024 * 1024;
const LIMIT_GB = [1, 2, 5, 10, 20, 50] as const;

function formatBytes(b: number): string {
  if (b < 1024 * 1024) return `${Math.round(b / 1024)} KB`;
  if (b < GB) return `${(b / 1024 / 1024).toFixed(0)} MB`;
  return `${(b / GB).toFixed(2)} GB`;
}

const inputCls =
  'w-full rounded-md bg-white/10 px-3 py-2 text-sm text-white placeholder-white/40 outline-none focus:bg-white/20';
const btnCls = 'rounded-md bg-white/15 px-3 py-2 text-sm hover:bg-white/25 disabled:opacity-40';

export function AccountSetup({
  accounts,
  onChanged,
}: {
  accounts: AccountSummary[];
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [servers, setServers] = useState<ServerAccountSummary[]>([]);
  const [conn, setConn] = useState<Record<number, ConnState>>({});
  const [adding, setAdding] = useState(false);

  // 既存アカウントのインライン編集（差出人名・既定署名）
  const [signatures, setSignatures] = useState<SignatureSummary[]>([]);
  const [editing, setEditing] = useState<number | null>(null);
  const [editName, setEditName] = useState('');
  const [editSig, setEditSig] = useState<number | null>(null);
  const [editWindow, setEditWindow] = useState('6m');
  const [editStatus, setEditStatus] = useState('');
  // ストレージ（容量）状態
  const [storage, setStorage] = useState<StorageInfo | null>(null);
  const [storageBusy, setStorageBusy] = useState(false);
  const [storageMsg, setStorageMsg] = useState('');
  const [resyncing, setResyncing] = useState(false);

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

  const checkConn = (id: number) => {
    setConn((c) => ({ ...c, [id]: { state: 'checking' } }));
    accountCheck(id)
      .then(() => setConn((c) => ({ ...c, [id]: { state: 'ok' } })))
      .catch((e) => setConn((c) => ({ ...c, [id]: { state: 'error', msg: String(e) } })));
  };

  // サーバー設定一覧の取得
  const loadServers = () => {
    if (!isTauri) return;
    serverAccountList()
      .then(setServers)
      .catch(() => undefined);
  };
  useEffect(loadServers, []);

  // 署名一覧（編集ドロップダウン用）
  useEffect(() => {
    if (!isTauri) return;
    signatureList()
      .then(setSignatures)
      .catch(() => undefined);
  }, []);

  const toggleEdit = (a: AccountSummary) => {
    if (editing === a.id) {
      setEditing(null);
      return;
    }
    setEditing(a.id);
    setEditName(a.display_name ?? '');
    setEditSig(a.signature_id ?? null);
    setEditWindow(a.sync_window ?? '6m');
    setEditStatus('');
    setStorage(null);
    setStorageMsg('');
    loadStorage(a.id);
  };

  const loadStorage = (id: number) => {
    if (!isTauri) return;
    accountStorageInfo(id)
      .then(setStorage)
      .catch(() => undefined);
  };

  const changeLimit = async (id: number, gb: number) => {
    setStorage((s) => (s ? { ...s, limit_bytes: gb * GB } : s));
    try {
      await accountSetStorageLimit(id, gb * GB);
      await storageOptimize(id); // 新上限で超過分があれば即整理
      loadStorage(id);
    } catch (e) {
      setStorageMsg(String(e));
    }
  };

  const optimize = async (id: number) => {
    setStorageBusy(true);
    setStorageMsg('');
    try {
      const r = await storageOptimize(id);
      setStorageMsg(t('storage.optimized', { count: r.evicted, size: formatBytes(r.freed_bytes) }));
      loadStorage(id);
    } catch (e) {
      setStorageMsg(String(e));
    } finally {
      setStorageBusy(false);
    }
  };

  const resync = async (id: number) => {
    setStorageBusy(true);
    setResyncing(true);
    setStorageMsg(t('storage.resyncing'));
    try {
      const r = await mailResync(id);
      setStorageMsg(
        t('storage.resynced', { fetched: r.fetched, stored: r.stored, backfilled: r.backfilled }),
      );
      onChanged();
      loadStorage(id);
    } catch (e) {
      setStorageMsg(String(e));
    } finally {
      setStorageBusy(false);
      setResyncing(false);
    }
  };

  const saveEdit = async (id: number) => {
    try {
      await accountUpdate(id, editName.trim() || null, editSig);
      setEditStatus('✓ ' + t('account.saved'));
      onChanged();
    } catch (e) {
      setEditStatus('✕ ' + String(e));
    }
  };

  const changeWindow = async (id: number, w: string) => {
    setEditWindow(w);
    try {
      await accountSetSyncWindow(id, w);
      onChanged();
    } catch {
      /* noop */
    }
  };

  // アカウントが変わるたびに各接続状態をチェック
  useEffect(() => {
    if (!isTauri) return;
    accounts.forEach((a) => checkConn(a.id));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accounts]);

  const onPickServer = (id: string) => {
    const s = servers.find((x) => String(x.id) === id);
    if (!s) return;
    setImapHost(s.imap_host);
    setImapPort(s.imap_port);
    setSmtpHost(s.smtp_host);
    setSmtpPort(s.smtp_port);
    setUsername(s.username);
    setUsernameEdited(true);
    setNote('');
  };

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
      // 本物の IMAP ログインで認証まで検証する
      await accountTestLogin(imapHost, imapPort, username || email, password);
      setStatus('✓ ' + t('account.testOk'));
    } catch (e) {
      setStatus('✕ ' + t('account.testFail') + ': ' + String(e));
    } finally {
      setBusy(false);
    }
  };

  const onDelete = async (id: number) => {
    try {
      await accountDelete(id);
      onChanged();
    } catch {
      /* noop */
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
      onChanged();
      loadServers();
    } catch (e) {
      setStatus('✕ ' + String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="max-w-[460px] text-left">
      {!adding && accounts.length === 0 && (
        <p className="text-sm text-white/60">{t('account.none')}</p>
      )}

      {!adding && accounts.length > 0 && (
        <ul className="space-y-2">
          {accounts.map((a) => (
            <li key={a.id} className="overflow-hidden rounded-md bg-white/10 text-sm">
              <div className="flex items-center justify-between gap-2 px-3 py-2">
                <button
                  onClick={() => toggleEdit(a)}
                  className="flex min-w-0 flex-1 items-center gap-2 text-left"
                >
                  <span
                    onClick={(e) => {
                      e.stopPropagation();
                      checkConn(a.id);
                    }}
                    title={conn[a.id]?.msg ?? conn[a.id]?.state ?? ''}
                    className={`h-2.5 w-2.5 shrink-0 rounded-full ${
                      conn[a.id]?.state === 'ok'
                        ? 'bg-emerald-400'
                        : conn[a.id]?.state === 'error'
                          ? 'bg-red-400'
                          : 'animate-pulse bg-amber-300'
                    }`}
                  />
                  <span className="min-w-0">
                    <span className="block truncate font-medium">
                      {a.display_name ? `${a.display_name} <${a.email}>` : a.email}
                    </span>
                    <span className="block truncate text-xs text-white/50">
                      IMAP {a.imap_host} · SMTP {a.smtp_host}
                    </span>
                  </span>
                  <ChevronDown
                    size={16}
                    className={`ml-auto shrink-0 text-white/40 transition-transform ${
                      editing === a.id ? 'rotate-180' : ''
                    }`}
                  />
                </button>
                <button
                  className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/60 hover:border-red-400/60 hover:bg-red-500/30 hover:text-white"
                  title={t('account.delete')}
                  aria-label={t('account.delete')}
                  onClick={() => onDelete(a.id)}
                >
                  <Minus size={18} />
                </button>
              </div>

              {editing === a.id && (
                <div className="space-y-3 border-t border-white/10 bg-black/15 px-3 py-3">
                  <label className="block">
                    <span className="mb-1 block text-xs text-white/55">
                      {t('account.displayName')}
                    </span>
                    <input
                      className={inputCls}
                      placeholder={t('account.displayNamePlaceholder')}
                      value={editName}
                      onChange={(e) => setEditName(e.target.value)}
                    />
                  </label>
                  <label className="block">
                    <span className="mb-1 block text-xs text-white/55">
                      {t('account.signature')}
                    </span>
                    <select
                      className={inputCls}
                      value={editSig ?? ''}
                      onChange={(e) =>
                        setEditSig(e.target.value === '' ? null : Number(e.target.value))
                      }
                    >
                      <option value="" className="text-black">
                        {t('account.signatureNone')}
                      </option>
                      {signatures.map((s) => (
                        <option key={s.id} value={s.id} className="text-black">
                          {s.name || t('signature.untitled')}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="block">
                    <span className="mb-1 block text-xs text-white/55">{t('mailbox.window')}</span>
                    <select
                      className={inputCls}
                      value={editWindow}
                      onChange={(e) => changeWindow(a.id, e.target.value)}
                    >
                      {WINDOWS.map((w) => (
                        <option key={w} value={w} className="text-black">
                          {t(`mailbox.w_${w}`)}
                        </option>
                      ))}
                    </select>
                  </label>

                  {/* ストレージ（容量上限とエビクション） */}
                  <div className="rounded-md border border-white/10 p-3">
                    <div className="mb-1 flex items-center justify-between">
                      <span className="text-xs text-white/55">{t('storage.title')}</span>
                      <span className="text-xs text-white/70">
                        {storage
                          ? `${formatBytes(storage.used_bytes)} / ${formatBytes(storage.limit_bytes)}`
                          : '—'}
                      </span>
                    </div>
                    {storage && (
                      <div className="mb-2 h-1.5 overflow-hidden rounded-full bg-white/10">
                        <div
                          className="h-full rounded-full bg-sky-400"
                          style={{
                            width: `${Math.min(100, storage.limit_bytes > 0 ? (storage.used_bytes / storage.limit_bytes) * 100 : 0)}%`,
                          }}
                        />
                      </div>
                    )}
                    <label className="mb-2 block">
                      <span className="mb-1 block text-xs text-white/55">{t('storage.limit')}</span>
                      <select
                        className={inputCls}
                        value={storage ? Math.round(storage.limit_bytes / GB) : 2}
                        onChange={(e) => changeLimit(a.id, Number(e.target.value))}
                      >
                        {LIMIT_GB.map((g) => (
                          <option key={g} value={g} className="text-black">
                            {g} GB
                          </option>
                        ))}
                      </select>
                    </label>
                    <div className="flex flex-wrap items-center gap-2">
                      <button
                        className={btnCls}
                        disabled={storageBusy}
                        onClick={() => optimize(a.id)}
                      >
                        {t('storage.optimize')}
                      </button>
                      <button
                        className={btnCls}
                        disabled={storageBusy}
                        onClick={() => resync(a.id)}
                        title={t('storage.resyncHint')}
                      >
                        {t('storage.resync')}
                      </button>
                      {storageMsg && <span className="text-xs text-white/70">{storageMsg}</span>}
                    </div>
                    {resyncing && (
                      <div className="mt-2 h-1 w-full overflow-hidden rounded-full bg-white/10">
                        <div className="h-full w-1/3 animate-pulse rounded-full bg-sky-400" />
                      </div>
                    )}
                  </div>

                  <div className="flex items-center gap-3">
                    <button className={btnCls} onClick={() => saveEdit(a.id)}>
                      {t('account.save')}
                    </button>
                    {editStatus && <span className="text-xs text-white/70">{editStatus}</span>}
                  </div>
                </div>
              )}
            </li>
          ))}
        </ul>
      )}

      {!adding && (
        <button
          onClick={() => setAdding(true)}
          title={t('account.addAccount')}
          aria-label={t('account.addAccount')}
          className="mt-3 flex h-9 w-9 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white"
        >
          <Plus size={18} />
        </button>
      )}

      {adding && (
        <div className="space-y-2">
          <div className="text-xs font-semibold text-white/60">{t('account.appAccount')}</div>
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
          <div className="pt-1 text-xs font-semibold text-white/60">
            {t('account.serverAccount')}
          </div>
          {servers.length > 0 && (
            <select
              className={inputCls}
              defaultValue=""
              onChange={(e) => onPickServer(e.target.value)}
            >
              <option value="" className="text-black">
                {t('account.useExistingServer')}
              </option>
              {servers.map((s) => (
                <option key={s.id} value={s.id} className="text-black">
                  {s.imap_host}（{s.username}）
                </option>
              ))}
            </select>
          )}
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
            <button
              className={btnCls}
              onClick={onTest}
              disabled={busy || !imapHost || !password}
            >
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
