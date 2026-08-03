import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { Copy, Paperclip, Quote, Save, Scissors, Send, Trash2, X } from 'lucide-react';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailDetail } from '@bindings/MailDetail';
import type { DraftContent } from '@bindings/DraftContent';
import type { DraftAttachment } from '@bindings/DraftAttachment';
import type { SignatureSummary } from '@bindings/SignatureSummary';
import {
  attachmentLocalPath,
  attachmentMeta,
  attachmentStage,
  mailAttachments,
  openLocalPath,
  mailDraftDiscard,
  mailDraftSyncRemote,
  mailSaveDraft,
  mailSend,
} from '../services/mail';
import { withActivity } from '../stores/activity';
import { signatureList } from '../services/signatures';
import {
  getComposeAutoSave,
  getFlyAnimation,
  getLastSignature,
  setLastSignature,
} from '../config/prefs';
import { playFlySound } from '../utils/flySound';
import { RecipientInput } from './RecipientInput';
import { ContextMenu } from './ContextMenu';
import { copyText } from '../utils/clipboard';
import { FlySwallow, type FlySwallowHandle } from './FlySwallow';
import swallowUrl from '../assets/swallow.png';

/** 作成モード。返信/転送は元メール（source）を伴う。draft は保存済み下書きの再編集。
 * new は任意で宛先(to)を初期設定できる（アドレスのクリックから「このアドレスへ新規」）。 */
export type ComposeTarget =
  | { mode: 'new'; to?: string }
  | { mode: 'reply' | 'replyAll' | 'forward'; source: MailDetail }
  // draft の再編集。source は返信下書きの元メール（右ペイン表示用・任意）。
  // 本文は保存済み全文をそのまま編集するので、source は表示専用（再引用はしない）。
  | { mode: 'draft'; draft: DraftContent; source?: MailDetail };

/** "Re: " / "Fwd: " を二重に付けない。 */
/** 添付の合計サイズ上限（Rust 側の MAX_ATTACHMENT_TOTAL と揃える）。 */
const MAX_ATTACH_TOTAL = 25 * 1024 * 1024;

/** バイト数を人が読みやすい単位に整形する。 */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/**
 * 作成画面で保持する添付。送信時は `path` を Rust へ渡す。
 * 転送で引き継いだ添付は、開いた時点では本体が手元に無いことがあるので `sourceId`
 * （元メールの attachments.id）だけを持ち、送信/保存の直前にローカルへ用意して path を得る
 * （開いただけで大きな添付をサーバーから取りに行かないため）。
 */
type Attach = {
  /** ローカルの実ファイル。未取得の転送添付では未設定。 */
  path?: string;
  /** 転送元の添付 id（未取得のものを取り直すキー）。手元のファイルなら未設定。 */
  sourceId?: number;
  name: string;
  size: number;
};

/** 一覧の key・重複判定に使う識別子（実ファイルはパス、転送添付は元の添付 id）。 */
function attachKey(a: Attach): string {
  return a.path ?? `src:${a.sourceId ?? ''}`;
}

/**
 * 添付をローカルパスへ解決する（送信/下書き保存の直前に呼ぶ）。転送で引き継いだ添付は
 * ここで初めてサーバーから取り直す。IMAP 接続を同時に張らないよう 1 件ずつ順に処理する。
 * 1 件でも用意できなければエラー（黙って添付を落として送らない）。
 */
async function resolveAttachmentPaths(items: Attach[]): Promise<string[]> {
  const paths: string[] = [];
  for (const a of items) {
    if (a.path) {
      paths.push(a.path);
    } else if (a.sourceId != null) {
      const meta = await attachmentLocalPath(a.sourceId);
      paths.push(meta.path);
    }
  }
  return paths;
}

/** 作成画面の添付を、下書きに保存する形へ変換する。 */
function toDraftAttachments(items: Attach[]): DraftAttachment[] {
  return items.map((a) => ({
    path: a.path ?? null,
    source_attachment_id: a.sourceId ?? null,
    filename: a.name,
    size: a.size,
  }));
}

