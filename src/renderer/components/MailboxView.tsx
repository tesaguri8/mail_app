import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailSummary } from '@bindings/MailSummary';
import type { MailDetail } from '@bindings/MailDetail';
import { accountSetSyncWindow, mailGet, mailList, mailSync } from '../services/mail';
import { MailBody } from './MailBody';

const WINDOWS = ['n50', 'n200', '3d', '7d', '30d', '3m', '6m', 'all'] as const;
const btnCls = 'rounded-md bg-white/15 px-3 py-1.5 text-sm hover:bg-white/25 disabled:opacity-40';

function formatDate(d: string | null): string {
  if (!d) return '';
  const dt = new Date(d);
  return isNaN(dt.getTime()) ? d : dt.toLocaleString();
}

/**
 * メールモード: 全幅。リスト＋本文。レイアウトは左右/上下を切替可能。
 */
export function MailboxView({
  accounts,
  initialAccountId,
  initialMailId,
}: {
  accounts: AccountSummary[];
  initialAccountId: number | null;
  initialMailId: number | null;
}) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<number | null>(
    initialAccountId ?? accounts[0]?.id ?? null
  );
  // 遷移直後に開くべきメッセージ（ホームの新着クリック）
  const pendingOpen = useRef<number | null>(initialMailId);
  const [mails, setMails] = useState<MailSummary[]>([]);
  const [opened, setOpened] = useState<MailDetail | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [status, setStatus] = useState('');
  const [windowSel, setWindowSel] = useState('6m');
  const [layout, setLayout] = useState<'side' | 'top'>('side');

  const current = accounts.find((a) => a.id === selected);

  useEffect(() => {
    if (selected == null && accounts.length > 0) setSelected(accounts[0].id);
  }, [accounts, selected]);

  useEffect(() => {
    setWindowSel(current?.sync_window ?? '6m');
  }, [current?.sync_window]);

  const loadMails = (id: number) => mailList(id, 200).then(setMails).catch(() => undefined);
  useEffect(() => {
    setOpened(null);
    if (selected != null) {
      loadMails(selected).then(() => {
        const pid = pendingOpen.current;
        if (pid != null) {
          pendingOpen.current = null;
          openMail(pid);
        }
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected]);

  // 開いたメッセージをリスト内でフォーカス（スクロール）
  useEffect(() => {
    if (opened?.id != null) {
      document.getElementById(`mail-li-${opened.id}`)?.scrollIntoView({ block: 'nearest' });
    }
  }, [opened?.id]);

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

  if (accounts.length === 0) {
    return <div className="p-8 text-white/60">{t('mailbox.addInSettings')}</div>;
  }

  const listPane = (
    <ul className="min-h-0 space-y-1 overflow-y-auto p-2">
      {mails.length === 0 ? (
        <li className="px-2 py-3 text-sm text-white/50">{t('mailbox.empty')}</li>
      ) : (
        mails.map((m) => (
          <li
            key={m.id}
            id={`mail-li-${m.id}`}
            onClick={() => openMail(m.id)}
            className={`cursor-pointer rounded-md px-3 py-2 hover:bg-white/10 ${
              opened?.id === m.id ? 'bg-white/15 ring-1 ring-sky-300/40' : ''
            }`}
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
            <div className="line-clamp-1 text-xs text-white/40">{m.preview}</div>
          </li>
        ))
      )}
    </ul>
  );

  const bodyPane = opened ? (
    <MailBody detail={opened} />
  ) : (
    <div className="flex h-full items-center justify-center text-sm text-white/40">
      {t('mailbox.selectMail')}
    </div>
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-white/10 px-4 py-2">
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
        <button
          className={btnCls}
          onClick={() => setLayout((l) => (l === 'side' ? 'top' : 'side'))}
          title={t('mailbox.window')}
        >
          {layout === 'side' ? '▥ ' + t('mailbox.side') : '▤ ' + t('mailbox.top')}
        </button>
        {status && <span className="text-xs text-white/60">{status}</span>}
      </div>

      {layout === 'side' ? (
        <div className="grid min-h-0 flex-1 grid-cols-[340px_1fr]">
          <div className="min-h-0 border-r border-white/10">{listPane}</div>
          <div className="min-h-0">{bodyPane}</div>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col">
          <div className="h-1/3 min-h-0 border-b border-white/10">{listPane}</div>
          <div className="min-h-0 flex-1">{bodyPane}</div>
        </div>
      )}
    </div>
  );
}
