import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailSummary } from '@bindings/MailSummary';
import type { MailDetail } from '@bindings/MailDetail';
import { accountSetSyncWindow, mailGet, mailList, mailSync } from '../services/mail';
import { MailView } from './MailView';

const WINDOWS = ['n50', 'n200', '3d', '7d', '30d', '3m', '6m', 'all'] as const;

const btnCls = 'rounded-md bg-white/15 px-3 py-1.5 text-sm hover:bg-white/25 disabled:opacity-40';

function formatDate(d: string | null): string {
  if (!d) return '';
  const dt = new Date(d);
  return isNaN(dt.getTime()) ? d : dt.toLocaleString();
}

export function MailList({ accounts }: { accounts: AccountSummary[] }) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<number | null>(null);
  const [mails, setMails] = useState<MailSummary[]>([]);
  const [syncing, setSyncing] = useState(false);
  const [status, setStatus] = useState('');
  const [opened, setOpened] = useState<MailDetail | null>(null);
  const [windowSel, setWindowSel] = useState('6m');

  useEffect(() => {
    if (accounts.length > 0) {
      setSelected((prev) => (prev != null && accounts.some((a) => a.id === prev) ? prev : accounts[0].id));
    } else {
      setSelected(null);
    }
  }, [accounts]);

  const loadMails = (id: number) => mailList(id, 100).then(setMails).catch(() => undefined);

  useEffect(() => {
    if (selected != null) loadMails(selected);
  }, [selected]);

  const onSync = async () => {
    if (selected == null) return;
    setSyncing(true);
    setStatus(t('mailbox.syncing'));
    try {
      const r = await mailSync(selected);
      setStatus(t('mailbox.result', { fetched: r.fetched, stored: r.stored }));
      await loadMails(selected);
    } catch (e) {
      setStatus('✕ ' + String(e));
    } finally {
      setSyncing(false);
    }
  };

  const current = accounts.find((a) => a.id === selected);

  useEffect(() => {
    setWindowSel(current?.sync_window ?? '6m');
  }, [current?.sync_window]);

  const onChangeWindow = async (w: string) => {
    if (selected == null) return;
    setWindowSel(w);
    try {
      await accountSetSyncWindow(selected, w);
    } catch {
      /* noop */
    }
  };

  const openMail = async (id: number) => {
    try {
      const d = await mailGet(id);
      setOpened(d);
      setMails((prev) => prev.map((m) => (m.id === id ? { ...m, is_read: true } : m)));
    } catch {
      /* noop */
    }
  };

  if (accounts.length === 0) return null;

  return (
    <div className="mt-4 w-[560px] max-w-full rounded-xl bg-black/25 p-4 text-left backdrop-blur">
      <div className="mb-3 flex items-center gap-2">
        <h2 className="text-sm font-semibold text-white/90">{t('mailbox.title')}</h2>
        <select
          className="rounded-md bg-white/10 px-2 py-1 text-xs outline-none"
          value={selected ?? ''}
          onChange={(e) => setSelected(Number(e.target.value))}
        >
          {accounts.map((a) => (
            <option key={a.id} value={a.id} className="text-black">
              {a.email}
            </option>
          ))}
        </select>
        <select
          className="rounded-md bg-white/10 px-2 py-1 text-xs outline-none"
          title={t('mailbox.window')}
          value={windowSel}
          onChange={(e) => onChangeWindow(e.target.value)}
        >
          {WINDOWS.map((w) => (
            <option key={w} value={w} className="text-black">
              {t(`mailbox.w_${w}`)}
            </option>
          ))}
        </select>
        <button className={btnCls} onClick={onSync} disabled={syncing || selected == null}>
          {syncing ? t('mailbox.syncing') : t('mailbox.sync')}
        </button>
        {status && <span className="text-xs text-white/60">{status}</span>}
      </div>

      {mails.length === 0 ? (
        <p className="text-sm text-white/50">{t('mailbox.empty')}</p>
      ) : (
        <ul className="max-h-72 space-y-1 overflow-y-auto pr-1">
          {mails.map((m) => (
            <li
              key={m.id}
              onClick={() => openMail(m.id)}
              className="cursor-pointer rounded-md bg-white/5 px-3 py-2 hover:bg-white/10"
            >
              <div className="flex items-baseline justify-between gap-2">
                <span className="truncate text-sm font-medium">
                  {!m.is_read && <span className="mr-1 text-sky-300">●</span>}
                  {m.from_address ?? '(no sender)'}
                </span>
                <span className="shrink-0 text-[10px] text-white/40">{formatDate(m.date)}</span>
              </div>
              <div className="truncate text-sm text-white/80">
                {m.subject ?? '(no subject)'} {m.has_attachments && '📎'}
              </div>
              {m.preview && <div className="truncate text-xs text-white/40">{m.preview}</div>}
            </li>
          ))}
        </ul>
      )}

      {opened && <MailView detail={opened} onClose={() => setOpened(null)} />}
    </div>
  );
}
