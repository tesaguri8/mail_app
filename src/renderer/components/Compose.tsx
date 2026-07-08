import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { Paperclip, Send, X } from 'lucide-react';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailDetail } from '@bindings/MailDetail';
import type { DraftContent } from '@bindings/DraftContent';
import type { SignatureSummary } from '@bindings/SignatureSummary';
import {
  attachmentMeta,
  mailDraftDiscard,
  mailDraftSyncRemote,
  mailSaveDraft,
  mailSend,
} from '../services/mail';
import { signatureList } from '../services/signatures';
import { getFlyAnimation } from '../config/prefs';
import { playFlySound } from '../utils/flySound';
import { RecipientInput } from './RecipientInput';
import { FlySwallow, type FlySwallowHandle } from './FlySwallow';
import swallowUrl from '../assets/swallow.png';

/** 作成モード。返信/転送は元メール（source）を伴う。draft は保存済み下書きの再編集。
 * new は任意で宛先(to)を初期設定できる（アドレスのクリックから「このアドレスへ新規」）。 */
export type ComposeTarget =
  | { mode: 'new'; to?: string }
  | { mode: 'reply' | 'replyAll' | 'forward'; source: MailDetail }
  | { mode: 'draft'; draft: DraftContent };

/** "Re: " / "Fwd: " を二重に付けない。 */
/** 添付の合計サイズ上限（Rust 側の MAX_ATTACHMENT_TOTAL と揃える）。 */
const MAX_ATTACH_TOTAL = 25 * 1024 * 1024;

/** バイト数を人が読みやすい単位に整形する。 */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** 作成画面で保持する添付（送信時に path を Rust へ渡す）。 */
type Attach = { path: string; name: string; size: number };

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

/** テキストを HTML 属性/本文へ安全に埋め込むためのエスケープ。 */
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/**
 * 返信で引用するオリジナル HTML を安全化する。原文を再構築せず、危険/解決不能な要素だけ除く：
 * - script / style / head / meta / link / base などは削除
 * - cid: インライン画像は返信では解決できないためプレースホルダに置換
 * - 返す innerHTML を blockquote で包んで使う（呼び出し側）
 */
function sanitizeQuotedHtml(html: string): string {
  let doc: Document;
  try {
    doc = new DOMParser().parseFromString(html, 'text/html');
  } catch {
    return '';
  }
  doc
    .querySelectorAll('script,style,head,title,noscript,meta,link,base,iframe,object,embed')
    .forEach((el) => el.remove());
  doc.querySelectorAll('img').forEach((img) => {
    const src = (img.getAttribute('src') ?? '').trim().toLowerCase();
    if (src.startsWith('cid:')) {
      img.replaceWith(doc.createTextNode(`[${img.getAttribute('alt') || '画像'}]`));
    }
  });
  return doc.body.innerHTML;
}

