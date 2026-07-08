import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Check,
  ChevronDown,
  ChevronUp,
  Forward,
  Gem,
  LeafyGreen,
  Mail,
  MailOpen,
  MoreHorizontal,
  Paperclip,
  Pencil,
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
import { mailGet } from '../services/mail';
import { greenDomainAdd, greenDomainWarn } from '../services/green';
import { threadRename, threadSplit, threadView } from '../services/threads';
import { parseAddress } from '../utils/address';
import { getBubbleHtml, PREFS_EVENT } from '../config/prefs';
import { MailBody, makeRenderDate } from './MailBody';
import { AutoLinkText, HtmlText } from './HtmlText';
import { ContextMenu, type MenuItem } from './ContextMenu';
import type { CalendarPanelInitial } from './CalendarPanel';

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

function formatTime(d: string | null): string {
  if (!d) return '';
  const dt = new Date(d);
  return isNaN(dt.getTime()) ? d : dt.toLocaleString();
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
}: {
  m: ThreadMessage;
  you: string;
  handlers: ConversationHandlers;
  expanded: boolean;
  onToggleExpand: () => void;
  /** 検索語（複数）。本文中の一致をハイライトする。 */
  highlight?: string[];
  /** 設定オン時、body_html があればバブルを HTML 本文で描画する（画像はプレースホルダ）。 */
  htmlBody?: boolean;
}) {
  const { t } = useTranslation();
  const out = m.direction === 'out';
  const [showQuotes, setShowQuotes] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
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
  const toggleGreen = async () => {
    if (!senderDomain) return;
    const next = !isGreen;
    try {
      if (next) await greenDomainAdd(senderDomain);
      else await greenDomainWarn(senderDomain);
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

  const clean = (m.clean_body ?? '').trim();
  const full = (m.body_plain ?? '').trim();
  const body = showQuotes ? full : clean || full;
  // 設定オンで HTML 本文があるときは HtmlText で描画（画像は取得せずプレースホルダのまま）。
  // ただし HTML には引用除去版が無いので、引用のある返信（has_quotes）はチャット感を保つため
  // プレーン（新規部分のみ）にフォールバックする。実質「引用のないメールだけ HTML 描画」。
  const renderHtml = !!htmlBody && !!m.body_html?.trim() && !m.has_quotes;

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
      <div
        data-msg-content
        className={`group/bubble min-w-0 ${expanded ? 'w-full' : 'max-w-[82%]'}`}
        onContextMenu={(e) => {
          // 文字選択中はコピー等のネイティブメニューを優先する。
          if (window.getSelection()?.toString()) return;
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        {/* 差出人＋時刻（相手は左、自分は右にそろえる） */}
        <div
          className={`mb-0.5 flex items-center gap-1.5 px-1 text-[10px] text-white/45 ${
            out ? 'justify-end' : 'justify-start'
          }`}
        >
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
          <span className="shrink-0">{formatTime(m.date)}</span>
          {!read && <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-sky-400" />}
          {starred && <Star size={11} className="shrink-0 fill-amber-300 text-amber-300" />}
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
          // 通常バブル: clean_body（新規部分）だけを chat 風に。クリックで全文展開する
          // （文字選択中は展開しない。内部のボタンは stopPropagation で独立動作）。
          <div
            onClick={() => {
              if (window.getSelection()?.toString()) return;
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
              <HtmlText html={m.body_html as string} highlight={highlight} renderDate={renderDate} />
            ) : body ? (
              <AutoLinkText text={body} highlight={highlight} renderDate={renderDate} />
            ) : (
              <span className="text-white/40">{t('mailbox.noBody')}</span>
            )}

            {/* 操作行（フッター）: 引用トグル・添付・全文・メニュー */}
            <div
              className={`mt-1 flex items-center gap-2 text-[10px] text-white/45 ${
                out ? 'justify-end' : 'justify-start'
              }`}
            >
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
                <span className="inline-flex items-center gap-0.5">
                  <Paperclip size={11} />
                </span>
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
  // 既定は全メール折りたたみ（バブルのみ）。クリックしたメールだけ展開する。
  const [expandedIds, setExpandedIds] = useState<Set<number>>(new Set());
  // タイトル編集
  const [editingTitle, setEditingTitle] = useState(false);
  const [titleDraft, setTitleDraft] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);

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

  // バブルの HTML 表示設定の変更に追従する。
  useEffect(() => {
    const onPrefs = () => setHtmlBubbles(getBubbleHtml());
    window.addEventListener(PREFS_EVENT, onPrefs);
    return () => window.removeEventListener(PREFS_EVENT, onPrefs);
  }, []);

  // メール切替後、開いたメッセージの見出し（ラベル）を読書域の先頭にそろえてスクロールする
  // （検索語が無いときのみ。検索中は下の効果で最初の一致へ移動する）。
  // block:'center' だと長いメールで見出しが上に隠れ、毎回上へスクロールが要るため 'start' で先頭に。
  useEffect(() => {
    if (!view || terms.length > 0) return;
    const el = document.getElementById(`bubble-${openedId}`);
    el?.scrollIntoView({ block: 'start' });
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

  const toggleExpand = (id: number) =>
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

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
          className="h-full space-y-3 overflow-y-auto px-4 py-4"
          onClick={(e) => {
            // 全文展開中に「バブル/カードの外＝余白」をクリックしたら畳んでバブルに戻す。
            // コンテンツ列（data-msg-content）の内側なら何もしない（展開・メール操作・閉じるボタンは従来どおり）。
            if (expandedIds.size === 0) return;
            if ((e.target as HTMLElement).closest('[data-msg-content]')) return;
            if (window.getSelection()?.toString()) return; // 文字選択の終端が余白でも畳まない
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
              />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
