import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  BadgeCheck,
  Check,
  ChevronDown,
  ChevronUp,
  Copy,
  Download,
  Forward,
  Gem,
  LeafyGreen,
  Mail,
  MailOpen,
  MoreHorizontal,
  Paperclip,
  Pencil,
  Printer,
  Quote,
  Reply,
  ReplyAll,
  Scissors,
  Star,
  StarOff,
  Tag,
  ThumbsDown,
  ThumbsUp,
  Trash2,
  X,
} from 'lucide-react';
import type { ThreadView } from '@bindings/ThreadView';
import type { ThreadMessage } from '@bindings/ThreadMessage';
import type { MailDetail } from '@bindings/MailDetail';
import type { TagSummary } from '@bindings/TagSummary';
import type { AttachmentSummary } from '@bindings/AttachmentSummary';
import { mailGet, mailAttachments, attachmentOpen } from '../services/mail';
import { attachmentImage } from '../utils/imageCache';
import { greenDomainAdd, greenDomainWarn } from '../services/green';
import { threadRename, threadSplit, threadView } from '../services/threads';
import { parseAddress } from '../utils/address';
import { copyText } from '../utils/clipboard';
import { getBubbleHtml, getInlineImages, PREFS_EVENT } from '../config/prefs';
import { formatDateTime } from '../utils/datetime';
import { saveAllAttachments, saveAttachment } from '../utils/attachmentSave';
import { withActivity } from '../stores/activity';
import { MailBody, makeRenderDate } from './MailBody';
import { AutoLinkText, HtmlText, inlineCidRefs } from './HtmlText';
import { AttachedImages, isImage } from './AttachedImages';
import { PrintMail } from './PrintMail';
import { ContextMenu, type MenuItem } from './ContextMenu';
import type { CalendarPanelInitial } from './CalendarPanel';

/** 全文展開時、本文カードの先頭を表示域の上端から少しだけ下げて置くための余白（px）。 */
const EXPAND_TOP_GAP = 8;

/** 選択テキストを引用文に整形する（各行の先頭に「> 」を付与。空行は「>」のみ）。 */
const toQuoted = (text: string): string =>
  text
    .replace(/\r\n?/g, '\n')
    .replace(/\s+$/, '')
    .split('\n')
    .map((line) => (line.length > 0 ? `> ${line}` : '>'))
    .join('\n');

/** 端末ローカルの今日（'YYYY-MM-DD'）。ヘッダの「カレンダーに追加」の既定日に使う。 */
const todayLocalDay = (): string => {
  const n = new Date();
  const p = (x: number) => String(x).padStart(2, '0');
  return `${n.getFullYear()}-${p(n.getMonth() + 1)}-${p(n.getDate())}`;
};

/** 会話ビューが親（MailboxView）に要求する、メール1通単位の操作・状態。 */
export interface ConversationHandlers {
  /** そのメールに付いているタグ（一覧側から解決。無ければ空）。 */
  tagsFor: (id: number) => TagSummary[];
  /** そのメールにスターが付いているか。 */
  starredFor: (id: number) => boolean;
  /** スターの付け外し。`next` 指定時はその値に設定（会話ビューはメッセージ単位の真値を持つため）。 */
  onToggleStar: (id: number, next?: boolean) => void;
  onTag: (id: number, x: number, y: number) => void;
  onRemoveTag: (id: number, tagId: number) => void;
  onMarkSpam: (id: number) => void;
  /** 非迷惑に戻す（迷惑フォルダ表示時）。 */
  onMarkNotSpam: (id: number) => void;
  /** 迷惑フォルダを表示中か（true なら迷惑操作を「非迷惑に戻す」に切り替える）。 */
  isSpam: boolean;
  /** 単一メールの既読/未読切替（バブルの右クリックメニューから）。 */
  onSetRead: (id: number, read: boolean) => void;
  /** 単一メールをゴミ箱へ。会話側は実行後に会話を再読込する。 */
  onDelete: (id: number) => void | Promise<void>;
  /** そのメールに返信/転送（作成画面を開く）。 */
  onReply: (mode: 'reply' | 'replyAll' | 'forward', messageId: number) => void;
  onAddContact?: (name: string | null, email: string) => void;
  onEditContact?: (id: number) => void;
  onComposeTo?: (email: string) => void;
  /** 本文の日付＋、またはヘッダの「カレンダーに追加」から、右ペインのカレンダー入力を開く。 */
  onAddCalendar?: (init: CalendarPanelInitial) => void;
  onGreenChange?: () => void;
  /** スレッド構成（分割/再件名）が変わったら一覧を作り直す。 */
  onThreadChanged?: () => void;
  /** メールを別スレッドへ切り出す（Conversation 内部で結線。バブルのメニューから呼ぶ）。 */
  onThreadChangedSplit?: (messageId: number, mode: 'this' | 'below') => void;
}

/** 差出人の表示名（ヘッダ名 → 住所録名 → アドレス）。 */
function senderName(m: ThreadMessage, you: string): string {
  if (m.direction === 'out') return you;
  const addr = parseAddress(m.from_address);
  return m.from_name?.trim() || addr.name || addr.email || '(no sender)';
}

/**
 * 1 通ぶんのバブル。既定は clean_body だけの chat 風表示（未読・★はバブル見出しにバッジ表示）。
 * 「引用を表示」で全文（引用込み）、バブルのクリックで従来の MailBody（添付/HTML/画像）を展開する。
 */
