import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CalendarDays, ChevronDown, Cloud, GripVertical, Mail, Pencil, Users } from 'lucide-react';
import type { ReactNode } from 'react';
import type { AccountProfile } from '@bindings/AccountProfile';
import type { AccountProvider } from '@bindings/AccountProvider';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import type { GoogleCredentialsStatus } from '@bindings/GoogleCredentialsStatus';
import type { ServerAccountSummary } from '@bindings/ServerAccountSummary';
import type { SignatureSummary } from '@bindings/SignatureSummary';
import { accountDelete, accountProfileRename } from '../../services/accounts';
import { useSync } from '../SyncProvider';
import { ConfirmDialog } from '../ConfirmDialog';
import { GoogleServices } from './GoogleServices';
import { MailAccountDetails } from './MailAccountDetails';
import { MailAccountForm } from './MailAccountForm';
import { ServiceRow } from './ServiceRow';
import { type ConnState, inputCls } from './shared';

/** 提供元の印（カードの見出しの左）。 */
export function ProviderMark({ provider }: { provider: AccountProvider }) {
  if (provider === 'google') {
    return (
      <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-sky-400/25 text-[11px] font-bold text-sky-100">
        G
      </span>
    );
  }
  if (provider === 'icloud') return <Cloud size={18} className="shrink-0 text-sky-200" />;
  return <Mail size={18} className="shrink-0 text-white/60" />;
}

/**
 * 設定の「アカウント」のカード 1 枚（アドレスごと。docs/ACCOUNTS.md §2-1）。
 * 見出しは提供元とアドレス（呼び名があれば呼び名）。中にサービスごとのスイッチを並べる。
 *
 * 開閉式（同時に開けるのは 1 枚。開いているカードは親が決める）。閉じているときは見出しの
 * 1 行と、使っているサービスの小さな印だけを出す。中身は閉じても組み立てたまま隠すので、
 * 同期の途中で閉じても結果の表示は失われない。
 */
