import { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Columns2,
  Gem,
  LeafyGreen,
  Mail,
  MailOpen,
  PanelLeftClose,
  PanelLeftOpen,
  RefreshCw,
  Rows2,
  Search,
  SquarePen,
  Star,
  StarOff,
  Tag,
  ThumbsDown,
  Trash2,
  X,
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
  mailEmptyFolder,
  mailGet,
  mailList,
  mailMarkSpam,
  mailSearch,
  mailSetRead,
  mailSetStarred,
  mailSync,
} from '../services/mail';
import { recipientSuggest } from '../services/recipients';
import { MAIL_SYNCED_EVENT } from '../hooks/useAutoSync';
import { RecipientSuggestList } from './RecipientSuggestList';
import { mailAddTag, mailRemoveTag, tagCreate, tagList } from '../services/tags';
import { pickTagColor, DEFAULT_TAG_COLOR } from '../utils/tagColors';
import { parseAddress } from '../utils/address';
import { MailBody } from './MailBody';
import { Compose, type ComposeTarget } from './Compose';
import { FolderIcons } from './FolderIcons';
import { MAIL_FILTERS, matchesFilters } from './mailFilters';
import { ContextMenu, type MenuItem } from './ContextMenu';
import { DateFilter, matchesDate, type DateRange } from './DateFilter';
import { TagFilter, matchesTags } from './TagFilter';
import { TagPicker } from './TagPicker';

const iconBtn =
  'flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80 disabled:opacity-40';

// サイドバー（メール一覧）の幅。ドラッグで可変。ここを変えれば最小幅/初期幅を一括変更できる。
export const MIN_SIDEBAR_WIDTH = 340;
export const MAX_SIDEBAR_WIDTH = 640;
export const DEFAULT_SIDEBAR_WIDTH = 340;

function formatDate(d: string | null): string {
  if (!d) return '';
  const dt = new Date(d);
  return isNaN(dt.getTime()) ? d : dt.toLocaleString();
}

/** スクロール位置インジケータ用の日付ラベル（例: 2026/7/2）。 */
function formatScrollDate(d: string | null): string {
  if (!d) return '';
  const dt = new Date(d);
  return isNaN(dt.getTime())
    ? ''
    : `${dt.getFullYear()}/${dt.getMonth() + 1}/${dt.getDate()}`;
}

/**
 * メールモード: 全幅。リスト＋本文。レイアウトは左右/上下を切替可能。
 */