function Bubble({
  m,
  you,
  handlers,
  expanded,
  onToggleExpand,
  highlight,
  htmlBody,
  inlineImagesOn,
}: {
  m: ThreadMessage;
  you: string;
  handlers: ConversationHandlers;
  expanded: boolean;
  onToggleExpand: () => void;
  /** 検索語（複数）。本文中の一致をハイライトする。 */
  highlight?: string[];
  /** 設定オン時、body_html があればバブルを HTML 本文で描画する（外部画像はプレースホルダ）。 */
  htmlBody?: boolean;
  /** 設定オン時、本文が cid: で参照する埋め込み画像を取得して表示する。 */
  inlineImagesOn?: boolean;
}) {
  const { t } = useTranslation();
  const out = m.direction === 'out';
  // 見出し（ラベル）の上端固定用。会話スクロール枠の上端に貼り付いている間だけ true にして、
  // その時だけラベル背景を不透明にする（普段はチャットを汚さない）。固定検出はラベル直前に置いた
  // 高さ0のセンチネルが枠上端より上へ抜けたかどうかで判定する（IntersectionObserver）。
  const [stuck, setStuck] = useState(false);
  const sentinelRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const sentinel = sentinelRef.current;
    const root = sentinel?.closest('[data-conversation-scroll]');
    if (!sentinel || !(root instanceof HTMLElement)) return;
    const io = new IntersectionObserver(
      (entries) => {
        const e = entries[0];
        if (!e) return;
        // センチネルが枠上端より上へ抜けた（=見えない）＝ラベルが固定中。下（未到達）は固定でない。
        setStuck(!e.isIntersecting && e.boundingClientRect.top <= (e.rootBounds?.top ?? 0));
      },
      { root },
    );
    io.observe(sentinel);
    return () => io.disconnect();
  }, []);
  const [showQuotes, setShowQuotes] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  // 文字選択中の右クリックで出す「コピー／引用としてコピー」メニュー（選択テキストを保持）。
  const [selMenu, setSelMenu] = useState<{ x: number; y: number; text: string } | null>(null);
  // スター・既読はメッセージ単位の真値を初期値に、トグルを即時反映する。
  const [starred, setStarred] = useState(m.is_starred);
  useEffect(() => setStarred(m.is_starred), [m.id, m.is_starred]);
  const toggleStar = () => {
    handlers.onToggleStar(m.id, !starred);
    setStarred((v) => !v);
  };
  const [read, setRead] = useState(m.is_read);
  useEffect(() => setRead(m.is_read), [m.id, m.is_read]);
  const toggleRead = () => {
    handlers.onSetRead(m.id, !read);
    setRead((v) => !v);
  };
  // グリーン認定/解除（差出人ドメイン単位）を折りたたみバブルからも操作する。
  // null＝ThreadMessage の is_green を使う（メール切替でリセット）。
  const [greenOverride, setGreenOverride] = useState<boolean | null>(null);
  useEffect(() => setGreenOverride(null), [m.id, m.is_green]);
  const isGreen = greenOverride ?? m.is_green;
  const senderDomain =
    !out && (m.from_address ?? '').includes('@')
      ? (m.from_address ?? '').split('@').pop()?.trim().toLowerCase() || ''
      : '';
  // フリーメール（gmail.com 等）はドメイン単位で信頼できないため認定は拒否される（false）。
  // その場合はバッジを変えない（理由の提示は本文ビュー側で行う）。
  const toggleGreen = async () => {
    if (!senderDomain) return;
    const next = !isGreen;
    try {
      if (next) {
        if (!(await greenDomainAdd(senderDomain))) return;
      } else await greenDomainWarn(senderDomain);
      setGreenOverride(next);
      handlers.onGreenChange?.();
    } catch {
      /* noop */
    }
  };
  // 展開時に読む詳細（添付・HTML・画像は従来の MailBody を再利用）。
  const [detail, setDetail] = useState<MailDetail | null>(null);
  useEffect(() => {
    let alive = true;
    if (expanded && !detail) {
      mailGet(m.id)
        .then((d) => alive && setDetail(d))
        .catch(() => undefined);
    }
    return () => {
      alive = false;
    };
  }, [expanded, detail, m.id]);

  // 全文を開いたら（本文カードが描画されたら）、その先頭から読めるよう、バブルをスクロール域の
  // 上側へ寄せる（できるだけ上に。末尾付近では届く範囲まで＝scrollTop はブラウザが頭打ちにする）。
  // 畳むときは親側の効果で位置を保つため、ここでは何もしない。
  useLayoutEffect(() => {
    if (!expanded || !detail) return;
    const el = document.getElementById(`bubble-${m.id}`);
    const container = el?.closest('[data-conversation-scroll]');
    if (!el || !(container instanceof HTMLElement)) return;
    const top =
      el.getBoundingClientRect().top - container.getBoundingClientRect().top + container.scrollTop;
    container.scrollTop = Math.max(0, top - EXPAND_TOP_GAP);
  }, [expanded, detail, m.id]);

  // 折りたたみバブルのまま、添付アイコンから直接「開く」／「ダウンロード」する。クリック時に
  // 一覧を遅延取得し、1 件ならそのまま実行、複数ならファイル名の小メニューを出す（mode で分岐）。
  // 未取得の添付は Rust 側が取得してから開く。取得に時間がかかるのでフッターに進捗を出す。
  const [atts, setAtts] = useState<AttachmentSummary[] | null>(null);
  const [attMenu, setAttMenu] = useState<{ x: number; y: number; mode: 'open' | 'save' } | null>(
    null,
  );
  const [attBusy, setAttBusy] = useState(false);
  const runAttachments = async (clientX: number, clientY: number, mode: 'open' | 'save') => {
    setAttBusy(true);
    try {
      let list = atts;
      if (!list) {
        // 本文に埋め込まれる（cid: で参照される）画像は本文側に出るので除き、
        // 参照されない inline パートは通常の添付として開く/保存できるようにする。
        list = (await mailAttachments(m.id)).filter(
          (a) => a.kind !== 'inline' || !(a.content_id && htmlCids.has(a.content_id)),
        );
        setAtts(list);
      }
      if (list.length === 0) return;
      if (list.length === 1) {
        const a = list[0];
        if (mode === 'save') await saveAttachment(a.id, a.filename, t('activity.downloadingAttachment'));
        else await withActivity(t('activity.openingAttachment'), () => attachmentOpen(a.id));
        return;
      }
      setAttMenu({ x: clientX, y: clientY, mode });
    } catch {
      /* noop（開けないときは無反応。全文を展開すれば MailBody で詳細に扱える） */
    } finally {
      setAttBusy(false);
    }
  };

  const clean = (m.clean_body ?? '').trim();
  const full = (m.body_plain ?? '').trim();
  const body = showQuotes ? full : clean || full;
  // 設定オンで HTML 本文があるときは HtmlText で描画（外部画像は取得せずプレースホルダのまま）。
  // ただし HTML には引用除去版が無いので、引用のある返信（has_quotes）はチャット感を保つため
  // プレーン（新規部分のみ）にフォールバックする。実質「引用のないメールだけ HTML 描画」。
  const renderHtml = !!htmlBody && !!m.body_html?.trim() && !m.has_quotes;

  // 本文（HTML）が cid: で参照している Content-ID。埋め込み画像の解決と、
  // 「本文に出ない inline パートは添付として扱う」判定の両方に使う。
  const htmlCids = useMemo(() => new Set(inlineCidRefs(m.body_html ?? '')), [m.body_html]);

  // 本文が cid: で参照する埋め込み画像（ロゴ等）を解決する。解決しないと本文中の画像が
  // プレースホルダ（🖼）のままになる＝「インライン画像が表示されない」状態になる。
  // 外部(http)画像とは別物で、取得はローカルの添付パートからなのでトラッキングは起きない。
  // 対象は「HTML で描画するバブル」かつ「cid: 参照がある」ものだけ（実質ニュースレター等）。
  // 本体未取得なら Rust 側が IMAP から該当パートだけ取り、以後はディスクキャッシュが効く。
  const [inlineImages, setInlineImages] = useState<Record<string, string>>({});
  // 右クリックの「保存/開く」で使う cid → 添付メタ。
  const [inlineAtts, setInlineAtts] = useState<Record<string, AttachmentSummary>>({});
  const wantInline = renderHtml && !!inlineImagesOn && htmlCids.size > 0;
  useEffect(() => {
    if (!wantInline) return;
    let alive = true;
    mailAttachments(m.id)
      .then((list) => {
        if (!alive) return;
        list
          .filter((a) => a.kind === 'inline' && a.content_id && htmlCids.has(a.content_id))
          .forEach((a) => {
            const cid = a.content_id as string;
            setInlineAtts((prev) => ({ ...prev, [cid]: a }));
            attachmentImage(a.id)
              .then((url) => {
                if (alive) setInlineImages((prev) => ({ ...prev, [cid]: url }));
              })
              .catch(() => undefined);
          });
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [wantInline, htmlCids, m.id]);

  // 本文中の埋め込み画像を右クリックしたときのメニュー（保存 / 既定アプリで開く）。
  const [imgMenu, setImgMenu] = useState<{ x: number; y: number; att: AttachmentSummary } | null>(
    null,
  );
  // 印刷中か（バブルのメニューから。版面は非表示 iframe に作る）。
  const [printing, setPrinting] = useState(false);

  // バブルに並べる画像。本文が無く画像だけのメール（iPhone から写真を送っただけ等）に加え、
  // **本文と画像の両方があるメールでも並べる**。以前は本文が空のときだけ出していたため、
  // 本文のあるメールに画像が付いていても、フッターの 📎 以外に手掛かりが無く気づけなかった
  // （利用者報告 2026-09-01）。サムネイル表示なのでチャットの流れは埋めない。
  const [bubbleImages, setBubbleImages] = useState<AttachmentSummary[]>([]);
  useEffect(() => {
    // 添付が無いメールで問い合わせない（スレッドの全バブルで引くのを避ける）。
    if (!m.has_attachments || !inlineImagesOn) return;
    let alive = true;
    mailAttachments(m.id)
      .then((list) => {
        if (!alive) return;
        // ここでは「画像かどうか」だけで拾う（kind は使わない。Content-ID の有無でラベルが
        // 割れ、送信クライアント次第で見え方が変わってしまうため）。本文に既に出ているものを
        // 除く判定は描画側で行う（下の shownImages）。
        setBubbleImages(list.filter(isImage));
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [m.has_attachments, inlineImagesOn, m.id]);

  // バブルに並べる画像から、本文に実際に表示できた埋め込み画像を除いたもの。
  const shownImages = bubbleImages.filter(
    (a) => !(a.content_id && inlineImages[a.content_id]),
  );

  // 本文中の日付に＋（カレンダー追加）を出す描画関数（折りたたみバブル・全文表示で共有）。
  const renderDate = makeRenderDate(handlers.onAddCalendar, {
    baseISO: m.date ?? undefined,
    title: m.subject ?? undefined,
    relatedEmailId: m.id,
  });

  // 右クリック（と「…」ボタン）のメニュー。一覧の右クリックと同じ操作をメール単位で提供する。
  const menuItems: MenuItem[] = [
    {
      key: 'reply',
      label: t('compose.reply'),
      Icon: Reply,
      onClick: () => handlers.onReply('reply', m.id),
    },
    {
      key: 'replyAll',
      label: t('compose.replyAll'),
      Icon: ReplyAll,
      onClick: () => handlers.onReply('replyAll', m.id),
    },
    {
      key: 'forward',
      label: t('compose.forward'),
      Icon: Forward,
      onClick: () => handlers.onReply('forward', m.id),
    },
    {
      key: 'print',
      label: t('mailbox.print'),
      Icon: Printer,
      onClick: () => setPrinting(true),
    },
    read
      ? { key: 'unread', label: t('ctx.markUnread'), Icon: Mail, onClick: toggleRead }
      : { key: 'read', label: t('ctx.markRead'), Icon: MailOpen, onClick: toggleRead },
    starred
      ? { key: 'unstar', label: t('ctx.unstar'), Icon: StarOff, onClick: toggleStar }
      : { key: 'star', label: t('ctx.star'), Icon: Star, onClick: toggleStar },
    {
      key: 'tags',
      label: t('ctx.tags'),
      Icon: Tag,
      onClick: () => {
        if (menu) handlers.onTag(m.id, menu.x, menu.y);
      },
    },
    ...(senderDomain
      ? [
          {
            key: 'green',
            label: isGreen
              ? t('green.uncertify', { domain: senderDomain })
              : t('green.certify', { domain: senderDomain }),
            Icon: LeafyGreen,
            onClick: () => {
              void toggleGreen();
            },
          },
        ]
      : []),
    handlers.isSpam
      ? {
          key: 'notSpam',
          label: t('ctx.notSpam'),
          Icon: ThumbsUp,
          onClick: () => handlers.onMarkNotSpam(m.id),
        }
      : {
          key: 'spam',
          label: t('ctx.markSpam'),
          Icon: ThumbsDown,
          onClick: () => handlers.onMarkSpam(m.id),
        },
    {
      key: 'delete',
      label: t('ctx.delete'),
      Icon: Trash2,
      danger: true,
      onClick: () => handlers.onDelete(m.id),
    },
    {
      key: 'splitBelow',
      label: t('thread.splitBelow'),
      Icon: Scissors,
      onClick: () => handlers.onThreadChangedSplit?.(m.id, 'below'),
    },
    {
      key: 'splitThis',
      label: t('thread.splitThis'),
      Icon: Scissors,
      onClick: () => handlers.onThreadChangedSplit?.(m.id, 'this'),
    },
  ];

  return (
    <div className={`flex ${out ? 'justify-end' : 'justify-start'}`}>
      {/* 全文表示中は幅いっぱいだが、右端に少し余白（pr-6）を残す。ここは会話スクロールの
          掴みしろで、全文カードの内部スクロールと分けてスレッド自体を送れるようにする。 */}
      <div
        data-msg-content
        className={`group/bubble min-w-0 ${expanded ? 'w-full pr-6' : 'max-w-[82%]'}`}
        onContextMenu={(e) => {
          // 文字選択中は「コピー／引用としてコピー」の独自メニューを出す（ネイティブの差し替え）。
          const sel = window.getSelection()?.toString() ?? '';
          if (sel.trim().length > 0) {
            e.preventDefault();
            setSelMenu({ x: e.clientX, y: e.clientY, text: sel });
            return;
          }
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        {/* 上端固定の検出用センチネル（高さ0）。これが枠上端より上へ抜けるとラベルが固定中になる。 */}
        <div ref={sentinelRef} aria-hidden className="pointer-events-none h-0" />
        {/* 差出人＋時刻（相手は左、自分は右にそろえる）。スレッドヘッダのすぐ下に貼り付き（sticky）、
            固定中は「アプリと同じ背景を同じ位置に」貼る（bg-fixed でビューポート基準に合わせる）。
            写真は不透明なので下から潜り込む本文はラベル下端でスパッと隠れ、かつ周囲と一体で透明に
            見える。バブルが尽きるとラベルも一緒に上へ流れて次のラベルと交代する。 */}
        <div
          className={`sticky top-0 z-10 mb-0.5 flex items-center gap-1.5 px-1 py-0.5 text-[10px] text-white/45 ${
            out ? 'justify-end' : 'justify-start'
          } ${stuck ? 'rounded-b-md shadow-sm shadow-black/20' : ''}`}
          // 固定中だけ、アプリと同じ背景を同じ位置（bg-fixed=ビューポート基準）で貼る。写真は不透明
          // なので本文を隠しつつ、周囲の背景と一体化して透明に見える。任意クラスの JIT 検出に頼らず
          // インラインで指定する。
          style={
            stuck
              ? {
                  backgroundImage: 'var(--app-backdrop)',
                  backgroundSize: 'cover',
                  backgroundPosition: 'center',
                  backgroundAttachment: 'fixed',
                }
              : undefined
          }
        >
          {m.verified_self && (
            <span
              title={t('mailbox.verifiedSelf')}
              className="inline-flex shrink-0 text-sky-300"
            >
              <BadgeCheck size={11} aria-label={t('mailbox.verifiedSelf')} />
            </span>
          )}
          {!out && m.is_vip && (
            <Gem
              size={11}
              className="shrink-0 fill-sky-300/30 text-sky-300"
              aria-label={t('filter.vip')}
            />
          )}
          {!out && senderDomain && isGreen && (
            <button
              onClick={(e) => {
                e.stopPropagation();
                void toggleGreen();
              }}
              title={t('green.uncertify', { domain: senderDomain })}
              aria-label={t('green.uncertify', { domain: senderDomain })}
              aria-pressed={true}
              className="flex shrink-0 items-center text-emerald-400 hover:text-emerald-300"
            >
              <LeafyGreen size={11} />
            </button>
          )}
          <span className="truncate font-medium text-white/60">{senderName(m, you)}</span>
          <span className="shrink-0">{formatDateTime(m.date)}</span>
          {!read && <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-sky-400" />}
          {/* スター: バブルを見ながら付け外しできる。付与済みは常時アンバー、未付与は
              ホバーで薄く出す（普段はチャットのラベルを汚さない。迷惑ボタンと同じ流儀）。 */}
          <button
            onClick={(e) => {
              e.stopPropagation();
              toggleStar();
            }}
            title={t(starred ? 'ctx.unstar' : 'ctx.star')}
            aria-label={t(starred ? 'ctx.unstar' : 'ctx.star')}
            aria-pressed={starred}
            className={`flex shrink-0 items-center transition-opacity ${
              starred
                ? 'text-amber-300'
                : 'text-white/40 opacity-0 hover:text-amber-300 group-hover/bubble:opacity-100'
            }`}
          >
            <Star size={11} className={starred ? 'fill-amber-300' : ''} />
          </button>
          {/* 迷惑メールとしてマーク（受信のみ）。ホバーで出す（普段はチャットのラベルを汚さない）。
              迷惑フォルダ表示中は「非迷惑に戻す」に切り替える。 */}
          {!out &&
            (handlers.isSpam ? (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  handlers.onMarkNotSpam(m.id);
                }}
                title={t('ctx.notSpam')}
                aria-label={t('ctx.notSpam')}
                className="flex shrink-0 items-center text-white/40 opacity-0 transition-opacity hover:text-emerald-300 group-hover/bubble:opacity-100"
              >
                <ThumbsUp size={11} />
              </button>
            ) : (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  handlers.onMarkSpam(m.id);
                }}
                title={t('ctx.markSpam')}
                aria-label={t('ctx.markSpam')}
                className="flex shrink-0 items-center text-white/40 opacity-0 transition-opacity hover:text-rose-300 group-hover/bubble:opacity-100"
              >
                <ThumbsDown size={11} />
              </button>
            ))}
        </div>

        {expanded && detail ? (
          // 全文表示: 従来の MailBody をそのまま埋め込む（添付・HTML・外部画像・タグ・スター）。
          // カードは高さ上限（max-h）＋ overflow-hidden にして「ヘッダ固定・本文だけ内部スクロール」
          // のセクション構成にする。裏を本文が流れないのでヘッダはカードと同じ透過のまま保て、角の
          // はみ出しも切れる。畳むボタンは固定ヘッダ内（onCollapse）にあるので常に押せる。
          <div className="flex max-h-[70vh] min-h-0 flex-col overflow-hidden rounded-xl border border-white/15 bg-neutral-900/40">
            <MailBody
              detail={detail}
              tags={handlers.tagsFor(m.id)}
              starred={starred}
              onToggleStar={toggleStar}
              onTag={(x, y) => handlers.onTag(m.id, x, y)}
              onRemoveTag={(tagId) => handlers.onRemoveTag(m.id, tagId)}
              onReply={(mode) => handlers.onReply(mode, m.id)}
              onMarkSpam={handlers.isSpam ? undefined : () => handlers.onMarkSpam(m.id)}
              onMarkNotSpam={handlers.isSpam ? () => handlers.onMarkNotSpam(m.id) : undefined}
              onAddContact={handlers.onAddContact}
              onEditContact={handlers.onEditContact}
              onComposeTo={handlers.onComposeTo}
              onAddCalendar={handlers.onAddCalendar}
              onOpenCalendar={
                handlers.onAddCalendar
                  ? () =>
                      handlers.onAddCalendar?.({
                        day: todayLocalDay(),
                        title: m.subject ?? undefined,
                        relatedEmailId: m.id,
                      })
                  : undefined
              }
              onGreenChange={handlers.onGreenChange}
              highlight={highlight}
              onCollapse={onToggleExpand}
            />
          </div>
        ) : (
          // 通常バブル: clean_body（新規部分）だけを chat 風に。ダブルクリックで全文展開する
          // （シングルクリックは文字選択のまま。ダブルクリックで選ばれる単語は消してから開く。
          // 「全文を開く」ボタンからも展開できる。内部のボタンは stopPropagation で独立動作）。
          <div
            onDoubleClick={() => {
              window.getSelection()?.removeAllRanges();
              onToggleExpand();
            }}
            className={`group relative cursor-pointer rounded-2xl px-3.5 py-2 text-sm leading-relaxed shadow-sm ${
              out
                ? 'rounded-tr-sm bg-sky-500/20 text-white/90'
                : 'rounded-tl-sm bg-white/10 text-white/90'
            }`}
          >
            {/* 引用表示中は長くなるため、上端にも「引用を隠す」を出す */}
            {showQuotes && (
              <div
                className={`mb-1 flex items-center gap-2 text-[10px] text-white/45 ${
                  out ? 'justify-end' : 'justify-start'
                }`}
              >
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setShowQuotes(false);
                  }}
                  className="hover:text-sky-300"
                >
                  {t('mailbox.hideQuotes')}
                </button>
              </div>
            )}
            {renderHtml ? (
              <HtmlText
                html={m.body_html as string}
                inlineImages={inlineImages}
                highlight={highlight}
                renderDate={renderDate}
                onInlineImageMenu={(cid, x, y) => {
                  const att = inlineAtts[cid];
                  if (att) setImgMenu({ x, y, att });
                }}
              />
            ) : body ? (
              <AutoLinkText text={body} highlight={highlight} renderDate={renderDate} />
            ) : bubbleImages.length === 0 ? (
              <span className="text-white/40">{t('mailbox.noBody')}</span>
            ) : null}

            {/* 画像添付をバブルに並べる（保存は右クリック）。本文の有無に関わらず出すので、
                本文のあるメールでも画像が付いていることに気づける。チャットの流れを埋めない
                ようサムネイル表示にし、クリックで実寸に広げる。
                除くのは「本文に **実際に表示できた** 埋め込み画像」だけ。cid: で参照されて
                いても読み込めていなければ本文にはプレースホルダしか出ないので、その場合は
                サムネイルを出す（実測 2026-09-01: 画像があるのに気づけない状態だった）。 */}
            {shownImages.length > 0 && (
              <AttachedImages
                images={shownImages}
                onMenu={(att, x, y) => setImgMenu({ x, y, att })}
                compact
              />
            )}

            {/* 操作行（フッター）: 返信・転送・引用トグル・添付・全文・メニュー */}
            <div
              className={`mt-1 flex items-center gap-2 text-[10px] text-white/45 ${
                out ? 'justify-end' : 'justify-start'
              }`}
            >
              {/* 折りたたみバブルのまま返信・転送を押せるようにする（「…」メニューにも同じ操作あり）。 */}
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  handlers.onReply('reply', m.id);
                }}
                title={t('compose.reply')}
                aria-label={t('compose.reply')}
                className="inline-flex items-center gap-0.5 hover:text-sky-300"
              >
                <Reply size={12} />
              </button>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  handlers.onReply('replyAll', m.id);
                }}
                title={t('compose.replyAll')}
                aria-label={t('compose.replyAll')}
                className="inline-flex items-center gap-0.5 hover:text-sky-300"
              >
                <ReplyAll size={12} />
              </button>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  handlers.onReply('forward', m.id);
                }}
                title={t('compose.forward')}
                aria-label={t('compose.forward')}
                className="inline-flex items-center gap-0.5 hover:text-sky-300"
              >
                <Forward size={12} />
              </button>
              {!renderHtml && m.has_quotes && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setShowQuotes((v) => !v);
                  }}
                  className="hover:text-sky-300"
                >
                  {showQuotes ? t('mailbox.hideQuotes') : t('mailbox.showQuotes')}
                </button>
              )}
              {m.has_attachments && (
                <>
                  {/* 添付をそのまま既定アプリで開く（従来どおり）。 */}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      void runAttachments(e.clientX, e.clientY, 'open');
                    }}
                    disabled={attBusy}
                    title={t('mailbox.openAttachment')}
                    aria-label={t('mailbox.openAttachment')}
                    className="inline-flex items-center gap-0.5 hover:text-sky-300 disabled:opacity-50"
                  >
                    <Paperclip size={11} />
                  </button>
                  {/* 添付を保存（ダウンロード）。開く動作は残しつつ、保存も選べるようにする。 */}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      void runAttachments(e.clientX, e.clientY, 'save');
                    }}
                    disabled={attBusy}
                    title={t('mailbox.attachmentDownload')}
                    aria-label={t('mailbox.attachmentDownload')}
                    className="inline-flex items-center gap-0.5 hover:text-sky-300 disabled:opacity-50"
                  >
                    <Download size={11} />
                  </button>
                </>
              )}
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onToggleExpand();
                }}
                className="hover:text-white/80"
              >
                {t('thread.showFull')}
              </button>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  const r = e.currentTarget.getBoundingClientRect();
                  setMenu({ x: r.left, y: r.bottom + 4 });
                }}
                title={t('thread.actions')}
                aria-label={t('thread.actions')}
                className="flex h-4 w-4 items-center justify-center rounded hover:bg-white/15 hover:text-white/80"
              >
                <MoreHorizontal size={13} />
              </button>
            </div>
          </div>
        )}
      </div>

      {menu && (
        <ContextMenu x={menu.x} y={menu.y} items={menuItems} onClose={() => setMenu(null)} />
      )}

      {selMenu && (
        <ContextMenu
          x={selMenu.x}
          y={selMenu.y}
          items={[
            {
              key: 'copy',
              label: t('ctx.copy'),
              Icon: Copy,
              onClick: () => void copyText(selMenu.text),
            },
            {
              key: 'copyQuote',
              label: t('ctx.copyQuote'),
              Icon: Quote,
              onClick: () => void copyText(toQuoted(selMenu.text)),
            },
          ]}
          onClose={() => setSelMenu(null)}
        />
      )}

      {printing && <PrintMail emailId={m.id} onDone={() => setPrinting(false)} />}

      {/* 本文に埋め込まれた画像の右クリック: 添付一覧には出ないので、ここから保存・表示する。 */}
      {imgMenu && (
        <ContextMenu
          x={imgMenu.x}
          y={imgMenu.y}
          header={imgMenu.att.filename}
          items={[
            {
              key: 'save',
              label: t('mailbox.attachmentDownload'),
              Icon: Download,
              onClick: () =>
                void saveAttachment(
                  imgMenu.att.id,
                  imgMenu.att.filename,
                  t('activity.downloadingAttachment'),
                ),
            },
            {
              key: 'open',
              label: t('mailbox.openAttachment'),
              Icon: Paperclip,
              onClick: () =>
                void withActivity(t('activity.openingAttachment'), () =>
                  attachmentOpen(imgMenu.att.id),
                ),
            },
          ]}
          onClose={() => setImgMenu(null)}
        />
      )}

      {attMenu && atts && atts.length > 1 && (
        <ContextMenu
          x={attMenu.x}
          y={attMenu.y}
          header={t(attMenu.mode === 'save' ? 'mailbox.attachmentDownload' : 'mailbox.attachments')}
          items={[
            // 保存のときだけ「すべて保存」を先頭に置く。開く側に付けないのは、添付の数だけ
            // 外部アプリが一斉に立ち上がってしまうため。
            ...(attMenu.mode === 'save'
              ? [
                  {
                    key: 'att-save-all',
                    label: t('mailbox.attachmentSaveAll', { count: atts.length }),
                    Icon: Download,
                    onClick: () =>
                      void saveAllAttachments(atts, t('activity.downloadingAttachment')),
                  },
                ]
              : []),
            ...atts.map((a) => ({
              key: `att-${a.id}`,
              label: a.filename,
              Icon: attMenu.mode === 'save' ? Download : Paperclip,
              onClick: () =>
                attMenu.mode === 'save'
                  ? void saveAttachment(a.id, a.filename, t('activity.downloadingAttachment'))
                  : void withActivity(t('activity.openingAttachment'), () => attachmentOpen(a.id)),
            })),
          ]}
          onClose={() => setAttMenu(null)}
        />
      )}
    </div>
  );
}

