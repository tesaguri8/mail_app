import { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Ban,
  Columns2,
  Flag,
  Mail,
  MailOpen,
  Paperclip,
  RefreshCw,
  Rows2,
  Search,
  SquarePen,
  Star,
  StarOff,
  Tag,
  Trash2,
  UserRound,
  X,
  type LucideIcon,
} from 'lucide-react';
import { listen } from '@tauri-apps/api/event';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailSummary } from '@bindings/MailSummary';
import type { MailDetail } from '@bindings/MailDetail';
import type { TagSummary } from '@bindings/TagSummary';
import type { SyncProgress } from '@bindings/SyncProgress';
import type { RecipientSuggestion } from '@bindings/RecipientSuggestion';
import {
  mailDelete,
  mailGet,
  mailList,
  mailMarkSpam,
  mailSearch,
  mailSetRead,
  mailSetStarred,
  mailSync,
} from '../services/mail';
import { recipientSuggest } from '../services/recipients';
import { RecipientSuggestList } from './RecipientSuggestList';
import { mailAddTag, mailRemoveTag, tagCreate, tagList } from '../services/tags';
import { pickTagColor } from '../utils/tagColors';
import { MailBody } from './MailBody';
import { Compose, type ComposeTarget } from './Compose';
import { FolderCombobox } from './FolderCombobox';
import { ContextMenu, type MenuItem } from './ContextMenu';
import { DateFilter, matchesDate, type DateRange } from './DateFilter';
import { TagFilter, matchesTags } from './TagFilter';
import { TagPicker } from './TagPicker';

const iconBtn =
  'flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80 disabled:opacity-40';

/** リスト絞り込みのトグル。known/flag はバックエンド実装まで非適用（並びのみ）。 */
const FILTERS: { key: string; Icon: LucideIcon }[] = [
  { key: 'unread', Icon: Mail },
  { key: 'star', Icon: Star },
  { key: 'known', Icon: UserRound },
  { key: 'attachment', Icon: Paperclip },
  { key: 'flag', Icon: Flag },
];