/** 下書きに保存されていた添付を、作成画面の形へ戻す。 */
function fromDraftAttachments(items: DraftAttachment[]): Attach[] {
  return items.map((a) => ({
    path: a.path ?? undefined,
    sourceId: a.source_attachment_id ?? undefined,
    name: a.filename,
    size: a.size,
  }));
}

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

/**
 * 事前入力の宛先を「確定済み」として RecipientInput に渡すため末尾を ", " で止める。
 * RecipientInput は「最後のカンマ以降＝編集中の下書き」とみなすので、カンマ止めにすると
 * 事前入力が全件チップ表示になる（空はそのまま空）。送信/保存時は splitAddresses が末尾の
 * 空要素を落とすため影響しない。
 */
function committed(list: string): string {
  const s = list.trim();
  return s ? `${s}, ` : '';
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
  onDraftId,
}: {
  accounts: AccountSummary[];
  defaultAccountId: number | null;
  target: ComposeTarget;
  onClose: () => void;
  /** 現在編集中の下書き id を親へ通知する（未保存/破棄後は null）。
   * ビュー切替で作成画面が閉じても、戻った時に同じ下書きを復元するために使う。 */
  onDraftId?: (id: number | null) => void;
}) {
  const { t } = useTranslation();

  // 元メールから初期値（宛先・件名・In-Reply-To、および送信時に本文末へ足す「引用/転送」部分）を
  // 組み立てる。引用は編集欄には入れず、送信/下書き保存の直前に本文へ連結する（書く欄を広く保つ）。
  const init = useMemo(() => {
    if (target.mode === 'new') {
      return {
        to: committed(target.to ?? ''),
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
        to: committed(d.to),
        cc: committed(d.cc),
        bcc: committed(d.bcc),
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
      to: committed(replyTarget),
      cc: committed(cc),
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
  // 添付ファイル（picker 選択 or ドロップ or 転送で引き継いだ元メールの添付）。
  // 送信時にローカルパスへ解決して Rust へ渡す（下書きの再編集では保存済みの添付を復元）。
  const [attachments, setAttachments] = useState<Attach[]>(
    target.mode === 'draft' ? fromDraftAttachments(target.draft.attachments) : [],
  );
  // OS からファイルをドラッグ中か（ドロップ領域のハイライト用）。
  const [dragOver, setDragOver] = useState(false);

  // 送信時に本文末へ足す引用/転送ブロック（編集欄には入れない）。プレーンと、あれば HTML。
  const quotedRef = useRef(init.quoted);
  quotedRef.current = init.quoted;
  const quotedHtmlRef = useRef(init.quotedHtml);
  quotedHtmlRef.current = init.quotedHtml;
  // 下書き保存に使う「プレーン全文」＝編集した本文（＋署名）＋プレーン引用。
  const composedBody = useCallback(() => body + quotedRef.current, [body]);

  // 本文テキストエリアと、選択テキストの右クリックメニュー（引用文にする/コピー/切り取り）。
  // 選択があるときだけネイティブメニューを差し替え、無いときは貼り付け等のネイティブを残す。
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const [bodyMenu, setBodyMenu] = useState<{
    x: number;
    y: number;
    start: number;
    end: number;
  } | null>(null);

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

  // 署名を選び直したら、そのアカウントの次回の既定として覚える（「署名なし」も覚える）。
  const chooseSignature = useCallback(
    (id: number | null) => {
      applySignature(id);
      if (accountId != null) setLastSignature(accountId, id);
    },
    [applySignature, accountId],
  );

  // 署名が読み込めたら（およびアカウント変更時に）そのアカウントの署名を自動で適用する。
  // 採用する署名は「前回そのアカウントで選んだ署名」→ 無ければアカウントの既定署名 → 無ければ
  // 署名なし。前回「署名なし」を選んでいればそれも尊重する（毎回入る/毎回消すの手間を無くす）。
  // 下書きの再編集では本文に署名が既に含まれているので自動挿入しない（二重を防ぐ）。
  // 同じアカウントで再適用すると、本文の署名まわりを編集していた場合に旧ブロックの
  // 剥がしに失敗して署名が二重に付くため、アカウント単位で一度だけ適用する
  // （切替時のみ付け替え。手動のドロップダウン変更は従来どおり剥がして差し替える）。
  const autoSigAccountRef = useRef<number | null | undefined>(undefined);
  useEffect(() => {
    if (signatures.length === 0 || target.mode === 'draft') return;
    if (autoSigAccountRef.current === accountId) return;
    autoSigAccountRef.current = accountId;
    const fallback = accounts.find((a) => a.id === accountId)?.signature_id ?? null;
    const remembered = accountId != null ? getLastSignature(accountId) : undefined;
    // 記録が無い、または記録していた署名が削除済みならアカウント既定へ戻す。
    const chosen =
      remembered === undefined
        ? fallback
        : remembered === null || signatures.some((s) => s.id === remembered)
          ? remembered
          : fallback;
    applySignature(chosen);
  }, [accountId, signatures, accounts, applySignature, target.mode]);

  // 下書きの自動保存。ユーザーが何か書き込んだら（dirty）ローカルの drafts へ保存する。
  // 署名の自動挿入や返信の引用だけでは保存しない（実際に書いたものだけ残す）。
  // 再編集中はその下書きの id で上書き更新する。
  const draftIdRef = useRef<number | null>(target.mode === 'draft' ? target.draft.id : null);
  const dirtyRef = useRef(false);
  const [dirty, setDirty] = useState(false);
  const [saved, setSaved] = useState(false);
  // 未保存の変更（最後の保存以降に編集があるか）。閉じる確認と保存ボタンの活性に使う。
  const [unsaved, setUnsaved] = useState(false);
  // 閉じる確認ダイアログ（未保存の変更があるとき表示）。
  const [confirmClose, setConfirmClose] = useState(false);
  // 下書きの自動保存を使うか（設定・既定オン）。開いた時点の値を採用する。
  // オフのときは入力中に自動保存せず、閉じる時の確認ダイアログでのみ保存を促す。
  const [autoSave] = useState(getComposeAutoSave);
  const markDirty = () => {
    setUnsaved(true); // 毎回: 未保存の変更あり（保存で false に戻す）
    if (!dirtyRef.current) {
      dirtyRef.current = true;
      setDirty(true);
    }
  };

  // 実際に「書いたもの」があるか（宛先・件名・本文のいずれか）。署名や引用だけでは空とみなす。
  // 空の新規は下書きを作らず、閉じる確認も出さない（意図しない下書きの量産を防ぐ）。
  const hasContent = useCallback(() => {
    if (splitAddresses(to).length > 0) return true;
    if (splitAddresses(cc).length > 0) return true;
    if (splitAddresses(bcc).length > 0) return true;
    if (subject.trim().length > 0) return true;
    if (attachments.length > 0) return true; // 添付だけ足した状態も「書きかけ」として残す
    // 本文から自動挿入の署名ブロックを除いた「書いた本文」で判定する。
    const sig = sigBlockRef.current;
    const written = sig && body.includes(sig) ? body.replace(sig, '') : body;
    return written.trim().length > 0;
  }, [to, cc, bcc, subject, body, attachments]);

  // 選択範囲を引用（各行 "> "）に置き換え、置き換えた範囲を選択し直してフォーカスを戻す。
  const quoteSelection = (start: number, end: number) => {
    const src = bodyRef.current?.value ?? body;
    const q = quote(src.slice(start, end));
    setBody(src.slice(0, start) + q + src.slice(end));
    markDirty();
    requestAnimationFrame(() => {
      const ta = bodyRef.current;
      if (!ta) return;
      ta.focus();
      ta.setSelectionRange(start, start + q.length);
    });
  };

  // 選択範囲を切り取る（クリップボードへコピーしてから本文から除く）。
  const cutSelection = (start: number, end: number) => {
    const src = bodyRef.current?.value ?? body;
    void copyText(src.slice(start, end));
    setBody(src.slice(0, start) + src.slice(end));
    markDirty();
    requestAnimationFrame(() => {
      const ta = bodyRef.current;
      if (!ta) return;
      ta.focus();
      ta.setSelectionRange(start, start);
    });
  };

  // 送信中フラグ（ref）。送信直前に立て、保留中の自動保存が送信後に下書きを作らないようにする。
  const sendingRef = useRef(false);
  // 明示的に閉じた（保存/破棄/送信/通常クローズ）か。アンマウント時の確定保存を抑止し、
  // 破棄済みの下書きを復活させないために使う。
  const closedRef = useRef(false);

  // サーバー Drafts への同期状態（フッターに表示）。'idle' は非表示。
  const [syncState, setSyncState] = useState<'idle' | 'syncing' | 'done' | 'error'>('idle');
  const syncErrRef = useRef<string>('');

  // サーバーの Drafts フォルダへ同期する（成功/失敗をフッターに反映）。best-effort。
  const syncRemote = useCallback(async (id: number) => {
    // 送信中／送信後は Drafts へ APPEND しない。ここで APPEND すると、送信完了時の
    // 下書き削除（mail_draft_discard → delete_draft_remote）と競合し、「削除の後に
    // APPEND が勝つ」とサーバー Drafts に下書きが残ってしまう。すると次回同期で会話に
    // 「添付なしの重複メール」として復活する。
    if (sendingRef.current) return;
    setSyncState('syncing');
    try {
      await mailDraftSyncRemote(id);
      setSyncState('done');
    } catch (e) {
      syncErrRef.current = String(e);
      setSyncState('error');
    }
  }, []);

  // 現在の内容で下書きを保存/更新する（自動保存と、閉じる時の確定保存で共有）。
  // `syncNow` が真なら保存後すぐサーバー Drafts へ同期する（明示保存ボタン用）。
  // 自動保存はローカルのみに留める（サーバー Drafts へは「明示保存」と「閉じる時」だけ
  // APPEND する）。入力のたびに APPEND すると、書いている間ずっとサーバー上に下書きの
  // 入れ替え（削除→APPEND）が走り、送信時の後片付けと競合してゴミが残りやすくなるため。
  // 何も書いていない新規は下書きを作らない（空の下書きがフォルダに溜まるのを防ぐ）。
  const saveDraft = useCallback(
    async (syncNow = false) => {
      if (accountId == null || sendingRef.current) return;
      if (draftIdRef.current == null && !hasContent()) return;
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
          // 添付も一緒に保存する（転送で引き継いだ添付が、再編集で消えないように）。
          attachments: toDraftAttachments(attachments),
        });
        draftIdRef.current = id;
        onDraftId?.(id); // 復元用: 現在編集中の下書き id を親へ通知
        setSaved(true);
        setUnsaved(false);
        if (syncNow) {
          void syncRemote(id); // 明示保存: 即サーバー同期（自動保存はローカルのみ）
        }
      } catch {
        // 自動保存の失敗は致命的でないので黙って無視（次の入力で再試行）。
      }
    },
    [
      accountId,
      to,
      cc,
      bcc,
      subject,
      composedBody,
      init.inReplyTo,
      hasContent,
      syncRemote,
      onDraftId,
      attachments,
    ]
  );

  // 再編集で開いた下書きは、開いた時点でその id を親へ通知しておく（ビュー切替後に復元できるよう）。
  useEffect(() => {
    if (target.mode === 'draft') onDraftId?.(target.draft.id);
    // 開いた対象が変わった時だけ通知する。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target]);

  // 入力のたびにデバウンス（1s）して自動保存（ローカル）。dirty になって初めて走る。
  // 自動保存がオフのときは走らせない（閉じる確認ダイアログでのみ保存する）。
  useEffect(() => {
    if (!dirty || !autoSave) return;
    const h = setTimeout(() => void saveDraft(), 1000);
    return () => clearTimeout(h);
  }, [dirty, autoSave, saveDraft]);

  // 最新の saveDraft を参照する箱（アンマウント時の確定保存に使う。ref なので常に最新を指す）。
  const saveDraftRef = useRef(saveDraft);
  saveDraftRef.current = saveDraft;
  // 明示的に閉じる。アンマウント時の確定保存フラグを立ててから親へ通知する。
  const close = useCallback(() => {
    closedRef.current = true;
    onClose();
  }, [onClose]);
  // 作成画面がビュー切替（カレンダー等）で畳まれてアンマウントされる時、保留中の自動保存
  // （1s デバウンス）が破棄されて書きかけが消えるのを防ぐため、最後にもう一度保存する。
  // これで「返信を書きかけのままカレンダーへ移動→戻る」でも下書き id が確定して復元できる。
  // 明示的に閉じた/破棄した場合（closedRef）はここでは保存しない。
  useEffect(() => {
    return () => {
      if (closedRef.current) return; // 明示的に閉じた/破棄した
      if (!dirtyRef.current) return; // ユーザーが何も編集していない（未タッチの返信・StrictMode の疑似アンマウント）
      void saveDraftRef.current();
    };
  }, []);

  // 閉じる時、未保存の変更があれば確認ダイアログを出す（保存して閉じる / 破棄 / 編集に戻る）。
  // 未保存が無く自動保存済みの下書きがあれば、サーバー Drafts へ背景同期してそのまま閉じる。
  const closeGuarded = () => {
    if (unsaved) {
      // 何も書いておらず、まだ下書きも作っていないなら、確認せずそのまま閉じる。
      if (!hasContent() && draftIdRef.current == null) {
        close();
        return;
      }
      setConfirmClose(true);
      return;
    }
    if (draftIdRef.current != null) {
      void mailDraftSyncRemote(draftIdRef.current).catch(() => undefined);
    }
    close();
  };
  // ダイアログ「保存して閉じる」: 最新を確定保存し、サーバー Drafts へ背景同期して閉じる。
  const saveAndClose = async () => {
    await saveDraft();
    if (draftIdRef.current != null) {
      // サーバー同期は待たない（IMAP 往復で閉じるのを遅らせない）。
      void mailDraftSyncRemote(draftIdRef.current).catch(() => undefined);
    }
    setConfirmClose(false);
    close();
  };
  // ダイアログ「破棄」: 自動保存済みの下書きがあれば削除して閉じる（未保存の入力は捨てる）。
  const discardAndClose = async () => {
    closedRef.current = true; // 破棄後にアンマウント保存で復活させない
    if (draftIdRef.current != null) {
      try {
        await mailDraftDiscard(draftIdRef.current); // ローカルは即時・サーバーは背景で削除
      } catch {
        // 破棄の失敗は致命的でないので無視
      }
    }
    setConfirmClose(false);
    close();
  };
  // 確認ダイアログ表示中は Esc で「編集に戻る」（キャンセル）。
  useEffect(() => {
    if (!confirmClose) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        setConfirmClose(false);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [confirmClose]);

  // 送信アニメーション（つばめ）を使うか（設定・既定オン）。開いた時点の値を採用。
  const [flyOn] = useState(getFlyAnimation);
  const flyRef = useRef<FlySwallowHandle>(null);
  const sendBtnRef = useRef<HTMLButtonElement>(null);

  const attachTotal = attachments.reduce((s, a) => s + a.size, 0);
  const attachTooBig = attachTotal > MAX_ATTACH_TOTAL;
  const canSend =
    accountId != null && splitAddresses(to).length > 0 && !sending && !attachTooBig;

  // 転送: 元メールの添付をすべて引き継いで添付欄に載せる。埋め込み画像（cid:）も含めるのは、
  // 引用 HTML では cid: を解決できずプレースホルダに置き換わる（＝そのままでは相手に届かない）
  // ため。落とすより添付として渡すほうが安全で、不要なものはユーザーが個別に外せる。
  // ※ 埋め込み画像を「埋め込みのまま」転送する（multipart/related で Content-ID ごと同梱）のは
  //   送信側の MIME 組み立ての拡張が要るため後続。
  // この時点では本体を取りに行かず、送信/下書き保存の直前にローカルへ用意する（開いただけで
  // 大きな添付をサーバーから取らないため）。
  useEffect(() => {
    if (target.mode !== 'forward') return;
    let alive = true;
    mailAttachments(target.source.id)
      .then((list) => {
        if (!alive || list.length === 0) return;
        addAttachments(list.map((a) => ({ sourceId: a.id, name: a.filename, size: a.size })));
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
    // 転送対象が変わったときだけ引き継ぐ（addAttachments は同一内容の再追加を弾く）。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target]);

  // 添付を一覧へ追加する（同名・同サイズは重複とみなして除外。バッチ内の重複も除く）。
  const dedupKey = (a: Attach) => `${a.name}|${a.size}`;
  const addAttachments = (items: Attach[]) =>
    setAttachments((prev) => {
      const seen = new Set(prev.map(dedupKey));
      const add: Attach[] = [];
      for (const it of items) {
        const k = dedupKey(it);
        if (seen.has(k)) continue;
        seen.add(k);
        add.push(it);
      }
      return [...prev, ...add];
    });
  const removeAttachment = (key: string) =>
    setAttachments((prev) => prev.filter((a) => attachKey(a) !== key));

  // 添付を既定アプリで開いて送信前に確認する。転送で引き継いだ添付はまだ手元に無いことが
  // あるので、その場でローカルへ用意してから開く（用意できたら以後は手元のファイル扱い）。
  const openAttachment = async (a: Attach) => {
    try {
      let path = a.path;
      if (!path && a.sourceId != null) {
        const src = a.sourceId;
        const meta = await withActivity(t('activity.preparingAttachments'), () =>
          attachmentLocalPath(src),
        );
        path = meta.path;
        setAttachments((prev) =>
          prev.map((x) => (attachKey(x) === attachKey(a) ? { ...x, path } : x)),
        );
      }
      if (path) await openLocalPath(path);
    } catch (e) {
      setError(String(e));
    }
  };

  // ファイル選択ダイアログから添付を追加する。
  const pickAttachments = async () => {
    const picked = await open({ multiple: true }).catch(() => null);
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    const metas = await attachmentMeta(paths).catch(() => []);
    addAttachments(metas.map((m) => ({ path: m.path, name: m.name, size: Number(m.size) })));
  };

  // OS からドロップされたファイルを一時退避して添付に追加する（パスが取れないため中身を渡す）。
  const onDropFiles = async (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    const files = Array.from(e.dataTransfer.files);
    if (files.length === 0) return;
    const existing = new Set(attachments.map((a) => `${a.name}|${a.size}`));
    const staged: Attach[] = [];
    for (const f of files) {
      if (existing.has(`${f.name}|${f.size}`)) continue; // 既に同じものがあれば退避もしない
      try {
        const m = await attachmentStage(f.name, await f.arrayBuffer());
        staged.push({ path: m.path, name: m.name, size: Number(m.size) });
      } catch {
        /* この1件はスキップ（他は続行） */
      }
    }
    addAttachments(staged);
  };

  const onSend = async () => {
    if (accountId == null) return;
    sendingRef.current = true;
    setSending(true);
    setError('');
    // 転送で引き継いだ添付は、まだ手元に無ければここでローカルへ用意する（サーバーから
    // 取り直す＝時間がかかるのでフッターに進捗を出す。1 件ずつ順に取る）。
    let paths: string[];
    try {
      paths = await withActivity(t('activity.preparingAttachments'), () =>
        resolveAttachmentPaths(attachments),
      );
    } catch (e) {
      setError(String(e));
      sendingRef.current = false;
      setSending(false);
      return;
    }
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
      attachments: paths,
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
      close();
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
    <div
      className="relative flex h-full min-h-0 flex-col overflow-hidden"
      onDragOver={(e) => {
        e.preventDefault();
        if (!dragOver) setDragOver(true);
      }}
      onDragLeave={(e) => {
        // 子要素へ移っただけのときは維持し、コンテナの外へ出たときだけ解除する。
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragOver(false);
      }}
      onDrop={onDropFiles}
    >
      {dragOver && (
        <div className="pointer-events-none absolute inset-0 z-30 flex items-center justify-center rounded-lg border-2 border-dashed border-sky-400/70 bg-sky-500/10 backdrop-blur-sm">
          <div className="flex items-center gap-2 rounded-lg bg-neutral-900/70 px-4 py-2 text-sm font-medium text-sky-100 ring-1 ring-white/15">
            <Paperclip size={16} />
            {t('compose.dropHint')}
          </div>
        </div>
      )}
      <div className="flex items-center justify-between border-b border-white/10 px-4 py-2.5">
        <h2 className="text-sm font-semibold">{t(`compose.${target.mode}`)}</h2>
        <div className="flex items-center gap-1">
          {/* 下書きを今すぐ保存＋サーバーの Drafts へも同期（本文等があるときに有効）。 */}
          <button
            type="button"
            onClick={() => void saveDraft(true)}
            disabled={sending || accountId == null || !hasContent() || syncState === 'syncing'}
            title={t('compose.saveDraft')}
            aria-label={t('compose.saveDraft')}
            className="flex h-7 w-7 items-center justify-center rounded-md text-white/55 hover:bg-white/10 hover:text-white/85 disabled:opacity-40"
          >
            <Save size={16} />
          </button>
          {/* 下書きを破棄（ローカル＋サーバーから削除）して閉じる。 */}
          <button
            type="button"
            onClick={() => void discardAndClose()}
            disabled={sending}
            title={t('compose.discard')}
            aria-label={t('compose.discard')}
            className="flex h-7 w-7 items-center justify-center rounded-md text-white/55 hover:bg-rose-500/20 hover:text-rose-300 disabled:opacity-40"
          >
            <Trash2 size={16} />
          </button>
          <button
            onClick={closeGuarded}
            disabled={sending}
            className="flex h-7 w-7 items-center justify-center rounded-md text-white/55 hover:bg-white/10 hover:text-white/85 disabled:opacity-40"
            aria-label={t('account.cancel')}
          >
            <X size={16} />
          </button>
        </div>
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
                  key={attachKey(a)}
                  // クリックで送信前に既定アプリで内容確認（未取得の転送添付はここで取り寄せる）。
                  title={`${a.name}（${formatSize(a.size)}）・${t('compose.attachOpenHint')}`}
                  onClick={() => void openAttachment(a)}
                  className="inline-flex max-w-[16rem] cursor-pointer select-none items-center gap-1.5 rounded-md bg-white/10 py-1 pl-2 pr-1 text-xs hover:bg-white/15"
                >
                  <Paperclip size={12} className="shrink-0 text-white/50" />
                  <span className="min-w-0 flex-1 truncate">{a.name}</span>
                  <span className="shrink-0 text-white/40">{formatSize(a.size)}</span>
                  <button
                    type="button"
                    // 「外す」は開く動作を巻き込まないよう伝播を止める。
                    onClick={(e) => {
                      e.stopPropagation();
                      removeAttachment(attachKey(a));
                    }}
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
              onChange={(e) => chooseSignature(e.target.value ? Number(e.target.value) : null)}
              title={t('compose.signatureRemembered')}
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

        {/* 本文（引用は編集欄に入れず、送信時に付ける）。ペインの残り高さいっぱいに広げる。
            文字選択中の右クリックは「引用文にする」等の小メニューに差し替える（選択が無ければ
            貼り付け等のネイティブメニューを残す）。 */}
        <textarea
          ref={bodyRef}
          className="min-h-[10rem] w-full flex-1 resize-none rounded-md bg-white/10 px-3 py-2 text-sm leading-relaxed outline-none placeholder:text-white/30 focus:bg-white/15"
          value={body}
          onChange={(e) => {
            setBody(e.target.value);
            markDirty();
          }}
          onContextMenu={(e) => {
            const ta = e.currentTarget;
            const start = ta.selectionStart;
            const end = ta.selectionEnd;
            if (end > start) {
              e.preventDefault();
              setBodyMenu({ x: e.clientX, y: e.clientY, start, end });
            }
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
        {/* 下書き保存/破棄はヘッダのアイコンボタンへ集約（重複を避ける）。
            状態表示: 送信エラー > サーバー同期失敗 > 未保存 > 同期中/同期済み > 保存済み。 */}
        {error ? (
          <span className="flex-1 truncate text-xs text-rose-300">{error}</span>
        ) : syncState === 'error' ? (
          <span className="flex-1 truncate text-xs text-rose-300" title={syncErrRef.current}>
            {t('compose.draftSyncFailed')}
          </span>
        ) : (
          <span className="flex-1 truncate text-xs text-white/40">
            {unsaved
              ? t('compose.unsavedShort')
              : syncState === 'syncing'
                ? t('compose.draftSyncing')
                : syncState === 'done'
                  ? t('compose.draftSynced')
                  : saved
                    ? t('compose.draftSaved')
                    : ''}
          </span>
        )}
      </div>

      {/* 未保存の変更があるまま閉じようとしたときの確認ダイアログ（保存/破棄/編集に戻る）。 */}
      {confirmClose && (
        <div className="absolute inset-0 z-40 flex items-center justify-center bg-black/50 p-4">
          <div className="w-full max-w-sm rounded-lg bg-neutral-900/95 p-4 shadow-xl ring-1 ring-white/10">
            <p className="mb-1 text-sm font-semibold text-white">{t('compose.unsavedTitle')}</p>
            <p className="mb-4 text-sm text-white/70">{t('compose.unsavedMessage')}</p>
            <div className="flex flex-wrap justify-end gap-2 text-sm">
              <button
                onClick={() => setConfirmClose(false)}
                className="rounded-md px-3 py-1.5 text-white/70 hover:bg-white/10"
              >
                {t('compose.unsavedCancel')}
              </button>
              <button
                onClick={() => void discardAndClose()}
                className="rounded-md px-3 py-1.5 text-rose-300 hover:bg-rose-500/20"
              >
                {t('compose.unsavedDiscard')}
              </button>
              <button
                onClick={() => void saveAndClose()}
                className="rounded-md bg-sky-500/90 px-3 py-1.5 font-medium text-white hover:bg-sky-500"
              >
                {t('compose.unsavedSave')}
              </button>
            </div>
          </div>
        </div>
      )}

      {flyOn && <FlySwallow ref={flyRef} />}

      {bodyMenu && (
        <ContextMenu
          x={bodyMenu.x}
          y={bodyMenu.y}
          items={[
            {
              key: 'quote',
              label: t('compose.makeQuote'),
              Icon: Quote,
              onClick: () => quoteSelection(bodyMenu.start, bodyMenu.end),
            },
            {
              key: 'copy',
              label: t('ctx.copy'),
              Icon: Copy,
              onClick: () =>
                void copyText((bodyRef.current?.value ?? body).slice(bodyMenu.start, bodyMenu.end)),
            },
            {
              key: 'cut',
              label: t('ctx.cut'),
              Icon: Scissors,
              onClick: () => cutSelection(bodyMenu.start, bodyMenu.end),
            },
          ]}
          onClose={() => setBodyMenu(null)}
        />
      )}
    </div>
  );
}