/** 引用スタイル付き blockquote（Apple Mail / Thunderbird 風の左罫線）。 */
function blockquote(innerHtml: string): string {
  return `<blockquote type="cite" style="margin:0 0 0 0.8ex;border-left:1px solid #ccc;padding-left:1ex;color:inherit">${innerHtml}</blockquote>`;
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

/** "名前 <addr>" / "addr" からメールアドレス部分だけを取り出す（小文字化はしない）。 */
function extractEmail(token: string): string {
  const m = token.match(/<([^>]+)>/);
  return (m ? m[1] : token).trim();
}

/**
 * 複数のアドレスリスト文字列（"名前 <addr>, ..."）を結合し、exclude のメール（自分・差出人など）と
 * 重複を除いて 1 本の文字列にする。表示名付きの表記はそのまま残す。全員返信の Cc 生成に使う。
 */
function mergeAddressList(lists: string[], exclude: string[]): string {
  const skip = new Set(exclude.map((e) => extractEmail(e).toLowerCase()).filter(Boolean));
  const seen = new Set<string>();
  const out: string[] = [];
  for (const list of lists) {
    for (const token of list.split(',')) {
      const t = token.trim();
      if (!t) continue;
      const email = extractEmail(t).toLowerCase();
      if (!email || skip.has(email) || seen.has(email)) continue;
      seen.add(email);
      out.push(t);
    }
  }
  return out.join(', ');
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

  // 元メールから初期値（宛先・件名・In-Reply-To、および送信時に本文末へ足す「引用/転送」部分）を
  // 組み立てる。引用は編集欄には入れず、送信/下書き保存の直前に本文へ連結する（書く欄を広く保つ）。
  const init = useMemo(() => {
    if (target.mode === 'new') {
      return {
        to: target.to ?? '',
        cc: '',
        bcc: '',
        subject: '',
        quoted: '',
        quotedHtml: null as string | null,
        inReplyTo: null as string | null,
      };
    }
    if (target.mode === 'draft') {
      // 保存済み下書きの再編集。本文は保存時の全文（署名・引用込み）をそのまま編集する
      // ので、送信時に足す引用(quoted)は無し。返信下書きは In-Reply-To を、手入力 Bcc も復元する。
      const d = target.draft;
      return {
        to: d.to,
        cc: d.cc,
        bcc: d.bcc,
        subject: d.subject,
        quoted: '',
        quotedHtml: null as string | null,
        inReplyTo: d.in_reply_to,
      };
    }
    const s = target.source;
    const body = s.body_plain ?? s.clean_body ?? '';
    const attribution = t('compose.quoteHeader', {
      from: s.from_address ?? '',
      date: formatQuoteDate(s.date),
    });
    // 元メールが HTML を持つなら、その HTML をサニタイズして「丸ごと引用」に使う（B 案）。
    // 持たない（text/plain 元）ならプレーン引用だけで送る。
    const srcHtml = s.body_html?.trim() ? sanitizeQuotedHtml(s.body_html) : '';
    if (target.mode === 'forward') {
      const fwd =
        `\n\n${t('compose.forwardSep')}\n` +
        `${t('mailbox.from')}: ${s.from_address ?? ''}\n` +
        `${t('mailbox.to')}: ${s.to_addresses ?? ''}\n` +
        `${t('compose.subject')}: ${s.subject ?? ''}\n\n` +
        body;
      const fwdHeaderHtml = [
        escapeHtml(t('compose.forwardSep')),
        `${escapeHtml(t('mailbox.from'))}: ${escapeHtml(s.from_address ?? '')}`,
        `${escapeHtml(t('mailbox.to'))}: ${escapeHtml(s.to_addresses ?? '')}`,
        `${escapeHtml(t('compose.subject'))}: ${escapeHtml(s.subject ?? '')}`,
      ].join('<br>');
      return {
        to: '',
        cc: '',
        bcc: '',
        subject: withPrefix(s.subject, 'Fwd'),
        quoted: fwd,
        quotedHtml: srcHtml.trim() ? `<br><br>${fwdHeaderHtml}<br><br>${srcHtml}` : null,
        inReplyTo: null,
      };
    }
    // 返信先: 差出人が Reply-To を指定していればそちらへ返信する（ML・no-reply＋実返信先 等）。
    // 無ければ従来どおり From。
    const replyTarget = s.reply_to?.trim() ? s.reply_to.trim() : (s.from_address ?? '');
    // reply / replyAll。全員返信の Cc には「元の宛先(To)＋元の Cc」を入れる。
    // 自分（受信アカウント）と返信先（To に入る）・差出人は Cc から除外し、重複も除く。
    const selfEmail = accounts.find((a) => a.id === s.account_id)?.email ?? '';
    const cc =
      target.mode === 'replyAll'
        ? mergeAddressList(
            [s.to_addresses ?? '', s.cc_addresses ?? ''],
            [selfEmail, s.from_address ?? '', replyTarget]
          )
        : '';
    return {
      to: replyTarget,
      cc,
      bcc: '',
      subject: withPrefix(s.subject, 'Re'),
      quoted: `\n\n${attribution}\n${quote(body)}`,
      quotedHtml: srcHtml.trim()
        ? `<br><br>${escapeHtml(attribution)}<br>${blockquote(srcHtml)}`
        : null,
      inReplyTo: s.message_id,
    };
  }, [target, t, accounts]);

  const [accountId, setAccountId] = useState<number | null>(
    // 下書きの再編集は保存時のアカウントを使う。
    target.mode === 'draft'
      ? target.draft.account_id
      : (defaultAccountId ?? accounts[0]?.id ?? null)
  );
  const [to, setTo] = useState(init.to);
  const [cc, setCc] = useState(init.cc);
  const [bcc, setBcc] = useState(init.bcc);
  const [showCc, setShowCc] = useState(Boolean(init.cc || init.bcc));
  const [subject, setSubject] = useState(init.subject);
  // 下書きの再編集は保存済み本文（署名・引用込み）をそのまま編集する。それ以外は空から。
  const [body, setBody] = useState(target.mode === 'draft' ? target.draft.body : '');
  const [sending, setSending] = useState(false);
  const [error, setError] = useState('');
  // 添付ファイル（picker で選択。送信時に path を Rust へ渡して読み込ませる）。
  const [attachments, setAttachments] = useState<Attach[]>([]);

  // 送信時に本文末へ足す引用/転送ブロック（編集欄には入れない）。プレーンと、あれば HTML。
  const quotedRef = useRef(init.quoted);
  quotedRef.current = init.quoted;
  const quotedHtmlRef = useRef(init.quotedHtml);
  quotedHtmlRef.current = init.quotedHtml;
  // 下書き保存に使う「プレーン全文」＝編集した本文（＋署名）＋プレーン引用。
  const composedBody = useCallback(() => body + quotedRef.current, [body]);

  // 署名（差出人ごとに使い回せる本文）。一覧を読み込み、ドロップダウンで切り替える。
  // 既定はアカウントの signature_id。切替時は本文中の署名ブロックだけを置換する。
  const [signatures, setSignatures] = useState<SignatureSummary[]>([]);
  const [sigId, setSigId] = useState<number | null>(null);
  // 現在 body に埋め込んでいる署名ブロック（"\n\n<署名>"。未挿入は ''）。切替時の除去に使う。
  const sigBlockRef = useRef('');

  useEffect(() => {
    signatureList()
      .then(setSignatures)
      .catch(() => undefined);
  }, []);

  // 署名を選び直す（null=なし）。旧ブロックを剥がし、新ブロックを本文末へ足す
  // （引用は送信時に付くので、署名は書いた本文の直後＝末尾でよい）。
  const applySignature = useCallback(
    (id: number | null) => {
      const sig = signatures.find((s) => s.id === id) ?? null;
      const block = sig && sig.body.trim() ? `\n\n${sig.body.replace(/\s+$/, '')}` : '';
      const old = sigBlockRef.current;
      setBody((prev) => {
        const stripped = old && prev.includes(old) ? prev.replace(old, '') : prev;
        return stripped + block;
      });
      sigBlockRef.current = block;
      setSigId(id);
    },
    [signatures]
  );

  // 署名が読み込めたら（およびアカウント変更時に）そのアカウントの既定署名を適用する。
  // 下書きの再編集では本文に署名が既に含まれているので自動挿入しない（二重を防ぐ）。
  useEffect(() => {
    if (signatures.length === 0 || target.mode === 'draft') return;
    const acc = accounts.find((a) => a.id === accountId);
    applySignature(acc?.signature_id ?? null);
  }, [accountId, signatures, accounts, applySignature, target.mode]);

  // 下書きの自動保存。ユーザーが何か書き込んだら（dirty）ローカルの drafts へ保存する。
  // 署名の自動挿入や返信の引用だけでは保存しない（実際に書いたものだけ残す）。
  // 再編集中はその下書きの id で上書き更新する。
  const draftIdRef = useRef<number | null>(target.mode === 'draft' ? target.draft.id : null);
  const dirtyRef = useRef(false);
  const [dirty, setDirty] = useState(false);
  const [saved, setSaved] = useState(false);
  const markDirty = () => {
    if (!dirtyRef.current) {
      dirtyRef.current = true;
      setDirty(true);
    }
  };

  // 送信中フラグ（ref）。送信直前に立て、保留中の自動保存が送信後に下書きを作らないようにする。
  const sendingRef = useRef(false);

  // 現在の内容で下書きを保存/更新する（自動保存と、閉じる時の確定保存で共有）。
  const saveDraft = useCallback(async () => {
    if (accountId == null || sendingRef.current) return;
    try {
      const id = await mailSaveDraft({
        draft_id: draftIdRef.current,
        account_id: accountId,
        to: splitAddresses(to),
        cc: splitAddresses(cc),
        bcc: splitAddresses(bcc),
        subject,
        body: composedBody(),
        in_reply_to: init.inReplyTo,
      });
      draftIdRef.current = id;
      setSaved(true);
    } catch {
      // 自動保存の失敗は致命的でないので黙って無視（次の入力で再試行）。
    }
  }, [accountId, to, cc, bcc, subject, composedBody, init.inReplyTo]);

  // 入力のたびにデバウンス（1s）して自動保存。dirty になって初めて走る。
  useEffect(() => {
    if (!dirty) return;
    const h = setTimeout(() => void saveDraft(), 1000);
    return () => clearTimeout(h);
  }, [dirty, saveDraft]);

  // 閉じる時、書きかけがあれば「下書きに残すか」を確認する。
  // 残す → 最新をローカル保存し、サーバー Drafts へも同期（背景）。
  // 破棄 → ローカル＋サーバーの下書きを削除。
  const closeGuarded = async () => {
    if (dirty) {
      const keep = window.confirm(t('compose.keepDraftConfirm'));
      if (keep) {
        await saveDraft(); // 最新の内容で確定保存
        if (draftIdRef.current != null) {
          // サーバー同期は待たない（IMAP 往復で閉じるのを遅らせない）。
          void mailDraftSyncRemote(draftIdRef.current).catch(() => undefined);
        }
      } else if (draftIdRef.current != null) {
        try {
          await mailDraftDiscard(draftIdRef.current); // ローカルは即時・サーバーは背景で削除
        } catch {
          // 破棄の失敗は致命的でないので無視
        }
      }
    }
    onClose();
  };

  // 送信アニメーション（つばめ）を使うか（設定・既定オン）。開いた時点の値を採用。
  const [flyOn] = useState(getFlyAnimation);
  const flyRef = useRef<FlySwallowHandle>(null);
  const sendBtnRef = useRef<HTMLButtonElement>(null);

  const attachTotal = attachments.reduce((s, a) => s + a.size, 0);
  const attachTooBig = attachTotal > MAX_ATTACH_TOTAL;
  const canSend =
    accountId != null && splitAddresses(to).length > 0 && !sending && !attachTooBig;

  // ファイルを選んで添付に追加する（重複パスは除外）。
  const pickAttachments = async () => {
    const picked = await open({ multiple: true }).catch(() => null);
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    const metas = await attachmentMeta(paths).catch(() => []);
    setAttachments((prev) => {
      const seen = new Set(prev.map((a) => a.path));
      const add = metas
        .filter((m) => !seen.has(m.path))
        .map((m) => ({ path: m.path, name: m.name, size: Number(m.size) }));
      return [...prev, ...add];
    });
  };
  const removeAttachment = (path: string) =>
    setAttachments((prev) => prev.filter((a) => a.path !== path));

  const onSend = async () => {
    if (accountId == null) return;
    sendingRef.current = true;
    setSending(true);
    setError('');
    // 送信リクエストは即開始し、その完了を待つ間つばめを飛ばす。
    const send = mailSend({
      account_id: accountId,
      to: splitAddresses(to),
      cc: splitAddresses(cc),
      bcc: splitAddresses(bcc),
      subject,
      // 新規本文（＋署名）と引用を分けて渡す。HTML 引用があればオリジナル HTML を
      // 丸ごと引用し、無ければプレーン引用のみ（Rust 側で組み立て）。
      body,
      quoted_plain: quotedRef.current || null,
      quoted_html: quotedHtmlRef.current || null,
      in_reply_to: init.inReplyTo,
      // References チェーンは Rust 側で in_reply_to から祖先を辿って積む（docs/THREADING.md）。
      references: null,
      // 添付はローカルパスを渡し、Rust が送信時に読み込んで MIME に同梱する。
      attachments: attachments.map((a) => a.path),
    });
    try {
      if (flyOn && flyRef.current && sendBtnRef.current) {
        playFlySound(); // 仮の羽ばたき音（後日 差し替え）
        const r = sendBtnRef.current.getBoundingClientRect();
        await flyRef.current.deliver({ x: r.left + r.width / 2, y: r.top + r.height / 2 }, send);
      } else {
        await send;
      }
      // 送信できたら、残っている下書きは不要なのでローカル＋サーバーから消す。
      if (draftIdRef.current != null) {
        try {
          await mailDraftDiscard(draftIdRef.current);
        } catch {
          // 下書き削除の失敗は無視（送信自体は成功している）
        }
        draftIdRef.current = null;
      }
      onClose();
    } catch (e) {
      setError(String(e));
      sendingRef.current = false; // 送信失敗: 自動保存を再開できるように戻す
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
          onClick={closeGuarded}
          disabled={sending}
          className="flex h-7 w-7 items-center justify-center rounded-md text-white/55 hover:text-white/85 disabled:opacity-40"
          aria-label={t('account.cancel')}
        >
          <X size={16} />
        </button>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-4">
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
            onChange={(v) => {
              setTo(v);
              markDirty();
            }}
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
                onChange={(v) => {
                  setCc(v);
                  markDirty();
                }}
                placeholder={t('compose.ccPlaceholder')}
              />
            </div>
            <div className="flex items-center gap-2">
              <label className="w-12 shrink-0 text-xs text-white/45">{t('compose.bcc')}</label>
              <RecipientInput
                className={inputCls}
                value={bcc}
                onChange={(v) => {
                  setBcc(v);
                  markDirty();
                }}
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
            onChange={(e) => {
              setSubject(e.target.value);
              markDirty();
            }}
            placeholder={t('compose.subjectPlaceholder')}
          />
        </div>

        {/* 添付ファイル */}
        <div className="flex flex-col gap-1.5">
          <div className="flex items-center gap-2">
            <label className="w-12 shrink-0 text-xs text-white/45">{t('compose.attachLabel')}</label>
            <button
              type="button"
              onClick={pickAttachments}
              className="flex items-center gap-1.5 rounded-md bg-white/10 px-2.5 py-1.5 text-xs text-white/80 hover:bg-white/20"
            >
              <Paperclip size={13} />
              {t('compose.attach')}
            </button>
            {attachments.length > 0 && (
              <span className={`text-[11px] ${attachTooBig ? 'text-red-400' : 'text-white/40'}`}>
                {t('compose.attachCount', { count: attachments.length })}・{formatSize(attachTotal)}
              </span>
            )}
          </div>
          {attachments.length > 0 && (
            <div className="flex flex-wrap gap-1.5 pl-14">
              {attachments.map((a) => (
                <span
                  key={a.path}
                  title={`${a.name}（${formatSize(a.size)}）`}
                  className="inline-flex max-w-[16rem] items-center gap-1.5 rounded-md bg-white/10 py-1 pl-2 pr-1 text-xs"
                >
                  <Paperclip size={12} className="shrink-0 text-white/50" />
                  <span className="min-w-0 flex-1 truncate">{a.name}</span>
                  <span className="shrink-0 text-white/40">{formatSize(a.size)}</span>
                  <button
                    type="button"
                    onClick={() => removeAttachment(a.path)}
                    title={t('compose.attachRemove')}
                    aria-label={t('compose.attachRemove')}
                    className="shrink-0 rounded p-0.5 text-white/50 hover:bg-white/15 hover:text-white"
                  >
                    <X size={12} />
                  </button>
                </span>
              ))}
            </div>
          )}
          {attachTooBig && (
            <p className="pl-14 text-[11px] text-red-400">{t('compose.attachTooLarge')}</p>
          )}
        </div>

        {/* 署名の選択（切り替え）。署名が 1 つも無いときは行ごと隠す。 */}
        {signatures.length > 0 && (
          <div className="flex items-center gap-2">
            <label className="w-12 shrink-0 text-xs text-white/45">{t('compose.signature')}</label>
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

        {/* 本文（引用は編集欄に入れず、送信時に付ける）。ペインの残り高さいっぱいに広げる。 */}
        <textarea
          className="min-h-[10rem] w-full flex-1 resize-none rounded-md bg-white/10 px-3 py-2 text-sm leading-relaxed outline-none placeholder:text-white/30 focus:bg-white/15"
          value={body}
          onChange={(e) => {
            setBody(e.target.value);
            markDirty();
          }}
          placeholder={t('compose.bodyPlaceholder')}
        />
        {quotedRef.current && (
          <p className="shrink-0 text-[11px] text-white/35">{t('compose.quoteAppendNote')}</p>
        )}
      </div>

      <div className="flex items-center gap-3 border-t border-white/10 px-4 py-2.5">
        <button
          ref={sendBtnRef}
          onClick={onSend}
          disabled={!canSend}
          className="flex items-center gap-1.5 rounded-md bg-sky-500/90 px-4 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-40"
        >
          {flyOn ? <img src={swallowUrl} alt="" className="h-4 w-auto" /> : <Send size={14} />}
          {sending ? t('compose.sending') : flyOn ? t('compose.fly') : t('compose.send')}
        </button>
        {error ? (
          <span className="flex-1 truncate text-xs text-rose-300">{error}</span>
        ) : (
          saved && (
            <span className="flex-1 truncate text-xs text-white/40">{t('compose.draftSaved')}</span>
          )
        )}
      </div>

      {flyOn && <FlySwallow ref={flyRef} />}
    </div>
  );
}