export function AccountCard({
  profile,
  mails,
  allAccounts,
  google,
  creds,
  servers,
  signatures,
  conn,
  onCheckConn,
  onChanged,
  onDragHandleDown,
  open,
  onToggleOpen,
}: {
  profile: AccountProfile;
  /** このカードのメールアカウント（`profile.mail_account_ids` の順）。 */
  mails: AccountSummary[];
  allAccounts: AccountSummary[];
  google: GoogleAccount | undefined;
  creds: GoogleCredentialsStatus | null;
  servers: ServerAccountSummary[];
  signatures: SignatureSummary[];
  conn: Record<number, ConnState>;
  onCheckConn: (id: number) => void;
  onChanged: () => void;
  /** 並べ替えのつまみを押したとき（つまみからだけドラッグを始める。入力欄の文字選択を妨げない）。
   *  並べ替えられないとき（1 枚だけ・追加の途中）は undefined でつまみを出さない。 */
  onDragHandleDown?: () => void;
  /** 開いているか。 */
  open: boolean;
  /** 見出しを押したとき（開く・閉じる）。 */
  onToggleOpen: () => void;
}) {
  const { t } = useTranslation();
  const sync = useSync();
  const [expanded, setExpanded] = useState<number | null>(null);
  const [addingMail, setAddingMail] = useState(false);
  const [confirmMailOff, setConfirmMailOff] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [error, setError] = useState('');

  const title = profile.display_name ?? profile.email;
  const mailOn = mails.length > 0;

  const saveName = async () => {
    if (renaming == null) return;
    try {
      await accountProfileRename(profile.id, renaming.trim() || null);
      setRenaming(null);
      onChanged();
    } catch (e) {
      setError('✕ ' + String(e));
    }
  };

  // メールをオフ: このアドレスのメールアカウントを外す（手元のメールも消える。サーバーは残る）。
  const removeMail = async () => {
    setRemoving(true);
    setError('');
    try {
      for (const m of mails) await accountDelete(m.id);
      setConfirmMailOff(false);
      setExpanded(null);
    } catch (e) {
      // 握り潰すと「押しても無反応」に見えるので、理由をそのまま出す。
      setError('✕ ' + String(e));
    } finally {
      setRemoving(false);
      onChanged();
    }
  };

  const connected = google != null && google.disconnected_at == null;
  const bodyId = `account-card-${profile.id}`;
  // 閉じているときに見出しへ出す、使っているサービスの印。
  type Mark = { key: string; icon: ReactNode; label: string };
  const marks: Mark[] = [];
  if (mailOn)
    marks.push({ key: 'mail', icon: <Mail size={13} />, label: t('account.serviceMail') });
  if (connected && google.sync_contacts) {
    marks.push({ key: 'contacts', icon: <Users size={13} />, label: t('account.serviceContacts') });
  }
  if (connected && google.sync_calendar) {
    marks.push({
      key: 'calendar',
      icon: <CalendarDays size={13} />,
      label: t('account.serviceCalendar'),
    });
  }

  const mailAuthHint =
    profile.provider === 'imap' ? t('account.authImap') : t('account.authAppPassword');

  return (
    <div className="rounded-lg bg-black/40 text-sm backdrop-blur-sm">
      <div className="flex items-center gap-2 px-4 py-3">
        {onDragHandleDown && (
          <GripVertical
            size={16}
            className="shrink-0 cursor-grab text-white/30"
            aria-label={t('account.reorder')}
            onMouseDown={onDragHandleDown}
          />
        )}
        {renaming != null ? (
          <>
            <ProviderMark provider={profile.provider} />
            <input
              className={inputCls}
              autoFocus
              value={renaming}
              placeholder={profile.email}
              onChange={(e) => setRenaming(e.target.value)}
              onBlur={() => void saveName()}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void saveName();
                if (e.key === 'Escape') setRenaming(null);
              }}
            />
          </>
        ) : (
          <>
            {/* 見出し全体が開閉のボタン（キーボードでも開閉できる）。 */}
            <button
              type="button"
              onClick={onToggleOpen}
              aria-expanded={open}
              aria-controls={bodyId}
              title={open ? t('account.collapse') : t('account.expand')}
              className="flex min-w-0 flex-1 items-center gap-2.5 rounded text-left focus-visible:outline focus-visible:outline-1 focus-visible:outline-sky-300/70"
            >
              <ProviderMark provider={profile.provider} />
              <span className="min-w-0 flex-1">
                <span className="block truncate font-medium text-white">{title}</span>
                <span className="block truncate text-xs text-white/45">
                  {t(`account.provider.${profile.provider}`)}
                  {profile.display_name ? ` · ${profile.email}` : ''}
                </span>
              </span>
              <span className="flex shrink-0 items-center gap-1.5 text-white/45">
                {google?.disconnected_at != null && (
                  <span className="rounded bg-amber-400/20 px-1.5 py-0.5 text-[10px] text-amber-200">
                    {t('settings.gcalDisconnectedBadge')}
                  </span>
                )}
                {marks.length === 0 ? (
                  <span className="text-[11px] text-white/35">{t('account.noServices')}</span>
                ) : (
                  marks.map((m) => (
                    <span key={m.key} title={m.label} aria-label={m.label}>
                      {m.icon}
                    </span>
                  ))
                )}
              </span>
              <ChevronDown
                size={16}
                className={`shrink-0 text-white/40 motion-safe:transition-transform motion-safe:duration-150 ${
                  open ? 'rotate-180' : ''
                }`}
              />
            </button>
            {open && (
              <button
                onClick={() => setRenaming(profile.display_name ?? '')}
                title={t('account.rename')}
                aria-label={t('account.rename')}
                className="shrink-0 rounded p-1 text-white/35 hover:bg-white/10 hover:text-white/70"
              >
                <Pencil size={13} />
              </button>
            )}
          </>
        )}
      </div>

      <div id={bodyId} hidden={!open} className="border-t border-white/5 px-4 pb-3">
        <div className="divide-y divide-white/5">
          <ServiceRow
            icon={<Mail size={16} />}
            label={t('account.serviceMail')}
            hint={
              mailOn
                ? mails.map((m) => `IMAP ${m.imap_host} · SMTP ${m.smtp_host}`).join(' / ')
                : mailAuthHint
            }
            checked={mailOn || addingMail}
            busy={removing}
            onToggle={() => {
              if (mailOn) setConfirmMailOff(true);
              else setAddingMail((v) => !v);
            }}
          >
            {addingMail && !mailOn && (
              <div className="mt-2 pl-[26px]">
                <MailAccountForm
                  email={profile.email}
                  provider={profile.provider}
                  servers={servers}
                  onAdded={(a) => {
                    setAddingMail(false);
                    onChanged();
                    // 最初の同期（バックグラウンド。進捗は共通の表示）。
                    sync.start(a.id, a.email, 'sync');
                  }}
                  onCancel={() => setAddingMail(false)}
                />
              </div>
            )}
            {mails.map((m) => (
              <div key={m.id} className="mt-1.5 pl-[26px]">
                <button
                  onClick={() => setExpanded(expanded === m.id ? null : m.id)}
                  className="flex w-full items-center gap-2 rounded px-1 py-1 text-left text-xs text-white/60 hover:bg-white/5"
                >
                  <span
                    onClick={(e) => {
                      e.stopPropagation();
                      onCheckConn(m.id); // クリック時は実 LOGIN で厳密確認
                    }}
                    title={conn[m.id]?.msg ?? conn[m.id]?.state ?? ''}
                    className={`h-2 w-2 shrink-0 rounded-full ${
                      conn[m.id]?.state === 'ok'
                        ? 'bg-emerald-400'
                        : conn[m.id]?.state === 'error'
                          ? 'bg-red-400'
                          : 'animate-pulse bg-amber-300'
                    }`}
                  />
                  <span className="flex-1 truncate">
                    {m.display_name
                      ? t('account.mailFrom', { name: m.display_name })
                      : t('account.mailDetails')}
                  </span>
                  <ChevronDown
                    size={14}
                    className={`shrink-0 text-white/40 transition-transform ${
                      expanded === m.id ? 'rotate-180' : ''
                    }`}
                  />
                </button>
                {expanded === m.id && (
                  <div className="mt-1 overflow-hidden rounded-md">
                    <MailAccountDetails
                      account={m}
                      accounts={allAccounts}
                      servers={servers}
                      signatures={signatures}
                      onChanged={onChanged}
                    />
                  </div>
                )}
              </div>
            ))}
          </ServiceRow>

          {profile.provider === 'google' && (
            <GoogleServices profile={profile} google={google} creds={creds} onChanged={onChanged} />
          )}

          {/* iCloud の連絡先・カレンダー（CardDAV / CalDAV）は後続。押せない表示だけ置く。 */}
          {profile.provider === 'icloud' && (
            <>
              <ServiceRow
                icon={<Users size={16} />}
                label={t('account.serviceContacts')}
                hint={t('account.comingSoon')}
                checked={false}
                disabled
              />
              <ServiceRow
                icon={<CalendarDays size={16} />}
                label={t('account.serviceCalendar')}
                hint={t('account.comingSoon')}
                checked={false}
                disabled
              />
            </>
          )}
        </div>
      </div>

      {error && <p className="px-4 pb-3 text-xs text-red-300">{error}</p>}

      {confirmMailOff && (
        <ConfirmDialog
          title={t('account.mailOffTitle', { email: profile.email })}
          body={t('account.mailOffBody')}
          notes={[t('account.mailOffNote')]}
          confirmLabel={t('account.mailOffRun')}
          danger
          busy={removing}
          onConfirm={() => void removeMail()}
          onCancel={() => setConfirmMailOff(false)}
        />
      )}
    </div>
  );
}
