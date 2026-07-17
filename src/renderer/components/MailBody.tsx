import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ask, open } from '@tauri-apps/plugin-dialog';
import { downloadDir, join } from '@tauri-apps/api/path';
import {
  BadgeCheck,
  BookOpen,
  CalendarPlus,
  ChevronDown,
  Download,
  Forward,
  Gem,
  Image as ImageIcon,
  ImageOff,
  LeafyGreen,
  Paperclip,
  Pencil,
  Plus,
  RefreshCw,
  Reply,
  ReplyAll,
  Star,
  Tag,
  ThumbsDown,
  ThumbsUp,
  X,
} from 'lucide-react';
import type { MailDetail } from '@bindings/MailDetail';
import type { AttachmentSummary } from '@bindings/AttachmentSummary';
import type { TagSummary } from '@bindings/TagSummary';
import { DEFAULT_TAG_COLOR } from '../utils/tagColors';
import {
  attachmentExport,
  attachmentOpen,
  attachmentView,
  mailAttachments,
  mailRefetch,
} from '../services/mail';
import type { ContactSummary } from '@bindings/ContactSummary';
import { getInlineImages, getRemoteImageMode, PREFS_EVENT } from '../config/prefs';
import { greenDomainAdd, greenDomainWarn } from '../services/green';
import { contactLookupEmail } from '../services/contacts';
import { mailLoadRemote, senderRemoteAllowed, senderSetRemotePolicy } from '../services/mail';
import { AutoLinkText, HtmlText, remoteImageUrls } from './HtmlText';
import { ContextMenu } from './ContextMenu';
import type { CalendarPanelInitial } from './CalendarPanel';
import { parseDateTime, type ParsedDate } from '../utils/dateparse';
import { formatDateTime } from '../utils/datetime';
import { saveAttachment } from '../utils/attachmentSave';
import { withActivity } from '../stores/activity';

/** 「表示名 <メール>」に整形。表示名が無ければアドレスのみ。 */
function formatAddress(name: string | null, address: string | null): string {
  const a = (address ?? '').trim();
  const n = (name ?? '').trim();
  if (!a) return '—';
  return n ? `${n} <${a}>` : a;
}

/** 本文/ヘッダ中のメールアドレス検出。 */
const EMAIL_RE = /([A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,})/g;

/** メールアドレスごとの住所録照合キャッシュ（同一アドレスの重複ルックアップを避ける）。
 *  住所録が変わり得るので、メール画面を離れるとき（MailBody アンマウント）にクリアする。 */
const emailLookupCache = new Map<string, Promise<ContactSummary[]>>();
function lookupEmailCached(email: string): Promise<ContactSummary[]> {
  const key = email.trim().toLowerCase();
  let p = emailLookupCache.get(key);
  if (!p) {
    p = contactLookupEmail(email).catch(() => [] as ContactSummary[]);
    emailLookupCache.set(key, p);
  }
  return p;
}

/** 氏名の突き合わせ用の正規化（全半角統一・空白除去・小文字化）。 */
const foldName = (s?: string | null) =>
  (s ?? '').normalize('NFKC').replace(/\s+/g, '').toLowerCase();

/**
 * メールアドレス＋（ホバー/フォーカスで現れる）操作ボタン。
 * 住所録に未登録＝＋（追加）、登録済み＝編集アイコン。重複（複数登録）があれば件数を黄色字で表示。
 */
function EmailAdd({
  email,
  name,
  onAdd,
  onEdit,
  onCompose,
}: {
  email: string;
  name?: string | null;
  onAdd: (name: string | null, email: string) => void;
  onEdit?: (id: number) => void;
  /** アドレスのクリックでこのアドレス宛の新規メール作成を開く。 */
  onCompose?: (email: string) => void;
}) {
  const { t } = useTranslation();
  const [matches, setMatches] = useState<ContactSummary[] | null>(null);
  useEffect(() => {
    let alive = true;
    lookupEmailCached(email).then((r) => alive && setMatches(r));
    return () => {
      alive = false;
    };
  }, [email]);

  const count = matches?.length ?? 0;
  const registered = count > 0;
  const dup = count > 1;

  // 差出人名に「関連する」既存を優先（完全一致・空白差のほか、姓のみ/名のみ等の部分一致）。
  // 同じ代表アドレスが別人（前任）に紐づくときの取り違えを防ぐ。
  const wantName = foldName(name);
  const named =
    matches && wantName
      ? (matches.find((m) => {
          const dn = foldName(m.display_name);
          return dn === wantName || dn.includes(wantName) || wantName.includes(dn);
        }) ?? null)
      : null;
  // クリック動作: 名前が関連する既存があれば編集。差出人名が無ければ（判別できないので）
  // 従来どおり編集。差出人名があるのに関連する既存が無い＝別人とみなして新規追加にする
  // （アドレスが 1 件だけ一致でも、名前が違えば開かず新規登録にする）。
  const willEdit = registered && !!onEdit && (named != null || !wantName);

  const handle = () => {
    if (willEdit && matches && onEdit) onEdit((named ?? matches[0]).id);
    else onAdd(name ?? null, email);
  };
  const title = willEdit
    ? dup
      ? t('mailbox.editContactDup', { count })
      : t('mailbox.editContact')
    : t('mailbox.addContact');

  return (
    <span className="group/email inline-flex items-center gap-0.5 align-baseline">
      {onCompose ? (
        <button
          type="button"
          onClick={() => onCompose(email)}
          title={t('mailbox.composeTo', { email })}
          className="cursor-pointer rounded hover:text-sky-300 hover:underline focus:outline-none focus:text-sky-300"
        >
          {email}
        </button>
      ) : (
        <span>{email}</span>
      )}
      <button
        onClick={handle}
        title={title}
        aria-label={title}
        className="inline-flex h-4 items-center justify-center gap-0.5 rounded-full bg-white/10 px-1 text-white/60 opacity-0 transition-opacity hover:bg-sky-500/50 hover:text-white focus:opacity-100 focus:outline-none group-hover/email:opacity-100"
      >
        {willEdit ? <Pencil size={11} /> : <Plus size={11} />}
        {dup && <span className="text-[9px] font-semibold leading-none text-amber-300">{count}</span>}
      </button>
    </span>
  );
}

