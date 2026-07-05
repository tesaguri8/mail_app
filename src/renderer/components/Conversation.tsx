import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Check,
  ChevronDown,
  Forward,
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
  Trash2,
  X,
} from 'lucide-react';
import type { ThreadView } from '@bindings/ThreadView';
import type { ThreadMessage } from '@bindings/ThreadMessage';
import type { MailDetail } from '@bindings/MailDetail';
import type { TagSummary } from '@bindings/TagSummary';
import { mailGet } from '../services/mail';
import { threadRename, threadSplit, threadView } from '../services/threads';
import { parseAddress } from '../utils/address';
import { MailBody } from './MailBody';
import { AutoLinkText } from './HtmlText';
import { ContextMenu, type MenuItem } from './ContextMenu';

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
  /** 単一メールの既読/未読切替（バブルの右クリックメニューから）。 */
  onSetRead: (id: number, read: boolean) => void;
  /** 単一メールをゴミ箱へ。会話側は実行後に会話を再読込する。 */
  onDelete: (id: number) => void | Promise<void>;
  /** そのメールに返信/転送（作成画面を開く）。 */
  onReply: (mode: 'reply' | 'replyAll' | 'forward', messageId: number) => void;
  onAddContact?: (name: string | null, email: string) => void;
  onEditContact?: (id: number) => void;
  onComposeTo?: (email: string) => void;
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
}: {
  m: ThreadMessage;
  you: string;
  handlers: ConversationHandlers;
  expanded: boolean;
  onToggleExpand: () => void;
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
    {
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
        className={`min-w-0 ${expanded ? 'w-full' : 'max-w-[82%]'}`}
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
          <span className="truncate font-medium text-white/60">{senderName(m, you)}</span>
          <span className="shrink-0">{formatTime(m.date)}</span>
          {!read && <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-sky-400" />}
          {starred && <Star size={11} className="shrink-0 fill-amber-300 text-amber-300" />}
        </div>

        {expanded && detail ? (
          // 全文表示: 従来の MailBody をそのまま埋め込む（添付・HTML・外部画像・タグ・スター）。
          // 閉じるボタンは上下両方に置き、長文でもスクロールせずに畳めるようにする。
          <div className="rounded-xl border border-white/15 bg-neutral-900/40">
            <div className="flex justify-end border-b border-white/10 px-3 py-1.5">
              <button
                onClick={onToggleExpand}
                className="flex items-center gap-1 text-[11px] text-white/50 hover:text-white/80"
              >
                <ChevronDown size={12} className="rotate-180" />
                {t('thread.collapse')}
              </button>
            </div>
            <MailBody
              detail={detail}
              tags={handlers.tagsFor(m.id)}
              starred={starred}
              onToggleStar={toggleStar}
              onTag={(x, y) => handlers.onTag(m.id, x, y)}
              onRemoveTag={(tagId) => handlers.onRemoveTag(m.id, tagId)}
              onReply={(mode) => handlers.onReply(mode, m.id)}
              onMarkSpam={() => handlers.onMarkSpam(m.id)}
              onAddContact={handlers.onAddContact}
              onEditContact={handlers.onEditContact}
              onComposeTo={handlers.onComposeTo}
              onGreenChange={handlers.onGreenChange}
            />
            <div className="flex justify-end border-t border-white/10 px-3 py-1.5">
              <button
                onClick={onToggleExpand}
                className="flex items-center gap-1 text-[11px] text-white/50 hover:text-white/80"
              >
                <ChevronDown size={12} className="rotate-180" />
                {t('thread.collapse')}
              </button>
            </div>
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
            {body ? (
              <AutoLinkText text={body} />
            ) : (
              <span className="text-white/40">{t('mailbox.noBody')}</span>
            )}

            {/* 操作行（フッター）: 引用トグル・添付・全文・メニュー */}
            <div
              className={`mt-1 flex items-center gap-2 text-[10px] text-white/45 ${
                out ? 'justify-end' : 'justify-start'
              }`}
            >
              {m.has_quotes && (
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
}: {
  openedId: number;
  /** 閲覧中のフォルダ。trash/spam を見ているとき以外は、それらのメールを会話から隠す。 */
  folder?: string;
  handlers: ConversationHandlers;
}) {
  const { t } = useTranslation();
  const [view, setView] = useState<ThreadView | null>(null);
  const [loading, setLoading] = useState(true);
  // 既定は全メール折りたたみ（バブルのみ）。クリックしたメールだけ展開する。
  const [expandedIds, setExpandedIds] = useState<Set<number>>(new Set());
  // タイトル編集
  const [editingTitle, setEditingTitle] = useState(false);
  const [titleDraft, setTitleDraft] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);

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

  // メール切替後、開いたメッセージまでスクロールする。
  useEffect(() => {
    if (!view) return;
    const el = document.getElementById(`bubble-${openedId}`);
    el?.scrollIntoView({ block: 'center' });
  }, [view, openedId]);

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

      {/* 時系列バブル（古い順・自分=右／相手=左） */}
      <div ref={scrollRef} className="min-h-0 flex-1 space-y-3 overflow-y-auto px-4 py-4">
        {visibleMessages.map((m) => (
          <div key={m.id} id={`bubble-${m.id}`}>
            <Bubble
              m={m}
              you={you}
              handlers={bubbleHandlers}
              expanded={expandedIds.has(m.id)}
              onToggleExpand={() => toggleExpand(m.id)}
            />
          </div>
        ))}
      </div>
    </div>
  );
}
