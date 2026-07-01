import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Send, X } from 'lucide-react';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailDetail } from '@bindings/MailDetail';
import { mailSend } from '../services/mail';

/** 作成モード。返信/転送は元メール（source）を伴う。 */
export type ComposeTarget =
  | { mode: 'new' }
  | { mode: 'reply' | 'replyAll' | 'forward'; source: MailDetail };

/** "Re: " / "Fwd: " を二重に付けない。 */
function withPrefix(subject: string | null, prefix: 'Re' | 'Fwd'): string {
  const s = (subject ?? '').trim();
  const re = new RegExp(`^${prefix}:`, 'i');
  return re.test(s) ? s : `${prefix}: ${s}`;
}

/** 本文を引用形式（各行を "> "）にする。 */
function quote(body: string): string {
  return body
    .split('\n')
    .map((l) => `> ${l}`)
    .join('\n');
}

/** 引用ヘッダ用に日付をローカル時刻表記へ（生の ISO/UTC 文字列を見せない）。 */
function formatQuoteDate(d: string | null): string {
  if (!d) return '';
  const dt = new Date(d);
  return isNaN(dt.getTime()) ? d : dt.toLocaleString();
}

/** カンマ・改行区切りの入力をアドレス配列へ。 */
function splitAddresses(s: string): string[] {
  return s
    .split(/[,\n]/)
    .map((a) => a.trim())
    .filter(Boolean);
}

/**
 * メール作成モーダル（新規／返信／全員返信／転送）。
 * プレーン本文で作成し、送信時にバックエンドで plain+HTML を同梱する。
 */
