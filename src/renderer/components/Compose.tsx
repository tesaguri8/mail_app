import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Send, X } from 'lucide-react';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailDetail } from '@bindings/MailDetail';
import type { SignatureSummary } from '@bindings/SignatureSummary';
import { mailSend } from '../services/mail';
import { signatureList } from '../services/signatures';
import { getFlyAnimation } from '../config/prefs';
import { playFlySound } from '../utils/flySound';
import { RecipientInput } from './RecipientInput';
import { FlySwallow, type FlySwallowHandle } from './FlySwallow';
import swallowUrl from '../assets/swallow.png';

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
 * メール作成ページ（新規／返信／全員返信／転送）。別ウィンドウにせず、メール画面内の
 * 全面ペインとして表示する（返信/転送は左=下書き・右=元メールの2分割）。
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

  // 元メールから初期値（宛先・件名・In-Reply-To、および署名より下に置く「引用/転送」部分）を
  // 組み立てる。署名はこの `after`（引用ブロック）の直前へ差し込む。
  const init = useMemo(() => {
    if (target.mode === 'new') {
      return { to: '', cc: '', subject: '', after: '', inReplyTo: null as string | null };
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
      return { to: '', cc: '', subject: withPrefix(s.subject, 'Fwd'), after: fwd, inReplyTo: null };
    }
    // reply / replyAll
    const cc = target.mode === 'replyAll' ? (s.to_addresses ?? '') : '';
    return {
      to: s.from_address ?? '',
      cc,
      subject: withPrefix(s.subject, 'Re'),
      after: `\n\n${attribution}\n${quote(body)}`,
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
  const [body, setBody] = useState(init.after);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState('');

  // 署名（差出人ごとに使い回せる本文）。一覧を読み込み、ドロップダウンで切り替える。
  // 既定はアカウントの signature_id。切替時は本文中の署名ブロックだけを置換する。
  const [signatures, setSignatures] = useState<SignatureSummary[]>([]);
  const [sigId, setSigId] = useState<number | null>(null);
  // 現在 body に埋め込んでいる署名ブロック（"\n\n<署名>"。未挿入は ''）。切替時の除去に使う。
  const sigBlockRef = useRef('');
  const afterRef = useRef(init.after);
  afterRef.current = init.after;

  useEffect(() => {
    signatureList()
      .then(setSignatures)
      .catch(() => undefined);
  }, []);

  // 署名を選び直す（null=なし）。旧ブロックを剥がし、新ブロックを引用の直前へ差し込む。
  const applySignature = useCallback(
    (id: number | null) => {
      const sig = signatures.find((s) => s.id === id) ?? null;
      const block = sig && sig.body.trim() ? `\n\n${sig.body.replace(/\s+$/, '')}` : '';
      const old = sigBlockRef.current;
      setBody((prev) => {
        const stripped = old && prev.includes(old) ? prev.replace(old, '') : prev;
        if (!block) return stripped;
        const after = afterRef.current;
        // 引用/転送部分の直前へ。無い（新規や本文編集済み）なら末尾へ足す。
        return after && stripped.includes(after)
          ? stripped.replace(after, block + after)
          : stripped + block;
      });
      sigBlockRef.current = block;
      setSigId(id);
    },
    [signatures]
  );

  // 署名が読み込めたら（およびアカウント変更時に）そのアカウントの既定署名を適用する。
  useEffect(() => {
    if (signatures.length === 0) return;
    const acc = accounts.find((a) => a.id === accountId);
    applySignature(acc?.signature_id ?? null);
  }, [accountId, signatures, accounts, applySignature]);

  // 送信アニメーション（つばめ）を使うか（設定・既定オン）。開いた時点の値を採用。
  const [flyOn] = useState(getFlyAnimation);
  const flyRef = useRef<FlySwallowHandle>(null);
  const sendBtnRef = useRef<HTMLButtonElement>(null);

  const canSend = accountId != null && splitAddresses(to).length > 0 && !sending;

  const onSend = async () => {
    if (accountId == null) return;
    setSending(true);
    setError('');
    // 送信リクエストは即開始し、その完了を待つ間つばめを飛ばす。
    const send = mailSend({
      account_id: accountId,
      to: splitAddresses(to),
      cc: splitAddresses(cc),
      bcc: splitAddresses(bcc),
      subject,
      body,
      in_reply_to: init.inReplyTo,
    });
    try {
      if (flyOn && flyRef.current && sendBtnRef.current) {
        playFlySound(); // 仮の羽ばたき音（後日 差し替え）
        const r = sendBtnRef.current.getBoundingClientRect();
        await flyRef.current.deliver({ x: r.left + r.width / 2, y: r.top + r.height / 2 }, send);
      } else {
        await send;
      }
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
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
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
            <RecipientInput
              className={inputCls}
              value={to}
              onChange={setTo}
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
                <RecipientInput
                  className={inputCls}
                  value={cc}
                  onChange={setCc}
                  placeholder={t('compose.ccPlaceholder')}
                />
              </div>
              <div className="flex items-center gap-2">
                <label className="w-12 shrink-0 text-xs text-white/45">{t('compose.bcc')}</label>
                <RecipientInput
                  className={inputCls}
                  value={bcc}
                  onChange={setBcc}
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

          {/* 署名の選択（切り替え）。署名が 1 つも無いときは行ごと隠す。 */}
          {signatures.length > 0 && (
            <div className="flex items-center gap-2">
              <label className="w-12 shrink-0 text-xs text-white/45">
                {t('compose.signature')}
              </label>
              <select
                className="rounded-md bg-white/10 px-2 py-1.5 text-sm outline-none"
                value={sigId ?? ''}
                onChange={(e) => applySignature(e.target.value ? Number(e.target.value) : null)}
              >
                <option value="" className="text-black">
                  {t('compose.noSignature')}
                </option>
                {signatures.map((s) => (
                  <option key={s.id} value={s.id} className="text-black">
                    {s.name}
                  </option>
                ))}
              </select>
            </div>
          )}

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
            ref={sendBtnRef}
            onClick={onSend}
            disabled={!canSend}
            className="flex items-center gap-1.5 rounded-md bg-sky-500/90 px-4 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-40"
          >
            {flyOn ? (
              <img src={swallowUrl} alt="" className="h-4 w-auto" />
            ) : (
              <Send size={14} />
            )}
            {sending ? t('compose.sending') : flyOn ? t('compose.fly') : t('compose.send')}
          </button>
          {error && <span className="flex-1 truncate text-xs text-rose-300">{error}</span>}
        </div>

      {flyOn && <FlySwallow ref={flyRef} />}
    </div>
  );
}