function matchesFilters(m: MailSummary, filters: Set<string>): boolean {
  if (filters.has('unread') && m.is_read) return false;
  if (filters.has('attachment') && !m.has_real_attachments) return false;
  if (filters.has('star') && !m.is_starred) return false;
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
  const [progress, setProgress] = useState<SyncProgress | null>(null);
  const [status, setStatus] = useState('');
  const [layout, setLayout] = useState<'side' | 'top'>('side');
  // メール作成モーダル（新規／返信／転送）。null なら閉じている。
  const [compose, setCompose] = useState<ComposeTarget | null>(null);
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
  // 全文検索（件名・差出人・本文）。query が空でなければ検索モード。
  const [query, setQuery] = useState('');
  const [searchResults, setSearchResults] = useState<MailSummary[]>([]);
  const [searching, setSearching] = useState(false);
  const searchMode = query.trim().length > 0;
  // 検索窓の入力補助: 住所録＋履歴の候補ドロップダウン。
  // sugActive=-1 はハイライト無し（Enter は候補を拾わず通常検索のまま）。
  const [sug, setSug] = useState<RecipientSuggestion[]>([]);
  const [sugOpen, setSugOpen] = useState(false);
  const [sugActive, setSugActive] = useState(-1);
  const sugPicked = useRef(false);
  const searchListId = useId();
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
  // 選択モード（チェックボックス表示中）。1件でも明示選択したら on。
  const [selecting, setSelecting] = useState(false);
  // 選択が空になったら選択モードを抜ける。
  useEffect(() => {
    if (selectedIds.size === 0) setSelecting(false);
  }, [selectedIds]);

  // Esc で複数選択を解除する。重なり UI（メニュー/タグピッカー/作成モーダル）が
  // 開いている間はそちらの Esc を優先し、入力欄フォーカス中（検索クリア等）も対象外。
  useEffect(() => {
    if (selectedIds.size === 0) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented) return;
      if (menu || tagPicker || compose) return;
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable))
        return;
      setSelectedIds(new Set());
      anchorId.current = null;
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [selectedIds.size, menu, tagPicker, compose]);

  useEffect(() => {
    if (selected == null && accounts.length > 0) setSelected(accounts[0].id);
  }, [accounts, selected]);

  // タグ一覧（チップ表示・絞り込み・付与候補の元データ）
  const reloadTags = () => tagList().then(setTags).catch(() => undefined);
  useEffect(() => {
    reloadTags();
  }, []);
  const tagById = new Map(tags.map((tg) => [tg.id, tg]));

  const loadMails = (id: number) =>
    mailList(id, folder, 200).then(setMails).catch(() => undefined);
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
  }, [selected, folder]);

  // 通常一覧・検索結果の両方へ同じ更新（既読/スター/削除/タグ）を反映する。
  const updateLists = (fn: (list: MailSummary[]) => MailSummary[]) => {
    setMails(fn);
    setSearchResults(fn);
  };

  // 全文検索: 入力を 250ms デバウンスして呼ぶ。アカウント/フォルダ切替でも再実行。
  useEffect(() => {
    const q = query.trim();
    if (!q || selected == null) {
      setSearchResults([]);
      setSearching(false);
      return;
    }
    setSearching(true);
    const h = setTimeout(() => {
      mailSearch(selected, folder, q, 200)
        .then(setSearchResults)
        .catch(() => setSearchResults([]))
        .finally(() => setSearching(false));
    }, 250);
    return () => clearTimeout(h);
  }, [query, selected, folder]);

  // 検索窓の入力補助: 入力に一致する住所録＋履歴の候補を出す（選ぶとアドレスで検索）。
  // スペースを含む入力（2語目以降）はオートコンプリート解除＝候補を出さない。
  useEffect(() => {
    const q = query.trim();
    if (sugPicked.current) {
      sugPicked.current = false;
      return;
    }
    if (q.length < 1 || /\s/.test(query)) {
      setSug([]);
      setSugOpen(false);
      return;
    }
    const h = setTimeout(() => {
      recipientSuggest(q, 6)
        .then((r) => {
          setSug(r);
          setSugActive(-1); // 自動ハイライトしない（Enter の誤確定を防ぐ）
          setSugOpen(r.length > 0);
        })
        .catch(() => {
          setSug([]);
          setSugOpen(false);
        });
    }, 200);
    return () => clearTimeout(h);
  }, [query]);

  // 候補を選ぶ: そのメールアドレスで検索する（再クエリは抑止してドロップダウンを閉じる）。
  const pickSuggest = (s: RecipientSuggestion) => {
    sugPicked.current = true;
    setQuery(s.email);
    setSug([]);
    setSugOpen(false);
    setSugActive(-1);
  };

  // 検索窓のキー操作: 候補表示中は ↑↓ でハイライト移動、Enter は「↑↓で選んだ時だけ」確定
  // （未選択の Enter は閉じるだけ＝通常の全文検索を邪魔しない）。Esc は閉じる→クリアの順。
  const onSearchKeyDown = (e: React.KeyboardEvent) => {
    if (sugOpen && sug.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSugActive((i) => (i + 1) % sug.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSugActive((i) => (i < 0 ? sug.length - 1 : (i - 1 + sug.length) % sug.length));
        return;
      }
      if (e.key === 'Enter') {
        if (sugActive >= 0) pickSuggest(sug[sugActive]);
        else setSugOpen(false);
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        setSugOpen(false);
        return;
      }
    } else if (e.key === 'Escape') {
      setQuery('');
    }
  };

  // 開いたメッセージをリスト内でフォーカス（スクロール）
  useEffect(() => {
    if (opened?.id != null) {
      document.getElementById(`mail-li-${opened.id}`)?.scrollIntoView({ block: 'nearest' });
    }
  }, [opened?.id]);

  const onSync = async () => {
    if (selected == null) return;
    setSyncing(true);
    setProgress(null);
    setStatus(t('mailbox.syncing'));
    // Rust からの "sync:progress" を購読して、フォルダ別の取得状況を表示する。
    const unlisten = await listen<SyncProgress>('sync:progress', (e) => setProgress(e.payload));
    try {
      const r = await mailSync(selected);
      setStatus(t('mailbox.result', { fetched: r.fetched, stored: r.stored }));
      await loadMails(selected);
    } catch (e) {
      setStatus('✕ ' + String(e));
    } finally {
      unlisten();
      setProgress(null);
      setSyncing(false);
    }
  };

  const openMail = async (id: number) => {
    try {
      const d = await mailGet(id);
      setOpened(d);
      updateLists((prev) => prev.map((m) => (m.id === id ? { ...m, is_read: true } : m)));
    } catch {
      /* noop */
    }
  };

  // 行クリック:
  // - Shift=範囲選択 / Ctrl(Cmd)=トグル
  // - 複数選択モード（チェックボックス表示中）は、修飾キーなしのクリックでもトグル
  // - 通常時（単一）はクリックで開く
  const onRowClick = (e: React.MouseEvent, id: number) => {
    if (e.shiftKey && anchorId.current != null) {
      const order = visibleMails.map((m) => m.id);
      const a = order.indexOf(anchorId.current);
      const b = order.indexOf(id);
      if (a !== -1 && b !== -1) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        setSelecting(true);
        setSelectedIds(new Set(order.slice(lo, hi + 1)));
        return;
      }
    }
    if (e.ctrlKey || e.metaKey || selecting) {
      toggleSelect(id);
      return;
    }
    // 通常クリック: 選択をクリアして開く（ハイライトは opened で行う）
    setSelectedIds(new Set());
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

  // チェックボックスでの単純トグル（開かない）。選択モードに入る。
  const toggleSelect = (id: number) => {
    setSelecting(true);
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
    anchorId.current = id;
  };

  const patchMails = (ids: Set<number>, patch: Partial<MailSummary>) =>
    updateLists((prev) => prev.map((m) => (ids.has(m.id) ? { ...m, ...patch } : m)));

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
  const actDelete = async () => {
    const ids = targetIds();
    const idSet = new Set(ids);
    updateLists((prev) => prev.filter((m) => !idSet.has(m.id)));
    if (opened && idSet.has(opened.id)) setOpened(null);
    setSelectedIds(new Set());
    try {
      await mailDelete(ids);
    } catch {
      /* noop */
    }
  };
  // 迷惑としてマーク: 学習＋隔離。楽観更新で受信一覧から外す（迷惑フォルダへ）。
  const actMarkSpam = async () => {
    const ids = targetIds();
    const idSet = new Set(ids);
    updateLists((prev) => prev.filter((m) => !idSet.has(m.id)));
    if (opened && idSet.has(opened.id)) setOpened(null);
    setSelectedIds(new Set());
    try {
      await mailMarkSpam(ids);
    } catch {
      /* noop */
    }
  };

  // 選択メール群へタグを付与/解除（楽観更新 → 永続化）。
  const applyTagDelta = async (ids: number[], tagId: number, add: boolean) => {
    const idSet = new Set(ids);
    updateLists((prev) =>
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
    return [
      { key: 'read', label: t('ctx.markRead'), Icon: MailOpen, onClick: () => actRead(true) },
      { key: 'unread', label: t('ctx.markUnread'), Icon: Mail, onClick: () => actRead(false) },
      allStarred
        ? { key: 'unstar', label: t('ctx.unstar'), Icon: StarOff, onClick: () => actStar(false) }
        : { key: 'star', label: t('ctx.star'), Icon: Star, onClick: () => actStar(true) },
      {
        key: 'tags',
        label: t('ctx.tags'),
        Icon: Tag,
        onClick: () => {
          if (menu) setTagPicker({ x: menu.x, y: menu.y });
        },
      },
      { key: 'spam', label: t('ctx.markSpam'), Icon: Ban, onClick: actMarkSpam },
      { key: 'delete', label: t('ctx.delete'), Icon: Trash2, danger: true, onClick: actDelete },
    ];
  };

  if (accounts.length === 0) {
    return <div className="p-8 text-white/60">{t('mailbox.addInSettings')}</div>;
  }

  // 検索モードでは FTS 結果を、通常は読み込み済み一覧を対象に、
  // 既存の絞り込み（トグル/期間/タグ）を重ねて表示する。
  const visibleMails = (searchMode ? searchResults : mails).filter(
    (m) =>
      matchesFilters(m, filters) &&
      matchesDate(m.date, dateFilter) &&
      matchesTags(m.tag_ids, tagFilter)
  );

  // 選択モード中はチェックボックスを表示して選択を簡単にする。
  const allVisibleSelected =
    visibleMails.length > 0 && visibleMails.every((m) => selectedIds.has(m.id));
  const someVisibleSelected = selectedIds.size > 0 && !allVisibleSelected;
  const toggleAllVisible = () => {
    setSelectedIds(allVisibleSelected ? new Set() : new Set(visibleMails.map((m) => m.id)));
  };

  // 送信済・下書きは自分が差出人なので、一覧では宛先(To)を主に見せる。
  const outgoing = folder === 'sent' || folder === 'drafts';

  const listPane = (
    <div className="flex h-full min-h-0 flex-col">
      {/* 絞り込みツールバー: 一覧を絞る操作はリスト直上に置く（トグル/期間/タグ） */}
      <div className="flex shrink-0 flex-wrap items-center gap-1 border-b border-white/10 px-2 py-1">
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
      </div>
      {selecting && (
        <div className="flex items-center gap-2 border-b border-white/10 px-3 py-2 text-xs text-white/60">
          <input
            type="checkbox"
            checked={allVisibleSelected}
            ref={(el) => {
              if (el) el.indeterminate = someVisibleSelected;
            }}
            onChange={toggleAllVisible}
            title={t('mailbox.selectAll')}
            className="h-3.5 w-3.5 shrink-0 accent-sky-400"
          />
          <span className="flex-1">{t('ctx.selected', { count: selectedIds.size })}</span>
          <button onClick={() => setSelectedIds(new Set())} className="hover:text-white/90">
            {t('mailbox.clearSelection')}
          </button>
        </div>
      )}
      <ul className="min-h-0 flex-1 space-y-1 overflow-y-auto p-2">
      {visibleMails.length === 0 ? (
        <li className="px-2 py-3 text-sm text-white/50">
          {searchMode ? (searching ? t('search.searching') : t('search.noResults')) : t('mailbox.empty')}
        </li>
      ) : (
        visibleMails.map((m) => (
          <li
            key={m.id}
            id={`mail-li-${m.id}`}
            onClick={(e) => onRowClick(e, m.id)}
            onContextMenu={(e) => onRowContextMenu(e, m.id)}
            className={`group flex cursor-pointer select-none gap-2 rounded-md px-3 py-2 hover:bg-white/10 ${
              selectedIds.has(m.id) ? 'bg-white/15' : ''
            } ${opened?.id === m.id ? 'ring-1 ring-sky-300/40' : ''}`}
          >
            <input
              type="checkbox"
              checked={selectedIds.has(m.id)}
              onChange={() => toggleSelect(m.id)}
              onClick={(e) => e.stopPropagation()}
              aria-label={t('mailbox.selectMailCheckbox')}
              className={`mt-1 h-3.5 w-3.5 shrink-0 accent-sky-400 ${
                selecting || selectedIds.has(m.id) ? '' : 'opacity-0 group-hover:opacity-100'
              }`}
            />
            <div className="min-w-0 flex-1">
            <div className="flex items-baseline justify-between gap-2">
              <span className="truncate text-sm font-medium">
                {!m.is_read && <span className="mr-1 text-sky-300">●</span>}
                {outgoing
                  ? `${t('mailbox.to')}: ${m.to_addresses ?? '—'}`
                  : (m.from_address ?? '(no sender)')}
              </span>
              <span className="flex shrink-0 items-center gap-1 text-[10px] text-white/40">
                {m.is_starred && <Star size={12} className="fill-amber-300 text-amber-300" />}
                {formatDate(m.date)}
              </span>
            </div>
            <div className="truncate text-sm text-white/80">
              {m.subject ?? '(no subject)'} {m.has_real_attachments && '📎'}
            </div>
            <div className="line-clamp-1 text-xs text-white/40">{m.preview}</div>
            </div>
          </li>
        ))
      )}
      </ul>
    </div>
    );

  // 開いているメールのタグ（詳細ヘッダに表示。MailDetail はタグを持たないため一覧側から解決）。
  const openedTags = opened
    ? ((mails.find((m) => m.id === opened.id) ?? searchResults.find((m) => m.id === opened.id))
        ?.tag_ids ?? [])
        .map((tid) => tagById.get(tid))
        .filter((tg): tg is TagSummary => tg != null)
    : [];

  // 開いているメールを迷惑としてマーク（学習＋隔離）。一覧から外して詳細を閉じる。
  const markSpamOpened = async () => {
    if (!opened) return;
    const id = opened.id;
    updateLists((prev) => prev.filter((m) => m.id !== id));
    setOpened(null);
    try {
      await mailMarkSpam([id]);
    } catch {
      /* noop */
    }
  };

  const bodyPane = opened ? (
    <MailBody
      detail={opened}
      tags={openedTags}
      onReply={(mode) => setCompose({ mode, source: opened })}
      onMarkSpam={markSpamOpened}
    />
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

        {/* 全文検索: 件名・差出人・本文を対象。入力はデバウンスして検索。
            入力補助として住所録＋履歴の候補を出し、選ぶとそのアドレスで検索する。 */}
        <div className="relative flex items-center">
          <Search
            size={13}
            className={`pointer-events-none absolute left-2 ${
              searching ? 'animate-pulse text-sky-300' : 'text-white/40'
            }`}
          />
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onSearchKeyDown}
            onFocus={() => sug.length > 0 && query.trim().length >= 1 && setSugOpen(true)}
            onBlur={() => setTimeout(() => setSugOpen(false), 120)}
            placeholder={t('search.placeholder')}
            aria-label={t('search.placeholder')}
            role="combobox"
            aria-expanded={sugOpen}
            aria-controls={searchListId}
            aria-autocomplete="list"
            className="w-44 rounded-md bg-white/10 py-1 pl-7 pr-7 text-xs outline-none placeholder:text-white/35 focus:w-56 focus:ring-1 focus:ring-sky-300/40"
          />
          {query && (
            <button
              onClick={() => setQuery('')}
              title={t('search.clear')}
              aria-label={t('search.clear')}
              className="absolute right-1.5 flex h-4 w-4 items-center justify-center rounded text-white/45 hover:text-white/80"
            >
              <X size={12} />
            </button>
          )}
          {sugOpen && sug.length > 0 && (
            <RecipientSuggestList
              items={sug}
              active={sugActive}
              onPick={pickSuggest}
              onHover={setSugActive}
              listId={searchListId}
              className="absolute left-0 top-full mt-1 min-w-[16rem]"
            />
          )}
        </div>

        <span className="mx-1 h-5 w-px bg-white/15" />
        {/* 新規作成 */}
        <button
          className={iconBtn}
          onClick={() => setCompose({ mode: 'new' })}
          title={t('compose.new')}
          aria-label={t('compose.new')}
        >
          <SquarePen size={16} />
        </button>
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
        {syncing && progress ? (
          <span className="flex items-center gap-2 text-xs text-white/60">
            <span className="tabular-nums">
              {t(`mailbox.f_${progress.folder}`, progress.folder)}: {progress.current}/
              {progress.total}
            </span>
            <span className="h-1 w-24 overflow-hidden rounded bg-white/15">
              <span
                className="block h-full bg-sky-400 transition-[width]"
                style={{
                  width: `${progress.total > 0 ? Math.min(100, Math.round((progress.current / progress.total) * 100)) : 0}%`,
                }}
              />
            </span>
          </span>
        ) : (
          status && <span className="text-xs text-white/60">{status}</span>
        )}
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

      {compose && (
        <Compose
          accounts={accounts}
          defaultAccountId={selected}
          target={compose}
          onClose={() => setCompose(null)}
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