/**
 * 本文中の日付＋（ホバー/フォーカスで現れる）カレンダー追加ボタン。
 * ＋を押すと右ペインのカレンダー入力が、この日時をプレフィルして開く。
 */
function DateAdd({
  raw,
  parsed,
  onAdd,
}: {
  raw: string;
  parsed: ParsedDate;
  onAdd: (parsed: ParsedDate) => void;
}) {
  const { t } = useTranslation();
  return (
    <span className="group/date inline-flex items-center gap-0.5 align-baseline">
      <span>{raw}</span>
      <button
        type="button"
        onClick={(e) => {
          // クリックが親（バブルの展開トグル等）へ伝播しないよう止める。
          e.stopPropagation();
          onAdd(parsed);
        }}
        title={t('cal.addFromDate')}
        aria-label={t('cal.addFromDate')}
        className="inline-flex h-4 items-center justify-center rounded-full bg-white/10 px-1 text-white/60 opacity-0 transition-opacity hover:bg-sky-500/50 hover:text-white focus:opacity-100 focus:outline-none group-hover/date:opacity-100"
      >
        <CalendarPlus size={11} />
      </button>
    </span>
  );
}

/**
 * 本文中の日付を「日付＋＋ボタン」に描画する関数を作る（HtmlText/AutoLinkText の renderDate 用）。
 * 会話バブルと全文表示（MailBody）で共有し、＋の挙動を揃える。
 * baseISO（メール受信日）で年を補い、件名・元メール id をプレフィルに渡す。
 */
export function makeRenderDate(
  onAddCalendar: ((init: CalendarPanelInitial) => void) | undefined,
  ctx: { baseISO?: string; title?: string; relatedEmailId?: number },
): ((raw: string) => ReactNode) | undefined {
  if (!onAddCalendar) return undefined;
  // これはコンポーネントではなく描画関数（renderProp）。display-name 規則は当てはまらない。
  // eslint-disable-next-line react/display-name
  return (raw: string) => {
    const parsed = parseDateTime(raw, ctx.baseISO);
    if (!parsed) return <>{raw}</>;
    return (
      <DateAdd
        raw={raw}
        parsed={parsed}
        onAdd={(p) =>
          onAddCalendar({
            day: p.day,
            time: p.time,
            allDay: p.allDay,
            title: ctx.title,
            relatedEmailId: ctx.relatedEmailId,
          })
        }
      />
    );
  };
}

/** ヘッダの差出人/宛先。onAdd があればアドレスに＋/編集を出す（無ければ従来のテキスト）。 */
function AddressLine({
  name,
  address,
  onAdd,
  onEdit,
  onCompose,
}: {
  name: string | null;
  address: string | null;
  onAdd?: (name: string | null, email: string) => void;
  onEdit?: (id: number) => void;
  onCompose?: (email: string) => void;
}) {
  const a = (address ?? '').trim();
  const n = (name ?? '').trim();
  if (!a) return <>—</>;
  if (!onAdd) return <>{formatAddress(name, address)}</>;
  return (
    <span className="inline-flex max-w-full items-center gap-1">
      {n && <span className="truncate">{n} &lt;</span>}
      <EmailAdd email={a} name={n || null} onAdd={onAdd} onEdit={onEdit} onCompose={onCompose} />
      {n && <span>&gt;</span>}
    </span>
  );
}