/** 会話ビュー本体（スレッド情報＋時系列バブル）。全メール折りたたみで表示し、クリックで展開する。 */
export function Conversation({
  openedId,
  folder,
  handlers,
  query,
}: {
  openedId: number;
  /** 閲覧中のフォルダ。trash/spam を見ているとき以外は、それらのメールを会話から隠す。 */
  folder?: string;
  handlers: ConversationHandlers;
  /** 検索中の語句。会話内で一致をハイライトし、上下ボタンで移動できるようにする。 */
  query?: string;
}) {
  const { t } = useTranslation();
  const [view, setView] = useState<ThreadView | null>(null);
  const [loading, setLoading] = useState(true);
  // バブルを HTML で描画するか（設定。PREFS_EVENT で即時反映）。
  const [htmlBubbles, setHtmlBubbles] = useState(getBubbleHtml());
  // 本文埋め込み画像（cid:）を取得して表示するか（設定。PREFS_EVENT で即時反映）。
  const [inlineImagesOn, setInlineImagesOn] = useState(getInlineImages());
  // 既定は全メール折りたたみ（バブルのみ）。クリックしたメールだけ展開する。
  const [expandedIds, setExpandedIds] = useState<Set<number>>(new Set());
  // タイトル編集
  const [editingTitle, setEditingTitle] = useState(false);
  const [titleDraft, setTitleDraft] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);
  // 展開/折りたたみで版面が伸縮しても、基準にしたバブルの表示位置を保つためのアンカー。
  // toggleExpand の直前に控え、expandedIds 変化後の useLayoutEffect で同じ位置へ戻す。
  const expandAnchor = useRef<{ id: number; top: number } | null>(null);

  // 検索語（空白区切り・全半角）。会話内ハイライトと <>移動に使う。
  const terms = useMemo(
    () =>
      (query ?? '')
        .split(/\s+/)
        .map((s) => s.trim())
        .filter(Boolean),
    [query],
  );
  const termsKey = terms.join('');
  // 検索一致の現在位置（1 始まり）と総数。
  const [match, setMatch] = useState({ idx: 0, total: 0 });
  const matchEls = useCallback(
    () =>
      Array.from(
        scrollRef.current?.querySelectorAll<HTMLElement>('[data-search-match]') ?? [],
      ),
    [],
  );
  const applyActive = useCallback(
    (i: number) => {
      const els = matchEls();
      els.forEach((e, k) => e.classList.toggle('search-hl-active', k === i));
      els[i]?.scrollIntoView({ block: 'center', behavior: 'smooth' });
    },
    [matchEls],
  );
  const gotoMatch = useCallback(
    (dir: 1 | -1) => {
      const total = matchEls().length;
      if (!total) return;
      const next = (match.idx - 1 + dir + total) % total; // 0 始まり
      applyActive(next);
      setMatch({ idx: next + 1, total });
    },
    [match.idx, matchEls, applyActive],
  );

  const load = useCallback(() => {
    setLoading(true);
    return threadView(openedId)
      .then((v) => {
        setView(v);
        return v;
      })
      .catch(() => setView(null))
      .finally(() => setLoading(false));
  }, [openedId]);

  useEffect(() => {
    setExpandedIds(new Set());
    load();
  }, [openedId, load]);

  // バブルの HTML 表示・埋め込み画像の設定変更に追従する。
  useEffect(() => {
    const onPrefs = () => {
      setHtmlBubbles(getBubbleHtml());
      setInlineImagesOn(getInlineImages());
    };
    window.addEventListener(PREFS_EVENT, onPrefs);
    return () => window.removeEventListener(PREFS_EVENT, onPrefs);
  }, []);

  // メール切替後、一番新しい（最後の）バブルが見えるようにスクロールする（チャット流儀）。
  // 一覧の代表はフォルダ内で最新のメールだが、会話には送信メールも時系列で並ぶため、代表
  // （openedId）を先頭合わせすると自分の最新返信などが下に隠れてしまう。最後のバブルの先頭を
  // 表示域の上端にそろえ、ただし最大スクロール量で頭打ちにする。こうすると、短いバブルは下端側
  // に収まって全体が見え、画面より長いバブルは先頭から読める（検索語があるときは下の効果で最初
  // の一致へ移動する）。
  useLayoutEffect(() => {
    if (!view || terms.length > 0) return;
    const el = scrollRef.current;
    if (!el) return;
    const bubbles = el.querySelectorAll<HTMLElement>('[id^="bubble-"]');
    const last = bubbles[bubbles.length - 1];
    if (!last) {
      el.scrollTop = el.scrollHeight;
      return;
    }
    const top = last.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop;
    el.scrollTop = Math.min(top, el.scrollHeight - el.clientHeight);
  }, [view, openedId, terms.length]);

  // 会話の描画後に検索一致を数え、最初の一致へ移動する（展開・語句変更にも追従）。
  useEffect(() => {
    if (!view || terms.length === 0) {
      setMatch({ idx: 0, total: 0 });
      return;
    }
    const id = window.setTimeout(() => {
      const total = matchEls().length;
      if (total > 0) {
        setMatch({ idx: 1, total });
        applyActive(0);
      } else {
        setMatch({ idx: 0, total: 0 });
      }
    }, 0);
    return () => window.clearTimeout(id);
    // termsKey で語句の実質変化のみに反応（配列の参照変化では動かさない）。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view, termsKey, expandedIds, matchEls, applyActive]);

  // 展開/折りたたみの直前に、基準バブルの現在の表示位置（コンテナ上端からの距離）を控える。
  const captureExpandAnchor = (id: number) => {
    const container = scrollRef.current;
    const el = document.getElementById(`bubble-${id}`);
    if (container && el) {
      expandAnchor.current = {
        id,
        top: el.getBoundingClientRect().top - container.getBoundingClientRect().top,
      };
    }
  };

  // 展開状態が変わった後、控えておいた基準バブルを同じ表示位置へ戻す
  // （中に入っても外に出ても、そのバブルはその場に留まり、版面が飛ばない）。
  useLayoutEffect(() => {
    const anchor = expandAnchor.current;
    const container = scrollRef.current;
    if (!anchor || !container) return;
    expandAnchor.current = null;
    const el = document.getElementById(`bubble-${anchor.id}`);
    if (!el) return;
    const now = el.getBoundingClientRect().top - container.getBoundingClientRect().top;
    container.scrollTop += now - anchor.top;
  }, [expandedIds]);

  // 全文展開はスレッド内で 1 通だけ。別のバブルを開くと、開いていた方は自動で畳む
  // （既に開いている同じバブルをもう一度押したら閉じる）。
  // 畳むときは基準バブルの表示位置を控えて版面が飛ばないようにする。開くときは、その本文を
  // 先頭から読めるよう、バブル側の効果でスクロール域の上側へ寄せるため、ここでは控えない。
  const toggleExpand = (id: number) => {
    if (expandedIds.has(id)) captureExpandAnchor(id);
    setExpandedIds((prev) => (prev.has(id) ? new Set() : new Set([id])));
  };

  // 分割: このメール（this）以降（below）を新スレッドへ切り出し、再読込＋一覧更新。
  const doSplit = async (messageId: number, mode: 'this' | 'below') => {
    try {
      await threadSplit(messageId, mode);
      await load();
      handlers.onThreadChanged?.();
    } catch {
      /* noop */
    }
  };

  // 分割・削除コールバックを handlers に足して Bubble へ渡す（親には露出しない内部結線）。
  // 削除は親（一覧の更新）を待ってから会話を再読込し、消えたメールを画面から外す。
  const bubbleHandlers: ConversationHandlers = {
    ...handlers,
    onThreadChangedSplit: doSplit,
    onDelete: async (id) => {
      await handlers.onDelete(id);
      await load();
    },
  };

  const saveTitle = async () => {
    if (!view) return;
    const clean = titleDraft.trim();
    try {
      await threadRename(view.thread.id, clean.length > 0 ? clean : null);
      setEditingTitle(false);
      await load();
      handlers.onThreadChanged?.();
    } catch {
      /* noop */
    }
  };

  if (loading && !view) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-white/40">
        {t('thread.loading')}
      </div>
    );
  }
  if (!view) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-white/40">
        {t('mailbox.selectMail')}
      </div>
    );
  }

  const { thread, messages } = view;
  // ゴミ箱/迷惑メール内のメールは、そのフォルダを見ているとき以外は会話から隠す
  // （削除・迷惑指定したメールが会話に残り続けないように）。
  const visibleMessages =
    folder === 'trash' || folder === 'spam'
      ? messages
      : messages.filter((m) => m.folder !== 'trash' && m.folder !== 'spam');
  const displayTitle = thread.title?.trim() || thread.auto_title?.trim() || t('thread.untitled');
  // 「自分」の表示名（送信メッセージのバブルの見出し）。
  const you = t('thread.you');

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* スレッド見出し（タイトル＝再件名・件数・参加者） */}
      <div className="shrink-0 border-b border-white/10 px-5 py-3">
        <div className="flex items-start justify-between gap-3">
          {editingTitle ? (
            <div className="flex min-w-0 flex-1 items-center gap-1">
              <input
                autoFocus
                value={titleDraft}
                onChange={(e) => setTitleDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') saveTitle();
                  if (e.key === 'Escape') setEditingTitle(false);
                }}
                placeholder={displayTitle}
                className="min-w-0 flex-1 rounded-md bg-white/10 px-2 py-1 text-base font-semibold outline-none focus:ring-1 focus:ring-sky-300/40"
              />
              <button
                onClick={saveTitle}
                title={t('thread.renameSave')}
                aria-label={t('thread.renameSave')}
                className="flex h-7 w-7 items-center justify-center rounded-md text-emerald-300 hover:bg-white/10"
              >
                <Check size={15} />
              </button>
              <button
                onClick={() => setEditingTitle(false)}
                title={t('account.cancel')}
                aria-label={t('account.cancel')}
                className="flex h-7 w-7 items-center justify-center rounded-md text-white/50 hover:bg-white/10"
              >
                <X size={15} />
              </button>
            </div>
          ) : (
            <button
              onClick={() => {
                setTitleDraft(thread.title ?? '');
                setEditingTitle(true);
              }}
              title={t('thread.rename')}
              className="group flex min-w-0 items-center gap-1.5 text-left"
            >
              <h3 className="min-w-0 truncate text-base font-semibold">{displayTitle}</h3>
              <Pencil
                size={13}
                className="shrink-0 text-white/30 opacity-0 transition-opacity group-hover:opacity-100"
              />
            </button>
          )}
          <span className="shrink-0 pt-1 text-[11px] text-white/40">
            {t('thread.messages', { count: thread.message_count })}
          </span>
        </div>
        {/* 元件名（再件名済みのときだけ小さく併記して迷子を防ぐ） */}
        {thread.is_user_renamed && thread.auto_title && (
          <div className="mt-0.5 truncate text-[11px] text-white/35">{thread.auto_title}</div>
        )}
      </div>

      {/* 時系列バブル（古い順・自分=右／相手=左）。検索中は一致移動バーを重ねる。 */}
      <div className="relative min-h-0 flex-1">
        {terms.length > 0 && match.total > 0 && (
          <div className="absolute right-4 top-2 z-20 flex items-center gap-1 rounded-full border border-white/10 bg-neutral-800/95 px-2 py-1 text-[11px] text-white/80 shadow-lg backdrop-blur">
            <span className="px-1 tabular-nums">
              {match.idx}/{match.total}
            </span>
            <button
              onClick={() => gotoMatch(-1)}
              title={t('search.prevMatch')}
              aria-label={t('search.prevMatch')}
              className="flex h-6 w-6 items-center justify-center rounded-full hover:bg-white/15"
            >
              <ChevronUp size={14} />
            </button>
            <button
              onClick={() => gotoMatch(1)}
              title={t('search.nextMatch')}
              aria-label={t('search.nextMatch')}
              className="flex h-6 w-6 items-center justify-center rounded-full hover:bg-white/15"
            >
              <ChevronDown size={14} />
            </button>
          </div>
        )}
        <div
          ref={scrollRef}
          data-conversation-scroll
          // 上パディングは付けない: 固定ラベルをスレッドヘッダ直下にピタッと貼り付け、ラベルより上に
          // 本文が漏れないようにする（先頭の余白は最初のバブルの mt で付ける）。
          className="h-full space-y-3 overflow-y-auto px-4 pb-4 pt-0 [&>*:first-child]:mt-3"
          onClick={(e) => {
            // 全文展開中に「バブル/カードの外＝余白」をクリックしたら畳んでバブルに戻す。
            // コンテンツ列（data-msg-content）の内側なら何もしない（展開・メール操作・閉じるボタンは従来どおり）。
            if (expandedIds.size === 0) return;
            if ((e.target as HTMLElement).closest('[data-msg-content]')) return;
            if (window.getSelection()?.toString()) return; // 文字選択の終端が余白でも畳まない
            const [openId] = expandedIds; // 展開中は 1 通のみ
            if (openId != null) captureExpandAnchor(openId);
            setExpandedIds(new Set());
          }}
        >
          {visibleMessages.map((m) => (
            <div key={m.id} id={`bubble-${m.id}`}>
              <Bubble
                m={m}
                you={you}
                handlers={bubbleHandlers}
                expanded={expandedIds.has(m.id)}
                onToggleExpand={() => toggleExpand(m.id)}
                highlight={terms}
                htmlBody={htmlBubbles}
                inlineImagesOn={inlineImagesOn}
              />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
