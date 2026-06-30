import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  BookmarkMinus,
  Bookmark,
  BookmarkPlus,
  Columns2,
  Flag,
  Mail,
  MailOpen,
  Paperclip,
  RefreshCw,
  Rows2,
  SquarePen,
  Star,
  StarOff,
  Tag,
  Trash2,
  UserRound,
  type LucideIcon,
} from 'lucide-react';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailSummary } from '@bindings/MailSummary';
import type { MailDetail } from '@bindings/MailDetail';
import type { TagSummary } from '@bindings/TagSummary';
import {
  mailDelete,
  mailGet,
  mailList,
  mailSetBookmarked,
  mailSetRead,
  mailSetStarred,
  mailSync,
} from '../services/mail';
import { mailAddTag, mailRemoveTag, tagCreate, tagList } from '../services/tags';
import { pickTagColor, DEFAULT_TAG_COLOR } from '../utils/tagColors';
import { MailBody } from './MailBody';
import { FolderCombobox } from './FolderCombobox';
import { ContextMenu, type MenuItem } from './ContextMenu';
import { DateFilter, matchesDate, type DateRange } from './DateFilter';
import { TagFilter, matchesTags } from './TagFilter';
import { TagPicker } from './TagPicker';

const iconBtn =
  'flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80 disabled:opacity-40';

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
  if (filters.has('star') && !m.is_starred) return false;
  if (filters.has('bookmark') && !m.is_bookmarked) return false;
  // known/flag は対応データが入るまでフィルタしない（空表示で混乱させない）
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
  // 期間フィルタ（以降/以前/期間）
  const [dateFilter, setDateFilter] = useState<DateRange | null>(null);
  // タグ（一覧データ・絞り込み条件・付与ポップオーバー位置）
  const [tags, setTags] = useState<TagSummary[]>([]);
  const [tagFilter, setTagFilter] = useState<Set<number>>(new Set());
  const [tagPicker, setTagPicker] = useState<{ x: number; y: number } | null>(null);
  const toggleFilter = (key: string) =>
    setFilters((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  // 複数選択（右クリックメニュー対象）。anchor は Shift 範囲選択の基点。
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const anchorId = useRef<number | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    if (selected == null && accounts.length > 0) setSelected(accounts[0].id);
  }, [accounts, selected]);

  // タグ一覧（チップ表示・絞り込み・付与候補の元データ）
  const reloadTags = () => tagList().then(setTags).catch(() => undefined);
  useEffect(() => {
    reloadTags();
  }, []);
  const tagById = new Map(tags.map((tg) => [tg.id, tg]));

  const loadMails = (id: number) => mailList(id, 200).then(setMails).catch(() => undefined);
  useEffect(() => {
    setOpened(null);
    setSelectedIds(new Set());
    anchorId.current = null;
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

  // 行クリック: 通常=単一選択して開く / Ctrl(Cmd)=トグル / Shift=範囲
  const onRowClick = (e: React.MouseEvent, id: number) => {
    if (e.ctrlKey || e.metaKey) {
      setSelectedIds((prev) => {
        const next = new Set(prev);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      });
      anchorId.current = id;
      return;
    }
    if (e.shiftKey && anchorId.current != null) {
      const order = visibleMails.map((m) => m.id);
      const a = order.indexOf(anchorId.current);
      const b = order.indexOf(id);
      if (a !== -1 && b !== -1) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        setSelectedIds(new Set(order.slice(lo, hi + 1)));
        return;
      }
    }
    setSelectedIds(new Set([id]));
    anchorId.current = id;
    openMail(id);
  };

  const onRowContextMenu = (e: React.MouseEvent, id: number) => {
    e.preventDefault();
    if (!selectedIds.has(id)) {
      setSelectedIds(new Set([id]));
      anchorId.current = id;
    }
    setMenu({ x: e.clientX, y: e.clientY });
  };

  const patchMails = (ids: Set<number>, patch: Partial<MailSummary>) =>
    setMails((prev) => prev.map((m) => (ids.has(m.id) ? { ...m, ...patch } : m)));

  const targetIds = () => [...selectedIds];

  const actRead = async (read: boolean) => {
    const ids = targetIds();
    patchMails(selectedIds, { is_read: read });
    try {
      await mailSetRead(ids, read);
    } catch {
      /* noop */
    }
  };
  const actStar = async (value: boolean) => {
    const ids = targetIds();
    patchMails(selectedIds, { is_starred: value });
    try {
      await mailSetStarred(ids, value);
    } catch {
      /* noop */
    }
  };
  const actBookmark = async (value: boolean) => {
    const ids = targetIds();
    patchMails(selectedIds, { is_bookmarked: value });
    try {
      await mailSetBookmarked(ids, value);
    } catch {
      /* noop */
    }
  };
  const actDelete = async () => {
    const ids = targetIds();
    const idSet = new Set(ids);
    setMails((prev) => prev.filter((m) => !idSet.has(m.id)));
    if (opened && idSet.has(opened.id)) setOpened(null);
    setSelectedIds(new Set());
    try {
      await mailDelete(ids);
    } catch {
      /* noop */
    }
  };

  // 選択メール群へタグを付与/解除（楽観更新 → 永続化）。
  const applyTagDelta = async (ids: number[], tagId: number, add: boolean) => {
    const idSet = new Set(ids);
    setMails((prev) =>
      prev.map((m) => {
        if (!idSet.has(m.id)) return m;
        const has = m.tag_ids.includes(tagId);
        if (add && !has) return { ...m, tag_ids: [...m.tag_ids, tagId] };
        if (!add && has) return { ...m, tag_ids: m.tag_ids.filter((id) => id !== tagId) };
        return m;
      })
    );
    try {
      await (add ? mailAddTag(ids, tagId) : mailRemoveTag(ids, tagId));
      reloadTags(); // 件数表示を更新
    } catch {
      /* noop */
    }
  };

  // 新規タグを作成して選択メールに付与。
  const createAndAssign = async (name: string) => {
    try {
      const created = await tagCreate(name, pickTagColor(tags.length));
      setTags((prev) => [...prev, created]);
      await applyTagDelta(targetIds(), created.id, true);
    } catch {
      /* noop */
    }
  };

  // 選択集合の状態に応じてメニュー項目（トグルラベル）を組み立てる
  const buildMenuItems = (): MenuItem[] => {
    const sel = mails.filter((m) => selectedIds.has(m.id));
    const allStarred = sel.length > 0 && sel.every((m) => m.is_starred);
    const allBookmarked = sel.length > 0 && sel.every((m) => m.is_bookmarked);
    return [
      { key: 'read', label: t('ctx.markRead'), Icon: MailOpen, onClick: () => actRead(true) },
      { key: 'unread', label: t('ctx.markUnread'), Icon: Mail, onClick: () => actRead(false) },
      allStarred
        ? { key: 'unstar', label: t('ctx.unstar'), Icon: StarOff, onClick: () => actStar(false) }
        : { key: 'star', label: t('ctx.star'), Icon: Star, onClick: () => actStar(true) },
      allBookmarked
        ? {
            key: 'unbookmark',
            label: t('ctx.unbookmark'),
            Icon: BookmarkMinus,
            onClick: () => actBookmark(false),
          }
        : {
            key: 'bookmark',
            label: t('ctx.bookmark'),
            Icon: BookmarkPlus,
            onClick: () => actBookmark(true),
          },
      {
        key: 'tags',
        label: t('ctx.tags'),
        Icon: Tag,
        onClick: () => {
          if (menu) setTagPicker({ x: menu.x, y: menu.y });
        },
      },
      { key: 'delete', label: t('ctx.delete'), Icon: Trash2, danger: true, onClick: actDelete },
    ];
  };

  if (accounts.length === 0) {
    return <div className="p-8 text-white/60">{t('mailbox.addInSettings')}</div>;
  }

  const visibleMails = mails.filter(
    (m) =>
      matchesFilters(m, filters) &&
      matchesDate(m.date, dateFilter) &&
      matchesTags(m.tag_ids, tagFilter)
  );

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
            onClick={(e) => onRowClick(e, m.id)}
            onContextMenu={(e) => onRowContextMenu(e, m.id)}
            className={`cursor-pointer select-none rounded-md px-3 py-2 hover:bg-white/10 ${
              selectedIds.has(m.id) ? 'bg-white/15' : ''
            } ${opened?.id === m.id ? 'ring-1 ring-sky-300/40' : ''}`}
          >
            <div className="flex items-baseline justify-between gap-2">
              <span className="truncate text-sm font-medium">
                {!m.is_read && <span className="mr-1 text-sky-300">●</span>}
                {m.from_address ?? '(no sender)'}
              </span>
              <span className="flex shrink-0 items-center gap-1 text-[10px] text-white/40">
                {m.is_starred && <Star size={12} className="fill-amber-300 text-amber-300" />}
                {m.is_bookmarked && <Bookmark size={12} className="fill-sky-300 text-sky-300" />}
                {formatDate(m.date)}
              </span>
            </div>
            <div className="truncate text-sm text-white/80">
              {m.subject ?? '(no subject)'} {m.has_attachments && '📎'}
            </div>
            <div className="line-clamp-1 text-xs text-white/40">{m.preview}</div>
            {m.tag_ids.length > 0 && (
              <div className="mt-1 flex flex-wrap gap-1">
                {m.tag_ids.map((tid) => {
                  const tg = tagById.get(tid);
                  if (!tg) return null;
                  const color = tg.color ?? DEFAULT_TAG_COLOR;
                  return (
                    <span
                      key={tid}
                      className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium"
                      style={{ backgroundColor: `${color}33`, color }}
                    >
                      <span
                        className="h-1.5 w-1.5 rounded-full"
                        style={{ backgroundColor: color }}
                      />
                      {tg.name}
                    </span>
                  );
                })}
              </div>
            )}
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
        {/* 新規作成（新規｜未読 のように配置） */}
        <button
          className={iconBtn}
          onClick={() => setStatus(t('comingSoon'))}
          title={t('compose.new')}
          aria-label={t('compose.new')}
        >
          <SquarePen size={16} />
        </button>
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
              <span className="relative inline-flex">
                <Icon size={15} />
                {key === 'unread' && (
                  <span className="absolute -right-1 -top-1 h-1.5 w-1.5 rounded-full bg-sky-400 ring-1 ring-neutral-900/60" />
                )}
              </span>
            </button>
          );
        })}
        <DateFilter value={dateFilter} onChange={setDateFilter} />
        <TagFilter tags={tags} value={tagFilter} onChange={setTagFilter} />
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

      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          header={selectedIds.size > 1 ? t('ctx.selected', { count: selectedIds.size }) : undefined}
          items={buildMenuItems()}
          onClose={() => setMenu(null)}
        />
      )}

      {tagPicker && (
        <TagPicker
          x={tagPicker.x}
          y={tagPicker.y}
          tags={tags}
          selectedMails={mails.filter((m) => selectedIds.has(m.id))}
          onToggle={(tagId, add) => applyTagDelta(targetIds(), tagId, add)}
          onCreate={createAndAssign}
          onClose={() => setTagPicker(null)}
        />
      )}
    </div>
  );
}