/** プレーン本文中のメールアドレスをリンク化し、それぞれに＋/編集を出す。 */
function LinkifyEmails({
  text,
  onAdd,
  onEdit,
  onCompose,
}: {
  text: string;
  onAdd?: (name: string | null, email: string) => void;
  onEdit?: (id: number) => void;
  onCompose?: (email: string) => void;
}) {
  if (!onAdd) return <>{text}</>;
  const parts = text.split(EMAIL_RE);
  return (
    <>
      {parts.map((p, i) =>
        i % 2 === 1 ? (
          <EmailAdd key={i} email={p} onAdd={onAdd} onEdit={onEdit} onCompose={onCompose} />
        ) : (
          <span key={i}>{p}</span>
        ),
      )}
    </>
  );
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

const IMAGE_EXT = /\.(jpe?g|png|gif|webp|bmp|tiff?|heic|heif|avif)$/i;

/** 画像（変換すれば表示できる HEIC 等を含む）かどうか。 */
function isImage(a: AttachmentSummary): boolean {
  if (a.content_type?.toLowerCase().startsWith('image/')) return true;
  return IMAGE_EXT.test(a.filename);
}

/**
 * メール本文の表示（インライン）。Phase: プレーン本文のみ（HTML/リモート画像は
 * 後続でサニタイズ＋ブロック。docs/MAIL_SECURITY.md）。既定は引用除去後の clean_body。
 */
export function MailBody({
  detail,
  tags,
  starred,
  onToggleStar,
  onTag,
  onRemoveTag,
  onReply,
  onMarkSpam,
  onMarkNotSpam,
  onAddContact,
  onEditContact,
  onComposeTo,
  onAddCalendar,
  onOpenCalendar,
  onGreenChange,
  highlight,
  onCollapse,
}: {
  detail: MailDetail;
  /** このメールに付いているタグ（ヘッダの宛先の下に表示）。 */
  tags?: TagSummary[];
  /** このメールにスターが付いているか。 */
  starred?: boolean;
  /** スターの切り替え。 */
  onToggleStar?: () => void;
  /** タグ付与ポップオーバーを開く（ボタンの画面座標を渡す）。 */
  onTag?: (x: number, y: number) => void;
  /** このメールからタグを外す（チップの × で呼ぶ）。 */
  onRemoveTag?: (tagId: number) => void;
  onReply?: (mode: 'reply' | 'replyAll' | 'forward') => void;
  /** 迷惑としてマーク（学習＋隔離）。 */
  onMarkSpam?: () => void;
  /** 非迷惑に戻す（隔離解除＋ham 学習）。迷惑フォルダ表示時に onMarkSpam の代わりに出す。 */
  onMarkNotSpam?: () => void;
  /** ヘッダ/本文のメールアドレスから住所録へ追加（名前・メールを渡す）。 */
  onAddContact?: (name: string | null, email: string) => void;
  /** 登録済みアドレスの編集アイコンから、その連絡先を開く。 */
  onEditContact?: (id: number) => void;
  /** ヘッダ/本文のメールアドレスのクリックで、そのアドレス宛の新規メール作成を開く。 */
  onComposeTo?: (email: string) => void;
  /** 本文中の日付の＋から、日時をプレフィルしてカレンダー入力（右ペイン）を開く。 */
  onAddCalendar?: (init: CalendarPanelInitial) => void;
  /** ヘッダの「カレンダーに追加」から、このメールに紐づくカレンダー入力を開く。 */
  onOpenCalendar?: () => void;
  /** グリーン認定/解除で一覧のバッジを更新するための通知。 */
  onGreenChange?: () => void;
  /** 検索語（複数）。本文中の一致を <mark> でハイライトする。 */
  highlight?: string[];
  /** ヘッダに「とじる（畳む）」ボタンを出す。会話バブルの展開解除に使う。 */
  onCollapse?: () => void;
}) {
  const { t } = useTranslation();
  const [showQuotes, setShowQuotes] = useState(false);
  const [note, setNote] = useState('');
  // 全文再取得（要約保存の解除）の結果でこのメールだけ本文を差し替える。
  const [refreshed, setRefreshed] = useState<MailDetail | null>(null);
  const [refetching, setRefetching] = useState(false);
  // グリーン認定/解除の即時反映（メール切替でリセット）。null＝props の is_green を使う。
  const [greenOverride, setGreenOverride] = useState<boolean | null>(null);
  // 表示に使う本文（再取得済みがあればそれ、無ければ props の detail）。
  const d = refreshed ?? detail;
  const isGreen = greenOverride ?? d.is_green;
  const senderDomain = (d.from_address ?? '').includes('@')
    ? (d.from_address ?? '').split('@').pop()?.trim().toLowerCase() || ''
    : '';
  // Reply-To（返信先指定）が From と別のときだけヘッダに出す（返信がこちらへ飛ぶことを明示）。
  const replyToAddr = ((d.reply_to ?? '').match(/<([^>]+)>/)?.[1] ?? d.reply_to ?? '')
    .trim()
    .toLowerCase();
  const showReplyTo =
    !!d.reply_to?.trim() && replyToAddr !== (d.from_address ?? '').trim().toLowerCase();
  const [attachments, setAttachments] = useState<AttachmentSummary[]>([]);
  const [busyId, setBusyId] = useState<number | null>(null);
  // 本文埋め込み画像（content_id → data URL）
  const [inlineImages, setInlineImages] = useState<Record<string, string>>({});
  // 許可して取得したリモート画像（正規化 URL → サニタイズ済み data URL）。既定は空＝ブロック。
  const [remoteImages, setRemoteImages] = useState<Record<string, string>>({});
  // この差出人が「常に許可」済みか（許可解除ボタンの出し分け・自動表示の判定に使う）。
  const [senderAllowed, setSenderAllowed] = useState(false);
  // 外部画像を表示するか（アイコンで on/off）。既定はグローバル設定（非表示なら false）。
  const [remoteShown, setRemoteShown] = useState(() => getRemoteImageMode() !== 'hidden');
  // 画像の初期サイズを完全表示にするか（グローバル既定が「完全」のとき）。各画像はクリックで切替。
  const [remoteExpandDefault, setRemoteExpandDefault] = useState(
    () => getRemoteImageMode() === 'full',
  );
  // このメールで取得を一度試みたか（空結果でも再取得しない。メール切替でリセット）。
  // state ではなく ref にする理由: 依存配列に入れると setState で effect が自己再実行し、
  // クリーンアップが in-flight の取得を中断して「読み込み中」で固まるため（StrictMode 二重実行対策込み）。
  const remoteAttemptedRef = useRef(false);
  // 外部画像アイコンの右クリックメニュー位置（許可の付与/解除・表示切替）。
  const [remoteMenu, setRemoteMenu] = useState<{ x: number; y: number } | null>(null);
  // 添付画像のアプリ内プレビュー（attachment id → data URL）
  const [previews, setPreviews] = useState<Record<number, string>>({});
  const [inlineEnabled, setInlineEnabled] = useState(getInlineImages());
  // 添付セクションの開閉（既定は閉じる）
  const [attachmentsOpen, setAttachmentsOpen] = useState(false);
  // チェックした添付（まとめて保存用）
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [savingAll, setSavingAll] = useState(false);
  const [attachmentsLoaded, setAttachmentsLoaded] = useState(false);

  // メール切替をレンダー中に検知してリモート画像の状態を即リセットする。effect だと 1 コミット
  // 遅れ、前のメールが「表示中」だと新メールの外部画像を誤って取得＝トラッキング漏れになるため、
  // 取得を判断する remoteShown 等はレンダー中（effect 前）に確定させる。
  const [remoteMailId, setRemoteMailId] = useState(detail.id);
  if (remoteMailId !== detail.id) {
    setRemoteMailId(detail.id);
    setRemoteImages({});
    remoteAttemptedRef.current = false;
    setSenderAllowed(false);
    setRemoteShown(getRemoteImageMode() !== 'hidden');
    setRemoteExpandDefault(getRemoteImageMode() === 'full');
  }

  // 設定（インライン画像の自動取得）の変更に追従する。
  useEffect(() => {
    const onPrefs = () => setInlineEnabled(getInlineImages());
    window.addEventListener(PREFS_EVENT, onPrefs);
    return () => window.removeEventListener(PREFS_EVENT, onPrefs);
  }, []);

  // メール画面を離れるときに住所録照合キャッシュをクリア（連絡先の追加/編集を次回反映）。
  useEffect(() => () => emailLookupCache.clear(), []);

  // メール切り替えごとに添付メタを読み込む（本体は押下時に取得）。
  useEffect(() => {
    let active = true;
    setAttachments([]);
    setInlineImages({});
    setPreviews({});
    setAttachmentsOpen(false);
    setSelected(new Set());
    setAttachmentsLoaded(false);
    setRefreshed(null);
    setGreenOverride(null);
    // インライン画像（cid:）は has_attachments に数えないため（例: 本文が画像 1 枚だけの
    // multipart/related）、本文が cid: を参照するときも添付メタを読み込む。読まないと
    // 本文中のインライン画像を解決できず、プレースホルダのまま何も表示されない。
    const needsInline = (detail.body_html ?? '').toLowerCase().includes('cid:');
    if (detail.has_attachments || needsInline) {
      mailAttachments(detail.id)
        .then((a) => {
          if (active) {
            setAttachments(a);
            setAttachmentsLoaded(true);
          }
        })
        .catch(() => active && setAttachmentsLoaded(true));
    } else {
      setAttachmentsLoaded(true);
    }
    return () => {
      active = false;
    };
  }, [detail.id, detail.has_attachments, detail.body_html]);

  const hasHtmlBody = (d.body_html?.trim()?.length ?? 0) > 0;

  // HTML 本文＋設定オンのとき、インライン画像を取得して cid マップを作る。
  useEffect(() => {
    let active = true;
    if (!hasHtmlBody || !inlineEnabled) return;
    const targets = attachments.filter((a) => a.kind === 'inline' && a.content_id && isImage(a));
    targets.forEach((a) => {
      attachmentView(a.id)
        .then((url) => {
          if (active && a.content_id) {
            setInlineImages((m) => ({ ...m, [a.content_id as string]: url }));
          }
        })
        .catch(() => {});
    });
    return () => {
      active = false;
    };
  }, [attachments, hasHtmlBody, inlineEnabled]);

  // 本文（HTML）に含まれる外部画像 URL。モード切替と一括取得に使う。
  const remoteUrls = useMemo(() => {
    const h = d.body_html?.trim() ?? '';
    return h ? remoteImageUrls(h) : [];
  }, [d.body_html]);

  // 「この差出人を常に許可」/ 解除。許可時はこのメールも表示 on にする。
  const setRemoteAllow = async (allow: boolean) => {
    const addr = (d.from_address ?? '').trim();
    if (!addr) return;
    try {
      await senderSetRemotePolicy(addr, allow);
      setSenderAllowed(allow);
      if (allow) setRemoteShown(true);
    } catch (e) {
      setNote(String(e));
    }
  };

  // アイコン左クリック: 表示中なら隠す。未表示なら、未許可の差出人はダイアログで許可方法を選ぶ。
  const onRemoteIconClick = async () => {
    if (remoteShown) {
      setRemoteShown(false);
      return;
    }
    // 許可済み（または差出人不明）は確認なしで表示する。
    if (senderAllowed || !(d.from_address ?? '').trim()) {
      setRemoteShown(true);
      return;
    }
    // 未許可の差出人: 「常に許可」か「この1通だけ」かをダイアログで選ぶ。
    let always = false;
    try {
      always = await ask(t('mailbox.remoteAllowAsk'), {
        title: t('mailbox.remoteAllowTitle'),
        kind: 'info',
        okLabel: t('mailbox.remoteAllowSender'),
        cancelLabel: t('mailbox.remoteShowOnce'),
      });
    } catch {
      /* 非 Tauri プレビュー等ではダイアログを出せないので、この1通だけ表示する */
    }
    if (always) await setRemoteAllow(true);
    else setRemoteShown(true);
  };

  // メールを開いたとき: 差出人の許可状態を確認し、許可済みなら（既定が非表示でも）表示 on にする。
  useEffect(() => {
    let active = true;
    const addr = (d.from_address ?? '').trim();
    if (remoteUrls.length === 0 || !addr) return;
    senderRemoteAllowed(addr)
      .then((ok) => {
        if (!active) return;
        setSenderAllowed(ok);
        if (ok) setRemoteShown(true);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [remoteUrls, d.from_address]);

  // 表示 on で未取得なら 1 度だけ取得する（on/off・メール切替に追従）。
  // 「試行済み」は ref（remoteAttemptedRef）で管理し、依存には入れない。試行フラグは取得完了後
  // （finally）に立てる → StrictMode の二重 mount でも取得が確実に 1 度は完走する。ignore で
  // 古い応答（メール切替後など）を破棄。setState は依存に無いので自己再実行しない＝固まらない。
  useEffect(() => {
    if (!remoteShown || remoteUrls.length === 0 || remoteAttemptedRef.current) return;
    let ignore = false;
    setNote('');
    // 差出人を渡す（バックエンドが許可済みなら data_dir にキャッシュする）。
    const sender = (d.from_address ?? '').trim() || null;
    mailLoadRemote(remoteUrls, sender)
      .then((imgs) => {
        if (ignore) return;
        const map: Record<string, string> = {};
        for (const it of imgs) map[it.url] = it.data_url;
        setRemoteImages(map);
      })
      .catch(() => undefined)
      .finally(() => {
        remoteAttemptedRef.current = true;
      });
    return () => {
      ignore = true;
    };
  }, [remoteShown, remoteUrls, d.from_address]);

  // 添付画像をアプリ内でプレビュー表示（トグル）。HEIC も JPEG 化して表示。
  const togglePreview = async (a: AttachmentSummary) => {
    if (previews[a.id]) {
      setPreviews((m) => {
        const next = { ...m };
        delete next[a.id];
        return next;
      });
      return;
    }
    setBusyId(a.id);
    setNote('');
    try {
      const url = await attachmentView(a.id);
      setPreviews((m) => ({ ...m, [a.id]: url }));
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusyId(null);
    }
  };

  // 「開く」: 未取得なら取得してから OS の関連アプリで開く（HEIC は変換して開く）。
  const handleOpen = async (a: AttachmentSummary) => {
    setBusyId(a.id);
    setNote('');
    try {
      // 取得に時間がかかることがあるのでフッターに進捗（不確定）を出す。
      await withActivity(t('activity.openingAttachment'), () => attachmentOpen(a.id));
      setAttachments((list) =>
        list.map((x) => (x.id === a.id ? { ...x, is_downloaded: true } : x)),
      );
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusyId(null);
    }
  };

  // 「ダウンロード」: 保存先を選び（既定はダウンロードフォルダ）、その場所へ保存する。
  // 保存ダイアログ＋書き出し＋フッター進捗は共通ヘルパ（saveAttachment）に集約している。
  const handleSave = async (a: AttachmentSummary) => {
    setNote('');
    setBusyId(a.id);
    try {
      const saved = await saveAttachment(a.id, a.filename, t('activity.downloadingAttachment'));
      if (saved) {
        // 保存時にローカルへも取得済みになる（開く/DLアイコンの表示を揃える）。
        setAttachments((list) =>
          list.map((x) => (x.id === a.id ? { ...x, is_downloaded: true } : x)),
        );
        setNote(t('mailbox.attachmentSaved'));
      }
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusyId(null);
    }
  };

  // 一覧に出すのは本来の添付のみ（inline 画像は本文側に表示）。
  const fileAttachments = attachments.filter((a) => a.kind !== 'inline');

  const toggleOne = (id: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const allSelected = fileAttachments.length > 0 && selected.size === fileAttachments.length;
  const someSelected = selected.size > 0 && !allSelected;
  const toggleAll = () => {
    setSelected(allSelected ? new Set() : new Set(fileAttachments.map((a) => a.id)));
  };

  // チェックした添付をまとめて、選んだフォルダへ保存する。
  const handleSaveSelected = async () => {
    let dir: string | null = null;
    try {
      const picked = await open({ directory: true, defaultPath: await downloadDir() });
      dir = typeof picked === 'string' ? picked : null;
    } catch {
      dir = null;
    }
    if (!dir) return;
    const folder = dir;
    setSavingAll(true);
    setNote('');
    const targets = fileAttachments.filter((a) => selected.has(a.id));
    let ok = 0;
    // まとめ保存中もフッターに進捗（不確定）を出す。
    await withActivity(t('activity.downloadingAttachment'), async () => {
      for (const a of targets) {
        try {
          await attachmentExport(a.id, await join(folder, a.filename));
          ok += 1;
        } catch {
          /* 個別の失敗はスキップ */
        }
      }
    });
    setNote(t('mailbox.attachmentSavedN', { count: ok }));
    setSavingAll(false);
  };

  // 全文をサーバーから再取得して要約保存を解除する（このメールだけ本文キャッシュを復元）。
  const handleRefetch = async () => {
    setRefetching(true);
    setNote('');
    try {
      const fresh = await mailRefetch(detail.id);
      setRefreshed(fresh);
    } catch (e) {
      setNote(String(e));
    } finally {
      setRefetching(false);
    }
  };

  // 未取得(absent)本文はメールを開いた時に自動でサーバから取得する（docs/SYNC.md §3.6：
  // 全件メタ索引で見出しだけ先に並べ、開いた本文はオンデマンド）。要約(evicted)は clean_body が
  // あるので自動取得せず、必要時に「全文を再取得」ボタンで取る。
  useEffect(() => {
    if (detail.body_state === 'absent' && !refreshed && !refetching) {
      void handleRefetch();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [detail.id, detail.body_state]);

  // 差出人ドメインをグリーン認定/解除（一覧のバッジは onGreenChange で更新）。
  const toggleGreen = async () => {
    if (!senderDomain) return;
    const next = !isGreen;
    try {
      if (next) await greenDomainAdd(senderDomain);
      else await greenDomainWarn(senderDomain);
      setGreenOverride(next);
      onGreenChange?.();
    } catch {
      /* noop */
    }
  };

  const COMPOSE_ACTIONS = [
    { key: 'reply', Icon: Reply },
    { key: 'replyAll', Icon: ReplyAll },
    { key: 'forward', Icon: Forward },
  ] as const;

  const clean = d.clean_body ?? '';
  const full = d.body_plain ?? '';
  const html = d.body_html?.trim() ?? '';
  const hasHtml = html.length > 0;
  const hasQuotedExtra = !hasHtml && full.trim().length > clean.trim().length;
  const body = showQuotes ? full : clean || full;

  // 本文中の日付に＋（カレンダー追加）を出す描画関数。年はメール受信日を基準に補う。
  const renderDate = makeRenderDate(onAddCalendar, {
    baseISO: d.date ?? undefined,
    title: d.subject ?? undefined,
    relatedEmailId: d.id,
  });

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* ヘッダ（件名・差出人・操作）。本文は下の body で内部スクロールするため、ヘッダは
          カード上部に据え置かれる（裏を本文が流れない＝カードと同じ透過のまま保てる）。 */}
      <div className="border-b border-white/10 px-5 py-3">
        <div className="flex items-start justify-between gap-3">
          <h3 className="min-w-0 truncate text-sm font-semibold">
            {d.subject ?? '(no subject)'}
          </h3>
          <div className="flex shrink-0 items-center gap-1">
            {/* スター切替 */}
            {onToggleStar && (
              <button
                onClick={onToggleStar}
                title={t(starred ? 'ctx.unstar' : 'ctx.star')}
                aria-label={t(starred ? 'ctx.unstar' : 'ctx.star')}
                aria-pressed={starred}
                className={`flex h-8 w-8 items-center justify-center rounded-md ${
                  starred ? 'text-amber-300' : 'text-white/55 hover:text-white/80'
                }`}
              >
                <Star size={16} className={starred ? 'fill-amber-300' : ''} />
              </button>
            )}
            {COMPOSE_ACTIONS.map(({ key, Icon }) => (
              <button
                key={key}
                onClick={() => onReply?.(key)}
                title={t(`compose.${key}`)}
                aria-label={t(`compose.${key}`)}
                className="flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80"
              >
                <Icon size={16} />
              </button>
            ))}
            {/* タグ付与 */}
            {onTag && (
              <button
                onClick={(e) => {
                  const r = e.currentTarget.getBoundingClientRect();
                  onTag(r.left, r.bottom + 4);
                }}
                title={t('ctx.tags')}
                aria-label={t('ctx.tags')}
                className="flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80"
              >
                <Tag size={16} />
              </button>
            )}
            {/* カレンダーに追加（右ペインにカレンダー入力を開く） */}
            {onOpenCalendar && (
              <button
                onClick={onOpenCalendar}
                title={t('mailbox.openCalendar')}
                aria-label={t('mailbox.openCalendar')}
                className="flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80"
              >
                <CalendarPlus size={16} />
              </button>
            )}
            {/* 外部画像の表示 on/off（このメールに外部画像があるときだけ表示）。
                左クリック=表示切替（未許可なら許可ダイアログ）／右クリック=許可メニュー。 */}
            {remoteUrls.length > 0 && (
              <button
                onClick={onRemoteIconClick}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setRemoteMenu({ x: e.clientX, y: e.clientY });
                }}
                title={remoteShown ? t('mailbox.remoteHide') : t('mailbox.remoteShow')}
                aria-label={remoteShown ? t('mailbox.remoteHide') : t('mailbox.remoteShow')}
                aria-pressed={remoteShown}
                className={`flex h-8 w-8 items-center justify-center rounded-md ${
                  remoteShown ? 'text-sky-400' : 'text-white/55 hover:text-white/80'
                }`}
              >
                {remoteShown ? <ImageIcon size={16} /> : <ImageOff size={16} />}
              </button>
            )}
            {/* グリーン認定/解除（差出人ドメイン単位） */}
            {senderDomain && (
              <button
                onClick={toggleGreen}
                title={isGreen ? t('green.uncertify', { domain: senderDomain }) : t('green.certify', { domain: senderDomain })}
                aria-label={isGreen ? t('green.uncertify', { domain: senderDomain }) : t('green.certify', { domain: senderDomain })}
                aria-pressed={isGreen}
                className={`flex h-8 w-8 items-center justify-center rounded-md ${
                  isGreen ? 'text-emerald-400' : 'text-white/55 hover:text-emerald-300'
                }`}
              >
                <LeafyGreen size={16} />
              </button>
            )}
            {/* 迷惑としてマーク（学習＋隔離）／迷惑フォルダでは「非迷惑に戻す」 */}
            {onMarkNotSpam ? (
              <button
                onClick={onMarkNotSpam}
                title={t('ctx.notSpam')}
                aria-label={t('ctx.notSpam')}
                className="flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-emerald-300"
              >
                <ThumbsUp size={16} />
              </button>
            ) : (
              onMarkSpam && (
                <button
                  onClick={onMarkSpam}
                  title={t('ctx.markSpam')}
                  aria-label={t('ctx.markSpam')}
                  className="flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-rose-300"
                >
                  <ThumbsDown size={16} />
                </button>
              )
            )}
            {/* 全文をサーバーから再取得（要約保存の解除・本文キャッシュの復元） */}
            <button
              onClick={handleRefetch}
              disabled={refetching}
              title={t('mailbox.refetch')}
              aria-label={t('mailbox.refetch')}
              className="flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80 disabled:opacity-50"
            >
              <RefreshCw size={16} className={refetching ? 'animate-spin' : ''} />
            </button>
            {/* 添付トグル: 転送アイコンの後に配置 */}
            {detail.has_attachments && (
              <button
                onClick={() => setAttachmentsOpen((o) => !o)}
                title={t('mailbox.attachments')}
                aria-label={t('mailbox.attachments')}
                className="flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80"
              >
                <Paperclip size={16} />
              </button>
            )}
            {/* 全文展開を畳む（会話バブルから渡されたときだけ）。固定ヘッダから常に押せる。 */}
            {onCollapse && (
              <button
                onClick={onCollapse}
                title={t('thread.collapse')}
                aria-label={t('thread.collapse')}
                className="flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80"
              >
                <X size={16} />
              </button>
            )}
            {note && <span className="ml-1 text-[10px] text-white/45">{note}</span>}
          </div>
        </div>
        <div className="mt-1 text-xs text-white/50">
          <div className="flex items-baseline justify-between gap-3">
            <span className="min-w-0 truncate">
              {d.verified_self && (
                <span
                  title={t('mailbox.verifiedSelf')}
                  className="mr-1 inline-flex align-[-2px] text-sky-300"
                >
                  <BadgeCheck size={12} aria-label={t('mailbox.verifiedSelf')} />
                </span>
              )}
              {d.is_vip && (
                <Gem
                  size={12}
                  className="mr-1 inline fill-sky-300/30 align-[-1px] text-sky-300"
                  aria-label={t('filter.vip')}
                />
              )}
              {isGreen && (
                <LeafyGreen
                  size={12}
                  className="mr-1 inline text-emerald-400 align-[-1px]"
                  aria-label={t('green.badge')}
                />
              )}
              {t('mailbox.from')}:{' '}
              <AddressLine
                name={d.from_name}
                address={d.from_address}
                onAdd={onAddContact}
                onEdit={onEditContact}
                onCompose={onComposeTo}
              />
            </span>
            <span className="shrink-0">{formatDateTime(d.date)}</span>
          </div>
          {showReplyTo && (
            <div className="break-words text-sky-300/80">
              {t('mailbox.replyTo')}:{' '}
              <LinkifyEmails
                text={d.reply_to ?? ''}
                onAdd={onAddContact}
                onEdit={onEditContact}
                onCompose={onComposeTo}
              />
            </div>
          )}
          {d.to_addresses && (
            <div className="break-words">
              {t('mailbox.to')}:{' '}
              <LinkifyEmails
                text={d.to_addresses}
                onAdd={onAddContact}
                onEdit={onEditContact}
                onCompose={onComposeTo}
              />
            </div>
          )}
          {d.cc_addresses && (
            <div className="break-words">
              {t('mailbox.cc')}:{' '}
              <LinkifyEmails
                text={d.cc_addresses}
                onAdd={onAddContact}
                onEdit={onEditContact}
                onCompose={onComposeTo}
              />
            </div>
          )}
          {/* タグ（一覧では出さず、詳細ヘッダの宛先の下にまとめて表示。× で個別に外せる） */}
          {tags && tags.length > 0 && (
            <div className="mt-1.5 flex flex-wrap gap-1">
              {tags.map((tg) => {
                const color = tg.color ?? DEFAULT_TAG_COLOR;
                return (
                  <span
                    key={tg.id}
                    className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium"
                    style={{ backgroundColor: `${color}33`, color }}
                  >
                    <span className="h-1.5 w-1.5 rounded-full" style={{ backgroundColor: color }} />
                    {tg.name}
                    {onRemoveTag && (
                      <button
                        onClick={() => onRemoveTag(tg.id)}
                        title={t('ctx.removeTag')}
                        aria-label={t('ctx.removeTag')}
                        className="-mr-0.5 flex h-3.5 w-3.5 items-center justify-center rounded-full hover:bg-white/20"
                      >
                        <X size={9} />
                      </button>
                    )}
                  </span>
                );
              })}
            </div>
          )}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-5 py-4">
        {d.body_state === 'absent' && !refreshed && (
          <div className="mb-3 flex items-center gap-2 rounded-md border border-sky-300/25 bg-sky-300/10 px-3 py-2 text-[11px] leading-snug text-sky-100/80">
            <RefreshCw size={12} className={refetching ? 'animate-spin' : ''} />
            <span className="flex-1">{t('mailbox.bodyAbsent')}</span>
            {!refetching && (
              <button
                onClick={handleRefetch}
                className="shrink-0 rounded bg-white/10 px-2 py-1 text-sky-50 hover:bg-white/20"
              >
                {t('mailbox.refetch')}
              </button>
            )}
          </div>
        )}
        {d.body_compacted && (
          <div className="mb-3 flex items-center gap-2 rounded-md border border-amber-300/25 bg-amber-300/10 px-3 py-2 text-[11px] leading-snug text-amber-100/80">
            <span className="flex-1">{t('mailbox.bodyCompacted')}</span>
            <button
              onClick={handleRefetch}
              disabled={refetching}
              className="flex shrink-0 items-center gap-1 rounded bg-white/10 px-2 py-1 text-amber-50 hover:bg-white/20 disabled:opacity-50"
            >
              <RefreshCw size={12} className={refetching ? 'animate-spin' : ''} />
              {t('mailbox.refetch')}
            </button>
          </div>
        )}
        {hasHtml ? (
          <HtmlText
            html={html}
            inlineImages={inlineImages}
            remoteImages={remoteShown ? remoteImages : {}}
            remoteDefaultExpanded={remoteExpandDefault}
            highlight={highlight}
            renderDate={renderDate}
            renderEmail={
              onAddContact
                ? (email) => (
                    <EmailAdd
                      email={email}
                      onAdd={onAddContact}
                      onEdit={onEditContact}
                      onCompose={onComposeTo}
                    />
                  )
                : undefined
            }
          />
        ) : body.trim() ? (
          <AutoLinkText
            text={body}
            className="text-sm leading-relaxed text-white/90"
            highlight={highlight}
            renderDate={renderDate}
            renderEmail={
              onAddContact
                ? (email) => (
                    <EmailAdd
                      email={email}
                      onAdd={onAddContact}
                      onEdit={onEditContact}
                      onCompose={onComposeTo}
                    />
                  )
                : undefined
            }
          />
        ) : (
          <p className="text-sm text-white/40">{t('mailbox.noBody')}</p>
        )}
      </div>

      {detail.has_attachments && (
        <div className="border-t border-white/10">
          <div className="flex items-center gap-2 px-5 py-2 text-xs font-medium text-white/50">
            {fileAttachments.length > 0 && (
              <input
                type="checkbox"
                checked={allSelected}
                ref={(el) => {
                  if (el) el.indeterminate = someSelected;
                }}
                onChange={toggleAll}
                title={t('mailbox.attachmentSelectAll')}
                className="h-3.5 w-3.5 shrink-0 accent-sky-400 opacity-60"
              />
            )}
            <button
              onClick={() => setAttachmentsOpen((o) => !o)}
              className="flex flex-1 items-center gap-1 hover:text-white/75"
            >
              <span>
                {t('mailbox.attachments')} ({fileAttachments.length})
              </span>
              <ChevronDown
                size={14}
                className={`transition-transform ${attachmentsOpen ? '' : '-rotate-90'}`}
              />
            </button>
            {selected.size > 0 && (
              <button
                onClick={handleSaveSelected}
                disabled={savingAll}
                className="flex shrink-0 items-center gap-1 rounded-md bg-white/10 px-2 py-1 text-white/80 hover:bg-white/20 disabled:opacity-50"
              >
                <Download size={12} />
                {t('mailbox.attachmentSaveSelected', { count: selected.size })}
              </button>
            )}
          </div>
          {attachmentsOpen && fileAttachments.length === 0 && (
            <div className="px-5 pb-3 text-xs text-white/40">
              {!attachmentsLoaded
                ? t('mailbox.attachmentBusy')
                : attachments.length > 0
                  ? t('mailbox.attachmentsInlineOnly')
                  : t('mailbox.attachmentsUnfetched')}
            </div>
          )}
          {attachmentsOpen && fileAttachments.length > 0 && (
            <ul className="max-h-64 space-y-1.5 overflow-y-auto px-5 pb-3">
              {fileAttachments.map((a) => {
                const image = isImage(a);
                const preview = previews[a.id];
                return (
                  <li key={a.id} className="rounded-md bg-white/5 px-3 py-2">
                    <div className="flex items-center gap-3">
                      <input
                        type="checkbox"
                        checked={selected.has(a.id)}
                        onChange={() => toggleOne(a.id)}
                        className="h-3.5 w-3.5 shrink-0 accent-sky-400 opacity-60"
                      />
                      <span
                        className="min-w-0 flex-1 truncate text-sm text-white/85"
                        title={a.filename}
                      >
                        {a.filename}
                      </span>
                      <span className="shrink-0 text-xs text-white/40">{formatSize(a.size)}</span>
                      {image && (
                        <button
                          onClick={() => togglePreview(a)}
                          disabled={busyId === a.id}
                          title={preview ? t('mailbox.attachmentHide') : t('mailbox.attachmentPreview')}
                          aria-label={t('mailbox.attachmentPreview')}
                          className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-white/10 text-white/80 hover:bg-white/20 disabled:opacity-50"
                        >
                          <ImageIcon size={13} />
                        </button>
                      )}
                      <button
                        onClick={() => handleOpen(a)}
                        disabled={busyId === a.id}
                        title={t('mailbox.attachmentOpen')}
                        aria-label={t('mailbox.attachmentOpen')}
                        className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-white/10 text-white/80 hover:bg-white/20 disabled:opacity-50"
                      >
                        <BookOpen size={13} />
                      </button>
                      {/* ダウンロード（保存）は未取得のときだけ。取得済み＝手元にあるので「開く」で十分。 */}
                      {!a.is_downloaded && (
                        <button
                          onClick={() => handleSave(a)}
                          disabled={busyId === a.id}
                          title={t('mailbox.attachmentDownload')}
                          aria-label={t('mailbox.attachmentDownload')}
                          className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-white/10 text-white/80 hover:bg-white/20 disabled:opacity-50"
                        >
                          <Download size={13} />
                        </button>
                      )}
                    </div>
                    {preview && (
                      <img
                        src={preview}
                        alt={a.filename}
                        className="mt-2 max-h-[480px] max-w-full rounded-md"
                      />
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}

      {hasQuotedExtra && (
        <div className="border-t border-white/10 px-5 py-2">
          <button
            className="text-xs text-sky-300 hover:underline"
            onClick={() => setShowQuotes((v) => !v)}
          >
            {showQuotes ? t('mailbox.hideQuotes') : t('mailbox.showQuotes')}
          </button>
        </div>
      )}

      {/* 外部画像アイコンの右クリックメニュー: 差出人の許可の付与/解除・表示切替。 */}
      {remoteMenu && (
        <ContextMenu
          x={remoteMenu.x}
          y={remoteMenu.y}
          items={[
            senderAllowed
              ? {
                  key: 'revoke',
                  label: t('mailbox.remoteRevoke'),
                  Icon: ImageOff,
                  danger: true,
                  onClick: () => setRemoteAllow(false),
                }
              : {
                  key: 'allow',
                  label: t('mailbox.remoteAllowSender'),
                  Icon: ImageIcon,
                  onClick: () => setRemoteAllow(true),
                },
            {
              key: 'toggle',
              label: remoteShown ? t('mailbox.remoteHide') : t('mailbox.remoteShow'),
              Icon: remoteShown ? ImageOff : ImageIcon,
              onClick: () => setRemoteShown((v) => !v),
            },
          ]}
          onClose={() => setRemoteMenu(null)}
        />
      )}
    </div>
  );
}