export function Compose({
  accounts,
  defaultAccountId,
  target,
  onClose,
}: {
  accounts: AccountSummary[];
  defaultAccountId: number | null;
  target: ComposeTarget;
  onClose: () => void;
}) {
  const { t } = useTranslation();

  // 元メールから初期値（宛先・件名・本文・In-Reply-To）を組み立てる。
  const init = useMemo(() => {
    if (target.mode === 'new') {
      return { to: '', cc: '', subject: '', body: '', inReplyTo: null as string | null };
    }
    const s = target.source;
    const body = s.body_plain ?? s.clean_body ?? '';
    const attribution = t('compose.quoteHeader', {
      from: s.from_address ?? '',
      date: formatQuoteDate(s.date),
    });
    if (target.mode === 'forward') {
      const fwd =
        `\n\n${t('compose.forwardSep')}\n` +
        `${t('mailbox.from')}: ${s.from_address ?? ''}\n` +
        `${t('mailbox.to')}: ${s.to_addresses ?? ''}\n` +
        `${t('compose.subject')}: ${s.subject ?? ''}\n\n` +
        body;
      return { to: '', cc: '', subject: withPrefix(s.subject, 'Fwd'), body: fwd, inReplyTo: null };
    }
    // reply / replyAll
    const cc = target.mode === 'replyAll' ? (s.to_addresses ?? '') : '';
    return {
      to: s.from_address ?? '',
      cc,
      subject: withPrefix(s.subject, 'Re'),
      body: `\n\n${attribution}\n${quote(body)}`,
      inReplyTo: s.message_id,
    };
  }, [target, t]);

  const [accountId, setAccountId] = useState<number | null>(
    defaultAccountId ?? accounts[0]?.id ?? null
  );
  const [to, setTo] = useState(init.to);
  const [cc, setCc] = useState(init.cc);
  const [bcc, setBcc] = useState('');
  const [showCc, setShowCc] = useState(Boolean(init.cc));
  const [subject, setSubject] = useState(init.subject);
  const [body, setBody] = useState(init.body);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState('');

  const canSend =
    accountId != null && splitAddresses(to).length > 0 && !sending;

  const onSend = async () => {
    if (accountId == null) return;
    setSending(true);
    setError('');
    try {
      await mailSend({
        account_id: accountId,
        to: splitAddresses(to),
        cc: splitAddresses(cc),
        bcc: splitAddresses(bcc),
        subject,
        body,
        in_reply_to: init.inReplyTo,
      });
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setSending(false);
    }
  };

  const inputCls =
    'w-full rounded-md bg-white/10 px-3 py-1.5 text-sm outline-none placeholder:text-white/30 focus:bg-white/15';

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !sending) onClose();
      }}
    >
      <div className="flex max-h-[88vh] w-[640px] max-w-[94vw] flex-col overflow-hidden rounded-xl border border-white/15 bg-neutral-900/95 shadow-2xl">
        <div className="flex items-center justify-between border-b border-white/10 px-4 py-2.5">
          <h2 className="text-sm font-semibold">{t(`compose.${target.mode}`)}</h2>
          <button
            onClick={onClose}
            disabled={sending}
            className="flex h-7 w-7 items-center justify-center rounded-md text-white/55 hover:text-white/85 disabled:opacity-40"
            aria-label={t('account.cancel')}
          >
            <X size={16} />
          </button>
        </div>

        <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-4">
          {/* 差出人アカウント */}
          <div className="flex items-center gap-2">
            <label className="w-12 shrink-0 text-xs text-white/45">{t('compose.from')}</label>
            <select
              className="flex-1 rounded-md bg-white/10 px-2 py-1.5 text-sm outline-none"
              value={accountId ?? ''}
              onChange={(e) => setAccountId(Number(e.target.value))}
            >
              {accounts.map((a) => (
                <option key={a.id} value={a.id} className="text-black">
                  {a.display_name ? `${a.display_name} <${a.email}>` : a.email}
                </option>
              ))}
            </select>
          </div>

          {/* 宛先 */}
          <div className="flex items-center gap-2">
            <label className="w-12 shrink-0 text-xs text-white/45">{t('compose.to')}</label>
            <input
              className={inputCls}
              value={to}
              onChange={(e) => setTo(e.target.value)}
              placeholder={t('compose.toPlaceholder')}
              autoFocus={target.mode === 'new' || target.mode === 'forward'}
            />
            {!showCc && (
              <button
                onClick={() => setShowCc(true)}
                className="shrink-0 text-xs text-sky-300 hover:underline"
              >
                {t('compose.addCc')}
              </button>
            )}
          </div>

          {showCc && (
            <>
              <div className="flex items-center gap-2">
                <label className="w-12 shrink-0 text-xs text-white/45">{t('compose.cc')}</label>
                <input
                  className={inputCls}
                  value={cc}
                  onChange={(e) => setCc(e.target.value)}
                  placeholder={t('compose.ccPlaceholder')}
                />
              </div>
              <div className="flex items-center gap-2">
                <label className="w-12 shrink-0 text-xs text-white/45">{t('compose.bcc')}</label>
                <input
                  className={inputCls}
                  value={bcc}
                  onChange={(e) => setBcc(e.target.value)}
                  placeholder={t('compose.bccPlaceholder')}
                />
              </div>
            </>
          )}

          {/* 件名 */}
          <div className="flex items-center gap-2">
            <label className="w-12 shrink-0 text-xs text-white/45">{t('compose.subject')}</label>
            <input
              className={inputCls}
              value={subject}
              onChange={(e) => setSubject(e.target.value)}
              placeholder={t('compose.subjectPlaceholder')}
            />
          </div>

          {/* 本文 */}
          <textarea
            className="h-64 w-full resize-none rounded-md bg-white/10 px-3 py-2 text-sm leading-relaxed outline-none placeholder:text-white/30 focus:bg-white/15"
            value={body}
            onChange={(e) => setBody(e.target.value)}
            placeholder={t('compose.bodyPlaceholder')}
          />
        </div>

        <div className="flex items-center gap-3 border-t border-white/10 px-4 py-2.5">
          <button
            onClick={onSend}
            disabled={!canSend}
            className="flex items-center gap-1.5 rounded-md bg-sky-500/90 px-4 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-40"
          >
            <Send size={14} />
            {sending ? t('compose.sending') : t('compose.send')}
          </button>
          {error && <span className="flex-1 truncate text-xs text-rose-300">{error}</span>}
        </div>
      </div>
    </div>
  );
}