export function MailboxView({
  accounts,
  initialAccountId,
  initialMailId,
  onAccountChange,
  onAddContact,
  onOpenContact,
}: {
  accounts: AccountSummary[];
  initialAccountId: number | null;
  initialMailId: number | null;
  /** 表示中アカウントの変化を親へ通知（フッターの件数表示用）。'all'=全アカウント。 */
  onAccountChange?: (id: number | 'all' | null) => void;
  /** メールのアドレスから住所録へ追加（名前・メールを渡す）。 */
  onAddContact?: (name: string | null, email: string) => void;
  /** メールのアドレスから既存連絡先を開く（編集アイコン）。 */
  onOpenContact?: (id: number) => void;
}) {
  const { t } = useTranslation();
  // 'all' = 全アカウント横断表示 / number = 特定アカウント / null = 未選択。
  const [selected, setSelected] = useState<number | 'all' | null>(
    initialAccountId ?? accounts[0]?.id ?? null
  );
  // クエリに渡すアカウント（number のみ。'all'/null は null=全アカウント）。
  const queryAccount = typeof selected === 'number' ? selected : null;
  // 表示中アカウントを親へ通知（フッターのメール総数表示）。
  useEffect(() => {
    onAccountChange?.(selected);
  }, [selected, onAccountChange]);
  // 遷移直後に開くべきメッセージ（ホームの新着クリック）
  const pendingOpen = useRef<number | null>(initialMailId);
  const [mails, setMails] = useState<MailSummary[]>([]);
  const [opened, setOpened] = useState<MailDetail | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [progress, setProgress] = useState<SyncProgress | null>(null);
  const [status, setStatus] = useState('');
  const [layout, setLayout] = useState<'side' | 'top'>('side');
  // サイドバー（メール一覧ペイン）の表示 ON/OFF。ツールバーのアイコン／Ctrl+S で切替。
  const [sidebarOpen, setSidebarOpen] = useState(true);
  // サイドバー幅（ドラッグで可変・localStorage 永続）。最小/最大は上部の定数で一括変更可。
  const [sidebarW, setSidebarW] = useState(() => {
    const saved = Number(localStorage.getItem('rondine.mailSidebarW'));
    return Number.isFinite(saved) && saved >= MIN_SIDEBAR_WIDTH ? saved : DEFAULT_SIDEBAR_WIDTH;
  });
  const splitRef = useRef<HTMLDivElement>(null);
  // 矢印キー移動用の「最新値」ref（早期 return より前の effect から参照する）。
  const keyNavRef = useRef<{
    mails: MailSummary[];
    openedId: number | null;
    open: (id: number) => void;
    blocked: boolean;
  }>({ mails: [], openedId: null, open: () => {}, blocked: false });
  useEffect(() => {
    localStorage.setItem('rondine.mailSidebarW', String(sidebarW));
  }, [sidebarW]);
  const startResize = (e: React.MouseEvent) => {
    e.preventDefault();
    const onMove = (ev: MouseEvent) => {
      const rect = splitRef.current?.getBoundingClientRect();
      if (!rect) return;
      const w = Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, ev.clientX - rect.left));
      setSidebarW(w);
    };
    const onUp = () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      document.body.style.userSelect = '';
      document.body.style.cursor = '';
    };
    document.body.style.userSelect = 'none';
    document.body.style.cursor = 'col-resize';
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  };
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
  // タグ付与ポップオーバー: 位置＋対象メールID群（選択群 or 開いている1通）。
  const [tagPicker, setTagPicker] = useState<{ x: number; y: number; ids: number[] } | null>(null);
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

  // Ctrl+S（Mac は Cmd+S）でサイドバー（一覧ペイン）の表示を切替。ブラウザの保存は抑止。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && (e.key === 's' || e.key === 'S')) {
        e.preventDefault();
        setSidebarOpen((v) => !v);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // 矢印キーで前後のメールへ移動し、本文も切り替える（一覧は ↑↓、本文閲覧中は ←→）。
  // 端で止まる。最新の一覧/選択は keyNavRef 経由で参照する。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
      const nav = keyNavRef.current;
      if (nav.blocked) return;
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable))
        return;
      let delta = 0;
      if (e.key === 'ArrowDown' || e.key === 'ArrowRight') delta = 1;
      else if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') delta = -1;
      else return;
      const { mails, openedId, open } = nav;
      if (mails.length === 0) return;
      const cur = openedId != null ? mails.findIndex((m) => m.id === openedId) : -1;
      const next = cur === -1 ? (delta > 0 ? 0 : mails.length - 1) : cur + delta;
      if (next < 0 || next >= mails.length) {
        e.preventDefault(); // 端では移動しない（既定スクロールも抑止）
        return;
      }
      e.preventDefault();
      open(mails[next].id);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  useEffect(() => {
    if (selected == null && accounts.length > 0) setSelected(accounts[0].id);
  }, [accounts, selected]);

  // タグ一覧（チップ表示・絞り込み・付与候補の元データ）
  const reloadTags = () => tagList().then(setTags).catch(() => undefined);
  useEffect(() => {
    reloadTags();
  }, []);
  const tagById = new Map(tags.map((tg) => [tg.id, tg]));

  // 一覧の取得（無限スクロール）: 1 ページずつ読み込み、スクロールで続きを追加する。
  // 切替時の取り違えを防ぐため、呼び出しトークン／ページキーで整合を取る。
  const PAGE_SIZE = 100;
  const loadTokenRef = useRef(0);
  const pageKeyRef = useRef('');
  const loadingMoreRef = useRef(false);
  const [hasMore, setHasMore] = useState(false);
  // スクロール位置インジケータ（右端に、先頭付近のメールの年月を表示）。
  const [scrollHint, setScrollHint] = useState<{ ratio: number; label: string } | null>(null);
  const scrollHintTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const loadMails = () => {
    const token = ++loadTokenRef.current;
    pageKeyRef.current = `${selected}:${folder}`;
    loadingMoreRef.current = false;
    return mailList(queryAccount, folder, PAGE_SIZE, 0)
      .then((rows) => {
        if (loadTokenRef.current !== token) return;
        setMails(rows);
        setHasMore(rows.length >= PAGE_SIZE);
      })
      .catch(() => undefined);
  };

  // 続きを読み込んで末尾に追加。検索モード中・読み込み中・末尾到達時は何もしない。
  const loadMore = () => {
    if (selected == null || searchMode || loadingMoreRef.current || !hasMore) return;
    const key = pageKeyRef.current;
    loadingMoreRef.current = true;
    mailList(queryAccount, folder, PAGE_SIZE, mails.length)
      .then((rows) => {
        if (pageKeyRef.current !== key) return; // 切替後の結果は破棄
        setMails((prev) => [...prev, ...rows]);
        setHasMore(rows.length >= PAGE_SIZE);
      })
      .catch(() => undefined)
      .finally(() => {
        loadingMoreRef.current = false;
      });
  };

  // 一覧のスクロール: 続きの自動読み込み＋位置インジケータ（一番上に見えているメールの日付）。
  const onListScroll = (e: React.UIEvent<HTMLUListElement>) => {
    const el = e.currentTarget;
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 400) loadMore();
    const n = visibleMails.length;
    const rows = el.children;
    const count = Math.min(rows.length, n);
    if (count === 0) return;
    // 一番上に見えているメール行を実測（getBoundingClientRect）で二分探索。行高不定でも正確。
    const ulTop = el.getBoundingClientRect().top;
    let lo = 0;
    let hi = count - 1;
    let top = 0;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      // その行の下端がリスト上端より下 = まだ見えている → これ以降を探す。
      if ((rows[mid] as HTMLElement).getBoundingClientRect().bottom > ulTop + 1) {
        top = mid;
        hi = mid - 1;
      } else {
        lo = mid + 1;
      }
    }
    const label = formatScrollDate(visibleMails[top]?.date ?? null);
    // 縦位置は通常のスクロールバーのつまみと同じ（スクロール量÷スクロール可能量）。
    const max = el.scrollHeight - el.clientHeight;
    const ratio = max > 0 ? Math.min(1, Math.max(0, el.scrollTop / max)) : 0;
    if (label) {
      setScrollHint({ ratio, label });
      if (scrollHintTimer.current) clearTimeout(scrollHintTimer.current);
      scrollHintTimer.current = setTimeout(() => setScrollHint(null), 1200);
    }
  };
  useEffect(() => {
    setOpened(null);
    setSelectedIds(new Set());
    anchorId.current = null;
    if (selected != null) {
      loadMails().then(() => {
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

  // 自動同期の完了で一覧を再読み込み（手動同期中は onSync 側が再読込するのでスキップ）。
  useEffect(() => {
    const onSynced = () => {
      if (!syncing && selected != null) loadMails();
    };
    window.addEventListener(MAIL_SYNCED_EVENT, onSynced);
    return () => window.removeEventListener(MAIL_SYNCED_EVENT, onSynced);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [syncing, selected, folder]);

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
      mailSearch(queryAccount, folder, q, 200)
        .then(setSearchResults)
        .catch(() => setSearchResults([]))
        .finally(() => setSearching(false));
    }, 250);
    return () => clearTimeout(h);
  }, [query, selected, folder, queryAccount]);

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
      // 「全て」表示では全アカウントを順に同期する。
      const ids = selected === 'all' ? accounts.map((a) => a.id) : [selected];
      let fetched = 0;
      let stored = 0;
      for (const id of ids) {
        const r = await mailSync(id);
        fetched += r.fetched;
        stored += r.stored;
      }
      setStatus(t('mailbox.result', { fetched, stored }));
      await loadMails();
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

  // 新規タグを作成して対象メールに付与。
  const createAndAssign = async (name: string, ids: number[]) => {
    try {
      const created = await tagCreate(name, pickTagColor(tags.length));
      setTags((prev) => [...prev, created]);
      await applyTagDelta(ids, created.id, true);
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
          if (menu) setTagPicker({ x: menu.x, y: menu.y, ids: targetIds() });
        },
      },
      { key: 'spam', label: t('ctx.markSpam'), Icon: ThumbsDown, onClick: actMarkSpam },
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

  // 矢印キー移動（下の effect）が参照する最新値を ref に反映する。
  // effect 自体は早期 return より前に置く必要があるため、ここでは値の受け渡しだけ行う。
  keyNavRef.current = {
    mails: visibleMails,
    openedId: opened?.id ?? null,
    open: openMail,
    blocked: Boolean(menu || tagPicker || compose || sugOpen),
  };

  // ゴミ箱/迷惑メールを空にする（完全削除。確認のうえ実行）。
  const emptyCurrentFolder = async () => {
    if (folder !== 'trash' && folder !== 'spam') return;
    if (!window.confirm(t('mailbox.emptyConfirm', { folder: t(`mailbox.f_${folder}`) }))) return;
    try {
      await mailEmptyFolder(queryAccount, folder);
      setOpened(null);
      setSelectedIds(new Set());
      await loadMails();
    } catch {
      /* noop */
    }
  };

  const listPane = (
    <div
      className={`flex h-full min-h-0 flex-col ${
        folder === 'spam' ? 'bg-amber-600/15' : ''
      }`}
    >
      {/* アカウント選択＋フォルダ選択（アイコンボタン）を同じ行に置く */}
      <div className="flex shrink-0 items-center gap-2 border-b border-white/10 px-2 py-1.5">
        <select
          className="min-w-0 flex-1 rounded-md bg-white/10 px-2 py-1 text-xs outline-none"
          value={selected ?? ''}
          onChange={(e) => setSelected(e.target.value === 'all' ? 'all' : Number(e.target.value))}
        >
          {/* 全アカウント横断表示。既定は特定アカウントだが、複数ある時のみ「全て」を出す。 */}
          {accounts.length > 1 && (
            <option value="all" className="text-black">
              {t('mailbox.allAccounts')}
            </option>
          )}
          {accounts.map((a) => (
            <option key={a.id} value={a.id} className="text-black">
              {a.email}
            </option>
          ))}
        </select>
        <FolderIcons value={folder} onChange={setFolder} />
      </div>
      {/* 絞り込みツールバー: 一覧を絞る操作はリスト直上に置く（トグル/期間/タグ）。アイコンは中央寄せ */}
      <div className="flex shrink-0 flex-wrap items-center justify-center gap-1 border-b border-white/10 px-2 py-1">
        {MAIL_FILTERS.map(({ key, Icon }) => {
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
      {/* ゴミ箱/迷惑メール: フィルタ群の下に「空にする」（完全削除）ボタン */}
      {(folder === 'trash' || folder === 'spam') && (
        <div className="flex shrink-0 items-center justify-center border-b border-white/10 px-2 py-1">
          <button
            onClick={emptyCurrentFolder}
            disabled={mails.length === 0}
            className="flex items-center gap-1.5 rounded-md border border-red-400/40 px-3 py-1 text-xs text-red-200 hover:bg-red-500/25 hover:text-white disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Trash2 size={13} />
            {t('mailbox.emptyFolder', { folder: t(`mailbox.f_${folder}`) })}
          </button>
        </div>
      )}
      {/* 選択中のタグ: 次の行にチップで並べ、× で個別に解除。右端に全解除ボタン */}
      {tagFilter.size > 0 && (
        <div className="flex shrink-0 flex-wrap items-center gap-1 border-b border-white/10 px-2 py-1.5">
          {tags
            .filter((tg) => tagFilter.has(tg.id))
            .map((tg) => {
              const color = tg.color ?? DEFAULT_TAG_COLOR;
              return (
                <span
                  key={tg.id}
                  className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium"
                  style={{ backgroundColor: `${color}33`, color }}
                >
                  <span className="h-1.5 w-1.5 rounded-full" style={{ backgroundColor: color }} />
                  {tg.name}
                  <button
                    onClick={() => {
                      const next = new Set(tagFilter);
                      next.delete(tg.id);
                      setTagFilter(next);
                    }}
                    title={t('tag.removeFilter')}
                    aria-label={t('tag.removeFilter')}
                    className="-mr-0.5 flex h-3.5 w-3.5 items-center justify-center rounded-full hover:bg-white/20"
                  >
                    <X size={9} />
                  </button>
                </span>
              );
            })}
          <button
            onClick={() => setTagFilter(new Set())}
            className="ml-auto shrink-0 rounded px-1.5 py-0.5 text-[10px] text-white/50 hover:bg-white/10 hover:text-white/80"
          >
            {t('tag.clearFilter')}
          </button>
        </div>
      )}
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
      <div className="relative min-h-0 flex-1">
      {/* スクロール位置インジケータ: 右端にその辺りのメールの年月を表示 */}
      {scrollHint && (
        <div
          className="pointer-events-none absolute right-1.5 z-10 -translate-y-1/2 rounded-full bg-neutral-900/80 px-2 py-0.5 text-[10px] font-medium tabular-nums text-white/85 shadow backdrop-blur"
          style={{ top: `calc(${scrollHint.ratio} * (100% - 20px) + 10px)` }}
        >
          {scrollHint.label}
        </div>
      )}
      <ul
        className="h-full space-y-1 overflow-y-auto p-2"
        onScroll={onListScroll}
      >
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
              // チェックボックスは複数選択モード中（または選択済み）のみ表示。
              // ホバー/フォーカスでは出さない。選択は Ctrl/Shift＋クリックか右クリックから開始。
              className={`mt-1 h-3.5 w-3.5 shrink-0 accent-sky-400 ${
                selecting || selectedIds.has(m.id) ? '' : 'hidden'
              }`}
            />
            <div className="min-w-0 flex-1">
            {(() => {
              // 送信済/下書きは相手（To）、それ以外は差出人（From）を主に見せる。
              // 表示名はヘッダ名（from_name/to_name）を優先し、無ければアドレスから導出。
              const addr = parseAddress(outgoing ? m.to_addresses : m.from_address);
              const headerName = (outgoing ? m.to_name : m.from_name)?.trim();
              const name = headerName || addr.name || addr.email || (outgoing ? '—' : '(no sender)');
              const showEmail = addr.email && addr.email !== name;
              return (
                <>
                  {/* 名前＜メール＞＋送信日時（1行） */}
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="truncate text-sm font-medium">
                      {!m.is_read && <span className="mr-1 text-sky-300">●</span>}
                      {outgoing && <span className="text-white/40">{t('mailbox.to')}: </span>}
                      {name}
                      {showEmail && (
                        <span className="font-normal text-white/40">
                          {' '}
                          &lt;{addr.email}&gt;
                        </span>
                      )}
                    </span>
                    <span className="flex shrink-0 items-center gap-1 text-[10px] text-white/40">
                      {m.is_vip && (
                        <Gem
                          size={12}
                          className="fill-sky-300/30 text-sky-300"
                          aria-label={t('filter.vip')}
                        />
                      )}
                      {m.is_green && (
                        <LeafyGreen
                          size={12}
                          className="text-emerald-400"
                          aria-label={t('green.badge')}
                        />
                      )}
                      {m.is_starred && <Star size={12} className="fill-amber-300 text-amber-300" />}
                      {formatDate(m.date)}
                    </span>
                  </div>
                  {/* 件名 */}
                  <div className="truncate text-sm text-white/80">
                    {m.subject ?? '(no subject)'} {m.has_real_attachments && '📎'}
                  </div>
                  {/* 本文（詰めて2行折り返し） */}
                  <div className="line-clamp-2 text-xs leading-snug text-white/40">{m.preview}</div>
                </>
              );
            })()}
            </div>
          </li>
        ))
      )}
      {/* 続きあり: スクロールで自動読み込み（末尾のヒント） */}
      {!searchMode && hasMore && visibleMails.length > 0 && (
        <li className="px-2 py-3 text-center text-xs text-white/35">{t('mailbox.loadingMore')}</li>
      )}
      </ul>
      </div>
    </div>
    );

  // 開いているメールのタグ（詳細ヘッダに表示。MailDetail はタグを持たないため一覧側から解決）。
  const openedTags = opened
    ? ((mails.find((m) => m.id === opened.id) ?? searchResults.find((m) => m.id === opened.id))
        ?.tag_ids ?? [])
        .map((tid) => tagById.get(tid))
        .filter((tg): tg is TagSummary => tg != null)
    : [];

  // 開いているメールの現在のスター状態（MailDetail は持たないため一覧から解決）。
  const openedMail = opened
    ? (mails.find((m) => m.id === opened.id) ?? searchResults.find((m) => m.id === opened.id))
    : undefined;
  const openedStarred = openedMail?.is_starred ?? false;

  // 開いているメールのスターを切り替え（楽観更新 → 永続化）。
  const toggleStarOpened = async () => {
    if (!opened) return;
    const id = opened.id;
    const value = !openedStarred;
    patchMails(new Set([id]), { is_starred: value });
    try {
      await mailSetStarred([id], value);
    } catch {
      /* noop */
    }
  };

  // 開いているメールにタグを付与（ボタン位置にタグピッカーを開く）。
  const openTagForOpened = (x: number, y: number) => {
    if (opened) setTagPicker({ x, y, ids: [opened.id] });
  };

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
      starred={openedStarred}
      onToggleStar={toggleStarOpened}
      onTag={openTagForOpened}
      onRemoveTag={(tagId) => applyTagDelta([opened.id], tagId, false)}
      onReply={(mode) => setCompose({ mode, source: opened })}
      onMarkSpam={markSpamOpened}
      onAddContact={onAddContact}
      onEditContact={onOpenContact}
      onGreenChange={loadMails}
    />
  ) : (
    <div className="flex h-full items-center justify-center text-sm text-white/40">
      {t('mailbox.selectMail')}
    </div>
  );

  // メール作成はページとして表示（別ウィンドウにしない）。
  const composeEl = compose ? (
    <Compose
      accounts={accounts}
      // 返信/転送は元メールを受信したアカウントを既定に。新規は選択中（全ての時は先頭）。
      defaultAccountId={
        ('source' in compose ? compose.source.account_id : null) ??
        queryAccount ??
        accounts[0]?.id ??
        null
      }
      target={compose}
      onClose={() => setCompose(null)}
    />
  ) : null;

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-white/10 px-4 py-2">
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

        {/* 検索窓は左のまま、以降のボタン類を右へ寄せる */}
        <span className="flex-1" />
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
          className={`${iconBtn} ${sidebarOpen ? '' : 'bg-white/10'}`}
          onClick={() => setSidebarOpen((v) => !v)}
          title={sidebarOpen ? t('mailbox.hideSidebar') : t('mailbox.showSidebar')}
          aria-label={sidebarOpen ? t('mailbox.hideSidebar') : t('mailbox.showSidebar')}
          aria-pressed={!sidebarOpen}
        >
          {sidebarOpen ? <PanelLeftClose size={15} /> : <PanelLeftOpen size={15} />}
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

      {compose ? (
        // メール作成はページとして表示（サイドバーは閉じる）。返信/転送は
        // 左=下書き・右=元メールの2分割で、並べて作成できる。
        'source' in compose ? (
          <div
            className="grid min-h-0 flex-1 overflow-hidden"
            style={{ gridTemplateColumns: '1fr 1fr' }}
          >
            <div className="min-h-0 overflow-hidden">{composeEl}</div>
            <div className="min-h-0 overflow-hidden border-l border-white/10">
              <MailBody detail={compose.source} />
            </div>
          </div>
        ) : (
          <div className="min-h-0 flex-1 overflow-hidden">{composeEl}</div>
        )
      ) : layout === 'side' ? (
        <div
          ref={splitRef}
          className="grid min-h-0 flex-1 overflow-hidden"
          style={{ gridTemplateColumns: sidebarOpen ? `${sidebarW}px 6px 1fr` : '1fr' }}
        >
          {/* overflow-hidden は付けない: 絞り込みのポップオーバーをコンテンツ側へ重ねて表示するため */}
          {sidebarOpen && (
            <>
              <div className="min-h-0 border-r border-white/10">{listPane}</div>
              {/* ドラッグでサイドバー幅を変える（幅は上部の MIN/MAX 定数でクランプ） */}
              <div
                onMouseDown={startResize}
                title={t('mailbox.resize')}
                className="cursor-col-resize bg-transparent transition-colors hover:bg-sky-400/40"
              />
            </>
          )}
          <div className="min-h-0 overflow-hidden">{bodyPane}</div>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          {/* overflow-hidden は付けない: ポップオーバーを本文側へ重ねて表示するため */}
          {sidebarOpen && (
            <div className="h-1/3 min-h-0 border-b border-white/10">{listPane}</div>
          )}
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
          selectedMails={mails.filter((m) => tagPicker.ids.includes(m.id))}
          onToggle={(tagId, add) => applyTagDelta(tagPicker.ids, tagId, add)}
          onCreate={(name) => createAndAssign(name, tagPicker.ids)}
          onClose={() => setTagPicker(null)}
        />
      )}
    </div>
  );
}
