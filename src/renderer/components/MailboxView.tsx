import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Bookmark,
  Columns2,
  Flag,
  Mail,
  Paperclip,
  RefreshCw,
  Rows2,
  Star,
  UserRound,
  type LucideIcon,
} from 'lucide-react';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailSummary } from '@bindings/MailSummary';
import type { MailDetail } from '@bindings/MailDetail';
import { mailGet, mailList, mailSync } from '../services/mail';
import { MailBody } from './MailBody';
import { FolderCombobox } from './FolderCombobox';

const iconBtn =
  'flex h-8 w-8 items-center justify-center rounded-md bg-white/10 hover:bg-white/20 disabled:opacity-40';

/** リスト絞り込みのトグル。star/known/bookmark/flag はバックエンド実装まで非適用（並びのみ）。 */
const FILTERS: { key: string; Icon: LucideIcon }[] = [
  { key: 'unread', Icon: Mail },
  { key: 'star', Icon: Star },
  { key: 'known', Icon: UserRound },
  { key: 'bookmark', Icon: Bookmark },
  { key: 'attachment', Icon: Paperclip },
  { key: 'flag', Icon: Flag },
];

function matchesFilters(m: MailSummary, filters: Set<string>): boolean {
  if (filters.has('unread') && m.is_read) return false;
  if (filters.has('attachment') && !m.has_attachments) return false;
  // star/known/bookmark/flag は対応データが入るまでフィルタしない（空表示で混乱させない）
  return true;
}

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
  const [layout, setLayout] = useState<'side' | 'top'>('side');
  // 表示するフォルダ/グループ（受信箱以外は後続実装）
  const [folder, setFolder] = useState('inbox');
  // リスト絞り込みトグル
  const [filters, setFilters] = useState<Set<string>>(new Set());
  const toggleFilter = (key: string) =>
    setFilters((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  useEffect(() => {
    if (selected == null && accounts.length > 0) setSelected(accounts[0].id);
  }, [accounts, selected]);

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

  const visibleMails = mails.filter((m) => matchesFilters(m, filters));

  const listPane =
    folder !== 'inbox' ? (
      <div className="flex h-full items-center justify-center p-4 text-sm text-white/40">
        {t('comingSoon')}
      </div>
    ) : (
    <ul className="h-full space-y-1 overflow-y-auto p-2">
      {visibleMails.length === 0 ? (
        <li className="px-2 py-3 text-sm text-white/50">{t('mailbox.empty')}</li>
      ) : (
        visibleMails.map((m) => (
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
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-white/10 px-4 py-2">
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
        <FolderCombobox value={folder} onChange={setFolder} />

        <span className="mx-1 h-5 w-px bg-white/15" />
        {/* 絞り込みトグル: 選択時のみハイライト、オフはゴースト */}
        {FILTERS.map(({ key, Icon }) => {
          const on = filters.has(key);
          return (
            <button
              key={key}
              onClick={() => toggleFilter(key)}
              title={t(`filter.${key}`)}
              aria-label={t(`filter.${key}`)}
              aria-pressed={on}
              className={`flex h-8 w-8 items-center justify-center rounded-md ${
                on
                  ? 'bg-sky-500/30 text-sky-200 ring-1 ring-sky-300/40'
                  : 'text-white/55 hover:text-white/80'
              }`}
            >
              <Icon size={15} />
            </button>
          );
        })}
        <span className="mx-1 h-5 w-px bg-white/15" />

        <button
          className={iconBtn}
          onClick={onSync}
          disabled={syncing || selected == null}
          title={t('mailbox.sync')}
          aria-label={t('mailbox.sync')}
        >
          <RefreshCw size={15} className={syncing ? 'animate-spin' : ''} />
        </button>
        <button
          className={iconBtn}
          onClick={() => setLayout((l) => (l === 'side' ? 'top' : 'side'))}
          title={layout === 'side' ? t('mailbox.side') : t('mailbox.top')}
          aria-label={layout === 'side' ? t('mailbox.side') : t('mailbox.top')}
        >
          {layout === 'side' ? <Columns2 size={15} /> : <Rows2 size={15} />}
        </button>
        {status && <span className="text-xs text-white/60">{status}</span>}
      </div>

      {layout === 'side' ? (
        <div className="grid min-h-0 flex-1 grid-cols-[340px_1fr] overflow-hidden">
          <div className="min-h-0 overflow-hidden border-r border-white/10">{listPane}</div>
          <div className="min-h-0 overflow-hidden">{bodyPane}</div>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <div className="h-1/3 min-h-0 overflow-hidden border-b border-white/10">{listPane}</div>
          <div className="min-h-0 flex-1 overflow-hidden">{bodyPane}</div>
        </div>
      )}
    </div>
  );
}
