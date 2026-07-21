import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  CalendarPlus,
  Columns2,
  FlipHorizontal2,
  Gem,
  LeafyGreen,
  Mail,
  MailOpen,
  PanelLeftClose,
  PanelLeftOpen,
  RefreshCw,
  RotateCcw,
  Rows2,
  Search,
  SquarePen,
  Star,
  StarOff,
  Tag,
  ThumbsDown,
  ThumbsUp,
  Trash2,
  Undo2,
  UserRoundPlus,
  X,
} from 'lucide-react';
import { listen } from '@tauri-apps/api/event';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailSummary } from '@bindings/MailSummary';
import type { ThreadListItem } from '@bindings/ThreadListItem';
import type { MailDetail } from '@bindings/MailDetail';
import type { TagSummary } from '@bindings/TagSummary';
import type { SyncProgress } from '@bindings/SyncProgress';
import type { RecipientSuggestion } from '@bindings/RecipientSuggestion';
import {
  mailDelete,
  mailTrash,
  mailRestore,
  mailEmptyFolder,
  mailGet,
  mailGetDraft,
  mailMarkSpam,
  mailMarkNotSpam,
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
import { formatDateTime } from '../utils/datetime';
import { MailBody } from './MailBody';
import { ContactEditor, type EditorRequest } from './ContactEditor';
import { CalendarPanel, type CalendarPanelInitial } from './CalendarPanel';
import { Conversation, type ConversationHandlers } from './Conversation';
import { threadList, threadCount } from '../services/threads';
import { Compose, type ComposeTarget } from './Compose';
import { FolderIcons } from './FolderIcons';
import { MAIL_FILTERS, matchesFilters } from './mailFilters';
import { ContextMenu, type MenuItem } from './ContextMenu';
import { DateFilter, matchesDate, type DateRange } from './DateFilter';
import { SpamConflictAlert } from './SpamConflictAlert';
import { TagFilter, matchesTags } from './TagFilter';
import { TagPicker } from './TagPicker';

const iconBtn =
  'flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80 disabled:opacity-40';

// サイドバー（メール一覧）の幅。ドラッグで可変。ここを変えれば最小幅/初期幅を一括変更できる。
// 最小幅は絞り込みツールバーのアイコン（32px × 10 個 ＋ gap ＋ px-2 ≒ 372px）が
// 1 行に収まるサイズにする（折り返して見切れないように）。
export const MIN_SIDEBAR_WIDTH = 380;
export const MAX_SIDEBAR_WIDTH = 640;
export const DEFAULT_SIDEBAR_WIDTH = 380;

// 住所録の編集パネル（メールを見ながら編集する右サイドバー）の幅。ドラッグで可変。
const MIN_CONTACT_PANEL_WIDTH = 320;
const MAX_CONTACT_PANEL_WIDTH = 560;
const DEFAULT_CONTACT_PANEL_WIDTH = 400;

/** スクロール位置インジケータ用の日付ラベル（例: 2026/7/2）。 */
function formatScrollDate(d: string | null): string {
  if (!d) return '';
  const dt = new Date(d);
  return isNaN(dt.getTime()) ? '' : `${dt.getFullYear()}/${dt.getMonth() + 1}/${dt.getDate()}`;
}

/** 検索結果（メッセージ単位）を、一覧（スレッド単位）と同じ行の形へ写像する（1 通=1 スレッド相当）。 */
function searchRowToThread(m: MailSummary): ThreadListItem {
  return {
    thread_id: -m.id,
    id: m.id,
    account_id: m.account_id,
    subject: m.subject,
    from_address: m.from_address,
    from_name: m.from_name,
    to_addresses: m.to_addresses,
    to_name: m.to_name,
    date: m.date,
    preview: m.preview,
    is_read: m.is_read,
    has_real_attachments: m.has_real_attachments,
    is_starred: m.is_starred,
    is_bookmarked: m.is_bookmarked,
    tag_ids: m.tag_ids,
    is_known: m.is_known,
    is_vip: m.is_vip,
    is_green: m.is_green,
    message_count: m.message_count,
    unread_count: m.is_read ? 0 : 1,
    email_ids: [m.id],
  };
}

/**
 * メールモード: 全幅。リスト＋本文。レイアウトは左右/上下を切替可能。
 */
export function MailboxView({
  accounts,
  initialAccountId,
  initialMailId,
  onAccountChange,
  compose,
  setCompose,
  restoreDraftId,
  onComposeDraftChange,
}: {
  accounts: AccountSummary[];
  initialAccountId: number | null;
  initialMailId: number | null;
  /** 表示中アカウントの変化を親へ通知（フッターの件数表示用）。'all'=全アカウント。 */
  onAccountChange?: (id: number | 'all' | null) => void;
  /** 作成セッション（新規/返信/転送/下書き再編集）。App が保持し、ビュー切替で
   * メール画面がアンマウントされても消えないようにする（未編集の返信も戻せる）。 */
  compose: ComposeTarget | null;
  setCompose: (c: ComposeTarget | null) => void;
  /** 直前まで編集していた下書き id（別ビューから戻った時に本文込みで復元する）。 */
  restoreDraftId?: number | null;
  /** 編集中の下書き id の変化を親へ通知（未編集/破棄後は null）。 */
  onComposeDraftChange?: (id: number | null) => void;
}) {
  const { t } = useTranslation();
  // 'all' = 全アカウント横断表示 / number = 特定アカウント / null = 未選択。
  const [selected, setSelected] = useState<number | 'all' | null>(() => {
    // ホームからの遷移（特定アカウント指定）が最優先。
    if (initialAccountId != null) return initialAccountId;
    // 前回選択したアカウントを復元。'all' は複数アカウント時のみ有効。
    const saved = localStorage.getItem('rondine.mailAccount');
    if (saved === 'all' && accounts.length > 1) return 'all';
    const savedId = Number(saved);
    if (Number.isFinite(savedId) && savedId > 0 && accounts.some((a) => a.id === savedId))
      return savedId;
    // 既定は「全て」（複数アカウント時）。1 つだけならそのアカウント。
    return accounts.length > 1 ? 'all' : (accounts[0]?.id ?? null);
  });
  // クエリに渡すアカウント（number のみ。'all'/null は null=全アカウント）。
  const queryAccount = typeof selected === 'number' ? selected : null;
  // 表示中アカウントを親へ通知（フッターのメール総数表示）。
  useEffect(() => {
    onAccountChange?.(selected);
  }, [selected, onAccountChange]);
  // 選択アカウントを保存（次回起動時に復元）。未選択(null)は保存しない。
  useEffect(() => {
    if (selected == null) return;
    localStorage.setItem('rondine.mailAccount', String(selected));
  }, [selected]);
  // アカウント一覧の読込後/変更後、選択が無効（未選択・削除済み・単一で 'all'）なら既定へ寄せる。
  useEffect(() => {
    if (accounts.length === 0) return;
    const isValid =
      (selected === 'all' && accounts.length > 1) ||
      (typeof selected === 'number' && accounts.some((a) => a.id === selected));
    if (!isValid) setSelected(accounts.length > 1 ? 'all' : accounts[0].id);
  }, [accounts, selected]);
  // 遷移直後に開くべきメッセージ（ホームの新着クリック）
  const pendingOpen = useRef<number | null>(initialMailId);
  // 一覧はスレッド単位（代表＋件数）。検索結果も同じ形へ写像して扱いを揃える。
  const [mails, setMails] = useState<ThreadListItem[]>([]);
  // 現在の読み込み件数を常に最新で保持（同期イベントの購読は再購読しないため、
  // クロージャの古い mails を避けて ref で参照する）。
  const mailsLenRef = useRef(0);
  mailsLenRef.current = mails.length;
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
  // スクロールする一覧本体（<ul>）。自動追い読み込みで高さ実測に使う。
  const listRef = useRef<HTMLUListElement>(null);
  // 矢印キー移動用の「最新値」ref（早期 return より前の effect から参照する）。
  const keyNavRef = useRef<{
    mails: ThreadListItem[];
    openedId: number | null;
    open: (id: number) => void;
    blocked: boolean;
  }>({ mails: [], openedId: null, open: () => {}, blocked: false });
  // Del / Ctrl+D 削除キー（下の effect）が参照する最新の削除処理と抑止状態。
  const delKeyRef = useRef<{ del: () => void; blocked: boolean }>({
    del: () => {},
    blocked: false,
  });
  // Ctrl+Z 取消キー（下の effect）が参照する最新の復元処理と抑止状態。
  const undoKeyRef = useRef<{ canUndo: boolean; undo: () => void; blocked: boolean }>({
    canUndo: false,
    undo: () => {},
    blocked: false,
  });
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

  // 住所録の編集パネル（メールを見ながら編集する右サイドバー）。null なら閉じている。
  // アドレスの ＋（追加）／編集アイコンから開く。閲覧ペインは残したまま右に並べて出す。
  const [contactPanel, setContactPanel] = useState<EditorRequest | null>(null);
  const [contactPanelW, setContactPanelW] = useState(() => {
    const saved = Number(localStorage.getItem('rondine.contactPanelW'));
    return Number.isFinite(saved) && saved >= MIN_CONTACT_PANEL_WIDTH
      ? Math.min(MAX_CONTACT_PANEL_WIDTH, saved)
      : DEFAULT_CONTACT_PANEL_WIDTH;
  });
  useEffect(() => {
    localStorage.setItem('rondine.contactPanelW', String(contactPanelW));
  }, [contactPanelW]);
  const panelSplitRef = useRef<HTMLDivElement>(null);
  // パネルを開く前のサイドバー表示状態（閉じたら元に戻すため）。
  const sidebarBeforePanelRef = useRef<boolean | null>(null);
  // カレンダー入力パネル（メールを見ながら予定を入れる右サイドバー）。null なら閉じている。
  // 本文の日付の ＋、またはヘッダの「カレンダーに追加」から開く。住所録パネルと同じ枠を使う。
  const [calendarPanel, setCalendarPanel] = useState<CalendarPanelInitial | null>(null);
  const openContactPanel = (req: EditorRequest) => {
    if (sidebarBeforePanelRef.current == null) sidebarBeforePanelRef.current = sidebarOpen;
    setSidebarOpen(false); // 一覧サイドバーを隠して、閲覧＋編集の2画面にする。
    setCalendarPanel(null); // 住所録とカレンダーの右パネルは相互排他。
    setContactPanel(req);
  };
  const closeContactPanel = () => {
    setContactPanel(null);
    if (sidebarBeforePanelRef.current != null) {
      setSidebarOpen(sidebarBeforePanelRef.current);
      sidebarBeforePanelRef.current = null;
    }
  };
  const openCalendarPanel = (init: CalendarPanelInitial) => {
    if (sidebarBeforePanelRef.current == null) sidebarBeforePanelRef.current = sidebarOpen;
    setSidebarOpen(false);
    setContactPanel(null); // 相互排他。
    setCalendarPanel(init);
  };
  const closeCalendarPanel = () => {
    setCalendarPanel(null);
    if (sidebarBeforePanelRef.current != null) {
      setSidebarOpen(sidebarBeforePanelRef.current);
      sidebarBeforePanelRef.current = null;
    }
  };
  const startPanelResize = (e: React.MouseEvent) => {
    e.preventDefault();
    const onMove = (ev: MouseEvent) => {
      const rect = panelSplitRef.current?.getBoundingClientRect();
      if (!rect) return;
      const w = Math.min(
        MAX_CONTACT_PANEL_WIDTH,
        Math.max(MIN_CONTACT_PANEL_WIDTH, rect.right - ev.clientX),
      );
      setContactPanelW(w);
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

  // 作成セッション（compose）は App 側で保持する（上の props）。メール画面はビュー切替で
  // アンマウントされるが、compose は App にあるため未編集の返信/新規もそのまま残り、戻ると再表示される。
  // 作成画面に入ったら編集/カレンダーパネルは閉じる（全幅で作成に集中）。
  useEffect(() => {
    if (compose) {
      closeContactPanel();
      closeCalendarPanel();
    }
    // close* は毎回同じ挙動。compose の変化だけをトリガにする。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [compose]);
  // 別ビュー（カレンダー等）から戻った時（＝このマウント時）、書きかけて自動保存された下書きが
  // あれば本文込みで開き直す。未編集の返信/新規は compose が保持されているのでそのまま表示され、
  // 編集済みは保持していたセッションを本文付きの下書きへ差し替える。マウント時に一度だけ実行し、
  // 編集中に自動保存で id が付いても（同一マウント中は再実行しない）ライブ編集を巻き戻さない。
  const restoredRef = useRef(false);
  useEffect(() => {
    if (restoredRef.current) return;
    restoredRef.current = true;
    if (restoreDraftId == null) return; // 書きかけの保存済み下書きは無い（未編集は compose 保持で足りる）
    // 既に同じ下書きを本文込みで表示中なら再取得しない（下書き再編集セッションの保持時）。
    if (compose?.mode === 'draft' && compose.draft.id === restoreDraftId) return;
    void openDraft(restoreDraftId);
    // マウント時に一度だけ。openDraft/compose/restoreDraftId は初期値で十分。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  // 表示するフォルダ/グループ（受信箱以外は後続実装）
  const [folder, setFolder] = useState('inbox');
  // リスト絞り込みトグル
  const [filters, setFilters] = useState<Set<string>>(new Set());
  // トグル絞り込みの反転（選択条件に「一致しない」ものを表示）。不要メールの一括選択に使う。
  const [filterInvert, setFilterInvert] = useState(false);
  // 期間フィルタ（以降/以前/期間）
  const [dateFilter, setDateFilter] = useState<DateRange | null>(null);
  // タグ（一覧データ・絞り込み条件・付与ポップオーバー位置）
  const [tags, setTags] = useState<TagSummary[]>([]);
  const [tagFilter, setTagFilter] = useState<Set<number>>(new Set());
  // タグ付与ポップオーバー: 位置＋対象メールID群（選択群 or 開いている1通）。
  const [tagPicker, setTagPicker] = useState<{ x: number; y: number; ids: number[] } | null>(null);
  // 全文検索（件名・差出人・本文）。query が空でなければ検索モード。
  const [query, setQuery] = useState('');
  const [searchResults, setSearchResults] = useState<ThreadListItem[]>([]);
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
  // 「フォルダ内の一致する全件」を選択したモード（表示範囲＝読み込み済みだけでなく DB 全体が対象）。
  // matchCount は一致総数、allMatchRowsRef はその全一致行（email_ids 展開・アクション適用に使う）。
  const [allMatching, setAllMatching] = useState(false);
  const [matchCount, setMatchCount] = useState(0);
  const [selectingAll, setSelectingAll] = useState(false);
  const allMatchRowsRef = useRef<ThreadListItem[]>([]);
  // 直前のゴミ箱移動の取消情報（Ctrl+Z／トーストで復元）。ids は移動したメール id。
  const [undoTrash, setUndoTrash] = useState<{ ids: number[]; count: number } | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; rowId: number } | null>(null);
  // 選択モード（チェックボックス表示中）。1件でも明示選択したら on。
  const [selecting, setSelecting] = useState(false);
  // 選択を完全に解除（手動選択・全一致モードともにクリア）。
  const resetSelection = () => {
    setSelectedIds(new Set());
    setAllMatching(false);
    setMatchCount(0);
    allMatchRowsRef.current = [];
    anchorId.current = null;
  };
  // 選択が空になったら選択モードを抜け、全一致モードも解除する。
  useEffect(() => {
    if (selectedIds.size === 0) {
      setSelecting(false);
      setAllMatching(false);
      setMatchCount(0);
      allMatchRowsRef.current = [];
    }
  }, [selectedIds]);
  // 絞り込み/検索の変更で「全一致選択」は対象集合が変わるため無効化する（手動選択は維持）。
  // フォルダ/アカウント切替は下の読み込み effect が resetSelection で処理する。
  useEffect(() => {
    if (allMatching) resetSelection();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filters, filterInvert, dateFilter, tagFilter, searchMode]);

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

  // Del / Ctrl+D（Mac は Cmd+D）で、選択中（未選択なら閲覧中）のメールを削除する。
  // 重なり UI（メニュー/タグピッカー/作成モーダル/候補）や入力欄フォーカス中は対象外。
  // 最新の削除処理・抑止状態は delKeyRef 経由で参照する。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      // 押しっぱなしのオートリピートは無視する（1回押下＝1件削除）。削除後に次のメールを
      // 自動で開くため、リピートを通すと連続削除で「次のメールが飛ばされる」ように見える。
      if (e.repeat) return;
      const isDel = e.key === 'Delete';
      const isCtrlD =
        (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && (e.key === 'd' || e.key === 'D');
      if (!isDel && !isCtrlD) return;
      const d = delKeyRef.current;
      if (d.blocked) return;
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable)) return;
      e.preventDefault();
      d.del();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // Ctrl+Z（Mac は Cmd+Z）で、直前のゴミ箱移動を元に戻す（復元）。
  // 取消対象が無い／重なり UI／入力欄フォーカス中は対象外。最新値は undoKeyRef 経由で参照。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      if (
        !((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && (e.key === 'z' || e.key === 'Z'))
      )
        return;
      const u = undoKeyRef.current;
      if (!u.canUndo || u.blocked) return;
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable)) return;
      e.preventDefault();
      u.undo();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // 取消トーストは一定時間で自動的に消す（その後は Ctrl+Z も無効）。
  useEffect(() => {
    if (!undoTrash) return;
    const tmr = setTimeout(() => setUndoTrash(null), 6000);
    return () => clearTimeout(tmr);
  }, [undoTrash]);

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
  const reloadTags = () =>
    tagList()
      .then(setTags)
      .catch(() => undefined);
  useEffect(() => {
    reloadTags();
  }, []);
  const tagById = new Map(tags.map((tg) => [tg.id, tg]));

  // 一覧の取得（無限スクロール）: 1 ページずつ読み込み、スクロールで続きを追加する。
  // 切替時の取り違えを防ぐため、呼び出しトークン／ページキーで整合を取る。
  const PAGE_SIZE = 100;
  // フィルタで可視行が少ないとき、ビューポートが埋まるまで自動で読み足す上限（一気読みの安全弁）。
  const AUTO_FILL_CAP = 1500;
  const loadTokenRef = useRef(0);
  const pageKeyRef = useRef('');
  const loadingMoreRef = useRef(false);
  const [hasMore, setHasMore] = useState(false);
  // フォルダ内のスレッド総数（一覧の「表示 X / 全 Y」表示用）。
  const [threadTotal, setThreadTotal] = useState<number | null>(null);
  // スクロール位置インジケータ（右端に、先頭付近のメールの年月を表示）。
  const [scrollHint, setScrollHint] = useState<{ ratio: number; label: string } | null>(null);
  const scrollHintTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // keepScroll: 同期後の再読込などで、すでに読み込んだページ数（＝スクロール位置）を
  // 保つため、先頭ページに戻さず現在の読み込み件数ぶんをまとめて引き直す。既定は
  // 先頭 1 ページ（フォルダ/アカウント切替時は先頭から表示したいため）。
  const loadMails = (opts?: { keepScroll?: boolean }) => {
    const token = ++loadTokenRef.current;
    pageKeyRef.current = `${selected}:${folder}`;
    loadingMoreRef.current = false;
    const count = opts?.keepScroll ? Math.max(PAGE_SIZE, mailsLenRef.current) : PAGE_SIZE;
    // 総数（スレッド件数）も取得して「表示 X / 全 Y」に使う。
    threadCount(queryAccount, folder)
      .then((n) => {
        if (loadTokenRef.current === token) setThreadTotal(n);
      })
      .catch(() => setThreadTotal(null));
    return threadList(queryAccount, folder, count, 0)
      .then((rows) => {
        if (loadTokenRef.current !== token) return;
        setMails(rows);
        setHasMore(rows.length >= count);
      })
      .catch(() => undefined);
  };

  // 続きを読み込んで末尾に追加。検索モード中・読み込み中・末尾到達時は何もしない。
  const loadMore = () => {
    if (selected == null || searchMode || loadingMoreRef.current || !hasMore) return;
    const key = pageKeyRef.current;
    loadingMoreRef.current = true;
    threadList(queryAccount, folder, PAGE_SIZE, mails.length)
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

  // 自動追い読み込み: フィルタ絞り込みは前段でクライアント側適用のため、読み込み済みページ内の
  // 一致が数件だと一覧がスクロール可能にならない。すると続きを読む「きっかけ」（スクロール）が
  // 起きず、少数の一致だけ表示して止まって見える（ウィンドウを小さくすると溢れてスクロールでき、
  // 続きが読める、という回避挙動になっていた）。ここではビューポートが埋まる＝スクロール可能に
  // なるまで、上限（AUTO_FILL_CAP 件）まで次ページを自動で読み足す。内容量・ウィンドウ/サイドバー
  // 幅の変化のたびに scrollHeight を実測して再判定する。上限超過後は通常のスクロールで続きを読む。
  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const fill = () => {
      if (searchMode || loadingMoreRef.current || !hasMore) return;
      if (mails.length >= AUTO_FILL_CAP) return; // 一気読みの安全弁（以降は手動スクロール）
      if (el.clientHeight <= 0) return; // 未レイアウト時は判定しない
      if (el.scrollHeight <= el.clientHeight + 1) loadMore(); // まだスクロールできない＝続きを読む
    };
    fill();
    const ro = new ResizeObserver(fill);
    ro.observe(el);
    return () => ro.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mails, searchResults, hasMore, searchMode, filters, filterInvert, dateFilter, tagFilter, folder, selected]);

  useEffect(() => {
    setOpened(null);
    resetSelection();
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
  const updateLists = (fn: (list: ThreadListItem[]) => ThreadListItem[]) => {
    setMails(fn);
    setSearchResults(fn);
  };

  // 「アクティブ（開いている未読スレッド）」は、そこから離れるまで未読のまま保つ。別スレッドを
  // 開く/閉じる/画面を離れるときに flushActiveRead() で既読化する（読書中は未読フィルタから
  // 消えない）。手動で既読/未読を切り替えたらこの予約は取り消す（下の onSetRead / actRead）。
  const activeReadRef = useRef<{ rowId: number; ids: number[] } | null>(null);
  const flushActiveRead = useCallback(() => {
    const t = activeReadRef.current;
    if (!t) return;
    activeReadRef.current = null;
    const mark = (prev: ThreadListItem[]) =>
      prev.map((m) => (m.id === t.rowId ? { ...m, is_read: true, unread_count: 0 } : m));
    setMails(mark);
    setSearchResults(mark);
    mailSetRead(t.ids, true).catch(() => undefined);
  }, []);
  // スレッドを閉じた（opened=null）ら既読化。別スレッドへの切替は openMail 側で処理する。
  useEffect(() => {
    if (opened == null) flushActiveRead();
  }, [opened, flushActiveRead]);
  // 画面を離れる（アンマウント）ときも、開いていた未読スレッドを既読化する。
  useEffect(() => () => flushActiveRead(), [flushActiveRead]);

  // 自動同期の完了で一覧を再読み込み（手動同期中は onSync 側が再読込するのでスキップ）。
  useEffect(() => {
    const onSynced = (e: Event) => {
      if (syncing || selected == null) return;
      // 新着が保存された時だけ一覧を更新（読み込み済みの位置は保持）。新着ゼロなら
      // 再取得しない＝スクロール中に先頭ページ（最新＝2026）へ巻き戻る「ループ」を防ぐ。
      const stored = (e as CustomEvent<{ stored?: number }>).detail?.stored ?? 0;
      if (stored > 0) loadMails({ keepScroll: true });
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
      // 検索はメッセージ単位。行の扱いを一覧（スレッド）と揃えるため 1 通=1 スレッド相当へ写像。
      mailSearch(queryAccount, folder, q, 200)
        .then((rows) => setSearchResults(rows.map(searchRowToThread)))
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
      // 手動同期後も読み込み済みの位置を保つ（先頭ページへ巻き戻さない）。
      await loadMails({ keepScroll: true });
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
      // 別スレッドへ移るときだけ、前にアクティブだった未読スレッドを既読化する。
      // 開いたスレッドはアクティブな間は未読のまま（会話を見ている＝まだ読書中）。抜けたとき
      // （別を開く／閉じる／画面を離れる）に既読化するよう予約する。
      if (activeReadRef.current?.rowId !== id) {
        flushActiveRead();
        const row = mails.find((m) => m.id === id) ?? searchResults.find((m) => m.id === id);
        const ids = row && row.email_ids.length ? row.email_ids : [id];
        activeReadRef.current = row && !row.is_read ? { rowId: id, ids } : null;
      }
      setOpened(d);
    } catch {
      /* noop */
    }
  };

  // 下書きフォルダのクリックは、閲覧ではなく作成画面で再編集を開く。
  // 返信の下書き（source_id あり）は元メールも読み込み、返信時と同じく右ペインに並べる。
  const openDraft = async (id: number) => {
    try {
      const draft = await mailGetDraft(id);
      const source =
        draft.source_id != null ? await mailGet(draft.source_id).catch(() => null) : null;
      setCompose({ mode: 'draft', draft, source: source ?? undefined });
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
    if (folder === 'drafts') openDraft(id);
    else openMail(id);
  };

  const onRowContextMenu = (e: React.MouseEvent, id: number) => {
    e.preventDefault();
    if (!selectedIds.has(id)) {
      setSelectedIds(new Set([id]));
      anchorId.current = id;
    }
    setMenu({ x: e.clientX, y: e.clientY, rowId: id });
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

  const patchMails = (ids: Set<number>, patch: Partial<ThreadListItem>) =>
    updateLists((prev) => prev.map((m) => (ids.has(m.id) ? { ...m, ...patch } : m)));

  const targetIds = () => [...selectedIds];

  // 選択した行（＝スレッド代表）を、そのフォルダ内のスレッド全メール id へ展開する。
  // 既読/削除/迷惑はスレッド全体に効かせる（スター/タグは代表に付ける）。
  const emailIdsFor = (rowIds: number[]): number[] => {
    const want = new Set(rowIds);
    // 読み込み済み一覧・検索結果・全一致行から、対象行を id 重複なく集める
    // （全一致行は読み込み済みと重なるため、Map で 1 行につき 1 回だけ拾う）。
    const byId = new Map<number, ThreadListItem>();
    for (const m of mails) if (want.has(m.id)) byId.set(m.id, m);
    for (const m of searchResults) if (want.has(m.id) && !byId.has(m.id)) byId.set(m.id, m);
    for (const m of allMatchRowsRef.current)
      if (want.has(m.id) && !byId.has(m.id)) byId.set(m.id, m);
    const seen = new Set<number>();
    const out: number[] = [];
    for (const m of byId.values()) {
      for (const id of m.email_ids.length ? m.email_ids : [m.id]) {
        if (!seen.has(id)) {
          seen.add(id);
          out.push(id);
        }
      }
    }
    return out.length ? out : rowIds;
  };

  const actRead = async (read: boolean) => {
    const wasAll = allMatching;
    const ids = emailIdsFor(targetIds()); // reset より前に対象 id を確定させる
    // 手動の既読/未読切替がアクティブ予約と重なるなら、離脱時の自動既読を取り消す（手動を優先）。
    if (activeReadRef.current && (wasAll || selectedIds.has(activeReadRef.current.rowId))) {
      activeReadRef.current = null;
    }
    patchMails(selectedIds, { is_read: read, unread_count: read ? 0 : 1 });
    if (wasAll) resetSelection();
    try {
      await mailSetRead(ids, read);
    } catch {
      /* noop */
    }
    // DB 全体へ適用したときは、表示中の窓を DB の実状態に合わせて取り直す。
    if (wasAll) await loadMails({ keepScroll: true });
  };
  const actStar = async (value: boolean) => {
    // スターはスレッド全体に効かせる（表示も会話単位で集約）。代表メールだけに付けると
    // 再構築で代表が入れ替わったとき印が消えて見えるため、フォルダ内の全メールへ適用する。
    const wasAll = allMatching;
    const ids = emailIdsFor(targetIds());
    patchMails(selectedIds, { is_starred: value });
    if (wasAll) resetSelection();
    try {
      await mailSetStarred(ids, value);
    } catch {
      /* noop */
    }
    if (wasAll) await loadMails({ keepScroll: true });
  };
  // 楽観更新で一覧から外し、閲覧中なら閉じる（削除／ゴミ箱移動／復元の共通処理）。
  const dropRows = (rowIds: number[], emailIds: number[]) => {
    const idSet = new Set(rowIds);
    updateLists((prev) => prev.filter((m) => !idSet.has(m.id)));
    if (opened && emailIds.includes(opened.id)) setOpened(null);
    setSelectedIds(new Set());
  };
  // 削除する行の直後（無ければ直前）の生存行 id を、削除前の並び順から求める。
  // 閲覧中のメールを消したときに「次のメール」を自動で開くために使う。
  const nextRowAfterDelete = (rowIds: number[]): number | null => {
    const order = visibleMails.map((m) => m.id);
    const del = new Set(rowIds);
    let last = -1;
    for (let i = 0; i < order.length; i++) if (del.has(order[i])) last = i;
    if (last === -1) return null;
    for (let i = last + 1; i < order.length; i++) if (!del.has(order[i])) return order[i];
    for (let i = last - 1; i >= 0; i--) if (!del.has(order[i])) return order[i];
    return null;
  };
  // 指定した行（スレッド代表）を削除する。既定はゴミ箱へ移動（復元可）。
  // ゴミ箱フォルダ内では完全削除（確認あり・復元不可）。
  const deleteRows = async (rowIds: number[]) => {
    if (rowIds.length === 0) return;
    const ids = emailIdsFor(rowIds);
    // 閲覧中のメールを消す場合だけ、次のメールへ自動で送る（クリックし直し不要にする）。
    const advanceTo = opened && rowIds.includes(opened.id) ? nextRowAfterDelete(rowIds) : null;
    const advance = () => {
      if (advanceTo == null) return;
      anchorId.current = advanceTo;
      if (folder === 'drafts') void openDraft(advanceTo);
      else void openMail(advanceTo);
    };
    if (folder === 'trash') {
      if (!window.confirm(t('mailbox.deletePermanentConfirm', { count: rowIds.length }))) return;
      dropRows(rowIds, ids);
      advance();
      try {
        await mailDelete(ids);
      } catch {
        /* noop */
      }
      return;
    }
    dropRows(rowIds, ids);
    advance();
    setUndoTrash({ ids, count: rowIds.length }); // Ctrl+Z／トーストで復元できるようにする
    try {
      await mailTrash(ids);
    } catch {
      /* noop */
    }
  };
  const actDelete = () => deleteRows(targetIds());
  // 一覧の行ホバー用: 選択状態を変えずに、その行（スレッド）単体へ効かせる。
  const rowEmailIds = (m: ThreadListItem) => (m.email_ids.length ? m.email_ids : [m.id]);
  // スターはスレッド全体に効かせる（代表が入れ替わっても印が消えて見えないように）。
  const rowToggleStar = async (m: ThreadListItem) => {
    const value = !m.is_starred;
    patchMails(new Set([m.id]), { is_starred: value });
    try {
      await mailSetStarred(rowEmailIds(m), value);
    } catch {
      /* noop */
    }
  };
  // 迷惑としてマーク / 非迷惑に戻す（1 行分）。楽観的に一覧から外し、サーバー反映後に取り直す。
  const rowSpam = async (m: ThreadListItem, spam: boolean) => {
    const ids = rowEmailIds(m);
    updateLists((prev) => prev.filter((x) => x.id !== m.id));
    if (opened && ids.includes(opened.id)) setOpened(null);
    try {
      await (spam ? mailMarkSpam(ids) : mailMarkNotSpam(ids));
    } catch {
      /* noop */
    }
    await loadMails();
  };
  // ゴミ箱の選択メールを元のフォルダへ復元する（メニュー「復元」／復元ボタン）。
  const actRestore = async () => {
    const rowIds = targetIds();
    if (rowIds.length === 0) return;
    const ids = emailIdsFor(rowIds);
    dropRows(rowIds, ids);
    try {
      await mailRestore(ids);
    } catch {
      /* noop */
    }
  };
  // 直前のゴミ箱移動を取り消す（Ctrl+Z／トースト）。復元後は一覧を読み直して元の位置へ戻す。
  const undoLastTrash = async () => {
    if (!undoTrash) return;
    const ids = undoTrash.ids;
    setUndoTrash(null);
    try {
      await mailRestore(ids);
    } catch {
      /* noop */
    }
    await loadMails();
  };
  // 迷惑としてマーク: 学習＋隔離。楽観更新で受信一覧から外す（迷惑フォルダへ。スレッド全体）。
  const actMarkSpam = async () => {
    const idSet = new Set(targetIds());
    const ids = emailIdsFor(targetIds());
    updateLists((prev) => prev.filter((m) => !idSet.has(m.id)));
    if (opened && ids.includes(opened.id)) setOpened(null);
    setSelectedIds(new Set());
    try {
      await mailMarkSpam(ids);
    } catch {
      /* noop */
    }
    // 同じ差出人の他メールもサーバー側で迷惑へ移るため、一覧を取り直して反映する。
    await loadMails();
  };
  // 非迷惑に戻す: 隔離解除＋ham 学習。迷惑フォルダ表示から外し受信箱へ戻す（楽観更新）。
  const actMarkNotSpam = async () => {
    const idSet = new Set(targetIds());
    const ids = emailIdsFor(targetIds());
    updateLists((prev) => prev.filter((m) => !idSet.has(m.id)));
    if (opened && ids.includes(opened.id)) setOpened(null);
    setSelectedIds(new Set());
    try {
      await mailMarkNotSpam(ids);
    } catch {
      /* noop */
    }
    // 迷惑差出人の解除で同アドレスのメールが受信箱へ戻るため、一覧を取り直して反映する。
    await loadMails();
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

  // 差出人（送信済/下書きは宛先）のアドレスで検索して、その相手のメールだけを絞り込む。
  // 既存の全文検索（FTS: 件名/差出人/本文）を流用し、検索窓にアドレスを入れるだけ。
  const searchBySender = (email: string) => {
    if (!email) return;
    sugPicked.current = true; // 候補ドロップダウンは出さない
    setSelectedIds(new Set());
    setSugOpen(false);
    setQuery(email);
  };

  // 選択集合の状態に応じてメニュー項目（トグルラベル）を組み立てる
  const buildMenuItems = (): MenuItem[] => {
    const sel = mails.filter((m) => selectedIds.has(m.id));
    const allStarred = sel.length > 0 && sel.every((m) => m.is_starred);
    const inTrash = folder === 'trash';
    // 右クリックした 1 通の相手（差出人／送信箱は宛先）のアドレス。空なら検索項目を出さない。
    const row = menu ? findMail(menu.rowId) : undefined;
    const corrEmail = row
      ? parseAddress(outgoing ? row.to_addresses : row.from_address).email
      : '';
    return [
      ...(corrEmail
        ? [
            {
              key: 'searchSender',
              label: outgoing ? t('ctx.searchRecipient') : t('ctx.searchSender'),
              Icon: Search,
              onClick: () => searchBySender(corrEmail),
            } as MenuItem,
          ]
        : []),
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
      // ゴミ箱内は「復元」、迷惑フォルダは「非迷惑に戻す」、それ以外は「迷惑」。
      inTrash
        ? { key: 'restore', label: t('ctx.restore'), Icon: RotateCcw, onClick: actRestore }
        : folder === 'spam'
          ? { key: 'notSpam', label: t('ctx.notSpam'), Icon: ThumbsUp, onClick: actMarkNotSpam }
          : { key: 'spam', label: t('ctx.markSpam'), Icon: ThumbsDown, onClick: actMarkSpam },
      {
        key: 'delete',
        // ゴミ箱内は完全削除（復元不可）、それ以外はゴミ箱へ移動。
        label: inTrash ? t('ctx.deletePermanent') : t('ctx.delete'),
        Icon: Trash2,
        danger: true,
        onClick: actDelete,
      },
    ];
  };

  if (accounts.length === 0) {
    return <div className="p-8 text-white/60">{t('mailbox.addInSettings')}</div>;
  }

  // 一覧の絞り込み述語（トグル/期間/タグ）。表示用と「全一致選択」で同じ条件を使う。
  const passesFilters = (m: ThreadListItem) => {
    // トグル絞り込み。反転（invert）時は「一致しない」ものを通す（条件が無ければ反転は無効）。
    const toggleBase = matchesFilters(m, filters);
    const togglePass = filterInvert && filters.size > 0 ? !toggleBase : toggleBase;
    return togglePass && matchesDate(m.date, dateFilter) && matchesTags(m.tag_ids, tagFilter);
  };
  // 検索モードでは FTS 結果を、通常は読み込み済み一覧を対象に、絞り込みを重ねて表示する。
  const visibleMails = (searchMode ? searchResults : mails).filter(passesFilters);

  // 選択モード中はチェックボックスを表示して選択を簡単にする。
  const allVisibleSelected =
    visibleMails.length > 0 && visibleMails.every((m) => selectedIds.has(m.id));
  const someVisibleSelected = selectedIds.size > 0 && !allVisibleSelected;
  // ヘッダのチェックボックス: 1 クリックで「フォルダ内の一致する全件」を選択/解除する
  // （表示範囲だけでなく DB 全体が対象）。すべて読み込み済み（続きなし）or 検索モードなら
  // 表示中＝全件なので取得不要で即選択、まだ DB に続きがあるときは全代表を取得して一致する
  // 全件を選択する。
  const toggleAllVisible = () => {
    if (allVisibleSelected) {
      resetSelection();
      return;
    }
    if (searchMode || !hasMore) {
      setSelecting(true);
      setSelectedIds(new Set(visibleMails.map((m) => m.id)));
      return;
    }
    void selectAllMatching();
  };

  // 「フォルダ内の一致する全件」を選択する（表示範囲＝読み込み済みではなく DB 全体が対象）。
  // 現在のアカウント/フォルダの全スレッド代表を取得し、表示中と同じ絞り込みを適用して一致行を
  // すべて選択する。email_ids も保持し、既読/削除/迷惑/タグをスレッド全体へ適用できるようにする。
  const selectAllMatching = async () => {
    if (searchMode || selected == null) return;
    setSelectingAll(true);
    try {
      // limit は実質無制限（全代表を一括取得）。offset 0 から引く。
      const all = await threadList(queryAccount, folder, 1_000_000, 0);
      const matched = all.filter(passesFilters);
      allMatchRowsRef.current = matched;
      setSelectedIds(new Set(matched.map((m) => m.id)));
      setMatchCount(matched.length);
      setAllMatching(true);
      setSelecting(true);
    } catch {
      /* 取得失敗時は何もしない（表示範囲の選択は維持） */
    } finally {
      setSelectingAll(false);
    }
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
  // Del / Ctrl+D 用: 選択中があればそれを、なければ閲覧中のメールを削除対象にする。
  delKeyRef.current = {
    del: () => {
      const rowIds = selectedIds.size > 0 ? [...selectedIds] : opened ? [opened.id] : [];
      void deleteRows(rowIds);
    },
    blocked: Boolean(menu || tagPicker || compose || sugOpen),
  };
  // Ctrl+Z 用: 直前のゴミ箱移動があれば復元する。
  undoKeyRef.current = {
    canUndo: undoTrash != null,
    undo: () => void undoLastTrash(),
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
      className={`relative flex h-full min-h-0 flex-col ${folder === 'spam' ? 'bg-amber-600/15' : ''}`}
    >
      {/* ゴミ箱移動の取消トースト（数秒で自動的に消える。Ctrl+Z でも取消可） */}
      {undoTrash && (
        <div className="pointer-events-none absolute inset-x-0 bottom-3 z-20 flex justify-center px-3">
          <div className="pointer-events-auto flex items-center gap-3 rounded-lg bg-neutral-800/95 px-3 py-2 text-sm text-white shadow-lg ring-1 ring-white/10">
            <span>{t('mailbox.trashedToast', { count: undoTrash.count })}</span>
            <button
              onClick={() => void undoLastTrash()}
              className="flex items-center gap-1 rounded-md bg-white/10 px-2 py-1 text-xs text-sky-200 hover:bg-white/20"
            >
              <Undo2 size={13} />
              {t('mailbox.undo')} (Ctrl+Z)
            </button>
          </div>
        </div>
      )}
      {/* アカウント選択＋フォルダ選択（アイコンボタン）を同じ行に置く */}
      <div className="flex shrink-0 items-center gap-2 border-b border-white/10 px-2 py-1.5">
        <select
          className="min-w-0 flex-1 rounded-md bg-white/10 px-2 py-1 text-xs outline-none"
          value={selected ?? ''}
          onChange={(e) => setSelected(e.target.value === 'all' ? 'all' : Number(e.target.value))}
        >
          {/* 全アカウント横断表示。既定は「全て」。複数アカウントがある時のみ選べる。 */}
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
        {/* 迷惑登録の矛盾（住所録/グリーンなのに迷惑）を知らせる情報アイコン。矛盾が無ければ非表示。 */}
        <SpamConflictAlert onResolved={() => loadMails({ keepScroll: true })} />
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
        {/* 反転（除外）: 選択中トグルに「一致しない」ものを表示。ツールバー右端に置く。
            不要メール（既読・知り合い以外 等）を一気に絞って一括選択するのに使う。 */}
        <button
          onClick={() => setFilterInvert((v) => !v)}
          disabled={filters.size === 0}
          title={t('filter.invert')}
          aria-label={t('filter.invert')}
          aria-pressed={filterInvert}
          className={`flex h-8 w-8 items-center justify-center rounded-md disabled:opacity-40 ${
            filterInvert && filters.size > 0
              ? 'bg-amber-500/30 text-amber-200 ring-1 ring-amber-300/40'
              : 'text-white/55 hover:text-white/80'
          }`}
        >
          <FlipHorizontal2 size={15} />
        </button>
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
        <div className="flex shrink-0 items-center gap-2 border-b border-white/10 px-3 py-2 text-xs text-white/60">
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
          <span className="flex-1">
            {selectingAll
              ? t('mailbox.selecting')
              : allMatching
                ? t('mailbox.selectedAllMatching', { count: matchCount })
                : t('ctx.selected', { count: selectedIds.size })}
          </span>
          <button onClick={resetSelection} className="hover:text-white/90">
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
          ref={listRef}
          className="h-full space-y-1 overflow-y-auto p-2"
          onScroll={onListScroll}
        >
          {visibleMails.length === 0 ? (
            <li className="px-2 py-3 text-sm text-white/50">
              {searchMode
                ? searching
                  ? t('search.searching')
                  : t('search.noResults')
                : t('mailbox.empty')}
            </li>
          ) : (
            visibleMails.map((m) => (
              <li
                key={m.id}
                id={`mail-li-${m.id}`}
                onClick={(e) => onRowClick(e, m.id)}
                onContextMenu={(e) => onRowContextMenu(e, m.id)}
                className={`group relative flex cursor-pointer select-none gap-2 rounded-md px-3 py-2 hover:bg-white/10 ${
                  selectedIds.has(m.id) ? 'bg-white/15' : ''
                } ${opened?.id === m.id ? 'ring-2 ring-sky-400/70' : ''}`}
              >
                {/* ホバーで出る行アクション（スター／迷惑／削除）。行の展開に伝播させない。
                    右上（日時の上）に重ねる。星は付与済みなら常時アンバー。 */}
                <div className="absolute right-1.5 top-1.5 z-10 flex items-center gap-0.5 rounded-md border border-white/10 bg-neutral-900/85 px-1 py-0.5 opacity-0 shadow-sm backdrop-blur transition-opacity group-hover:opacity-100">
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      void rowToggleStar(m);
                    }}
                    title={t(m.is_starred ? 'ctx.unstar' : 'ctx.star')}
                    aria-label={t(m.is_starred ? 'ctx.unstar' : 'ctx.star')}
                    className="flex h-6 w-6 items-center justify-center rounded hover:bg-white/15"
                  >
                    <Star
                      size={14}
                      className={m.is_starred ? 'fill-amber-300 text-amber-300' : 'text-white/70'}
                    />
                  </button>
                  {folder === 'spam' ? (
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        void rowSpam(m, false);
                      }}
                      title={t('ctx.notSpam')}
                      aria-label={t('ctx.notSpam')}
                      className="flex h-6 w-6 items-center justify-center rounded text-white/70 hover:bg-white/15 hover:text-emerald-300"
                    >
                      <ThumbsUp size={14} />
                    </button>
                  ) : folder !== 'sent' && folder !== 'drafts' && folder !== 'trash' ? (
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        void rowSpam(m, true);
                      }}
                      title={t('ctx.markSpam')}
                      aria-label={t('ctx.markSpam')}
                      className="flex h-6 w-6 items-center justify-center rounded text-white/70 hover:bg-white/15 hover:text-rose-300"
                    >
                      <ThumbsDown size={14} />
                    </button>
                  ) : null}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      void deleteRows([m.id]);
                    }}
                    title={t(folder === 'trash' ? 'ctx.deletePermanent' : 'ctx.delete')}
                    aria-label={t(folder === 'trash' ? 'ctx.deletePermanent' : 'ctx.delete')}
                    className="flex h-6 w-6 items-center justify-center rounded text-white/70 hover:bg-white/15 hover:text-rose-300"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
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
                    const name =
                      headerName || addr.name || addr.email || (outgoing ? '—' : '(no sender)');
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
                            {m.is_starred && (
                              <Star size={12} className="fill-amber-300 text-amber-300" />
                            )}
                            {formatDateTime(m.date)}
                          </span>
                        </div>
                        {/* 件名（スレッド複数通なら「N通」バッジを先頭に） */}
                        <div className="truncate text-sm text-white/80">
                          {m.message_count > 1 && (
                            <span className="mr-1 inline-flex items-center rounded-full bg-sky-400/20 px-1.5 text-[10px] font-medium text-sky-200 align-[1px]">
                              {t('thread.count', { count: m.message_count })}
                            </span>
                          )}
                          {m.subject ?? '(no subject)'} {m.has_real_attachments && '📎'}
                        </div>
                        {/* 本文（詰めて2行折り返し） */}
                        <div className="line-clamp-2 text-xs leading-snug text-white/40">
                          {m.preview}
                        </div>
                      </>
                    );
                  })()}
                </div>
              </li>
            ))
          )}
          {/* 続きあり: スクロールで追加読み込み。表示中/全スレッド数を出す（あとどれくらいか分かる）。 */}
          {!searchMode && hasMore && visibleMails.length > 0 && (
            <li className="px-2 py-3 text-center text-xs text-white/35">
              {threadTotal != null
                ? t('mailbox.loadedOf', { shown: mails.length, total: threadTotal })
                : t('mailbox.loadingMore')}
            </li>
          )}
        </ul>
      </div>
    </div>
  );

  // 一覧（通常＋検索結果）から 1 通を引く。会話ビューの各メッセージのタグ/スター解決に使う。
  const findMail = (id: number) =>
    mails.find((m) => m.id === id) ?? searchResults.find((m) => m.id === id);

  // 会話ビュー（Conversation）へ渡す、メール 1 通単位の操作・状態。
  const conversationHandlers: ConversationHandlers = {
    tagsFor: (id) =>
      (findMail(id)?.tag_ids ?? [])
        .map((tid) => tagById.get(tid))
        .filter((tg): tg is TagSummary => tg != null),
    starredFor: (id) => findMail(id)?.is_starred ?? false,
    onToggleStar: async (id, next) => {
      // 会話ビューはメッセージ単位の真値を渡してくる（一覧は代表行しか持たないため）。
      const value = next ?? !(findMail(id)?.is_starred ?? false);
      patchMails(new Set([id]), { is_starred: value });
      try {
        await mailSetStarred([id], value);
      } catch {
        /* noop */
      }
    },
    onTag: (id, x, y) => setTagPicker({ x, y, ids: [id] }),
    onRemoveTag: (id, tagId) => applyTagDelta([id], tagId, false),
    // 単一メールの既読/未読。所属スレッド行の未読数もその場で増減する
    // （行の email_ids は現フォルダ内の id 群＝未読数の母集合と一致）。
    onSetRead: async (id, read) => {
      // 会話内で手動に既読/未読を切り替えたら、離脱時の自動既読の予約を取り消す（手動を優先）。
      if (activeReadRef.current && activeReadRef.current.ids.includes(id)) {
        activeReadRef.current = null;
      }
      updateLists((prev) =>
        prev.map((row) => {
          if (!row.email_ids.includes(id)) return row;
          const unread = Math.max(0, row.unread_count + (read ? -1 : 1));
          return { ...row, unread_count: unread, is_read: unread === 0 };
        })
      );
      try {
        await mailSetRead([id], read);
      } catch {
        /* noop */
      }
    },
    // 単一メールを削除。既定はゴミ箱へ移動（Ctrl+Z で取消可）。ゴミ箱内は完全削除（確認あり）。
    // 一覧は再読込で正す（代表・件数が変わるため）。
    onDelete: async (id) => {
      if (folder === 'trash') {
        if (!window.confirm(t('mailbox.deletePermanentConfirm', { count: 1 }))) return;
        try {
          await mailDelete([id]);
        } catch {
          /* noop */
        }
      } else {
        try {
          await mailTrash([id]);
        } catch {
          /* noop */
        }
        setUndoTrash({ ids: [id], count: 1 });
      }
      await loadMails();
    },
    onMarkSpam: async (id) => {
      updateLists((prev) => prev.filter((m) => m.id !== id));
      if (opened?.id === id) setOpened(null);
      try {
        await mailMarkSpam([id]);
      } catch {
        /* noop */
      }
      // 同じ差出人の他メールも迷惑へ移るため、一覧を取り直して反映する。
      await loadMails();
    },
    // 迷惑フォルダで「非迷惑に戻す」（隔離解除＋ham 学習）。表示から外し受信箱へ戻す。
    onMarkNotSpam: async (id) => {
      updateLists((prev) => prev.filter((m) => m.id !== id));
      if (opened?.id === id) setOpened(null);
      try {
        await mailMarkNotSpam([id]);
      } catch {
        /* noop */
      }
      // 迷惑差出人の解除で同アドレスのメールが受信箱へ戻るため、一覧を取り直して反映する。
      await loadMails();
    },
    isSpam: folder === 'spam',
    // 特定のメッセージへ返信/転送（会話内のどのメールにも返信できる）。
    onReply: async (mode, messageId) => {
      try {
        const source = await mailGet(messageId);
        setCompose({ mode, source });
      } catch {
        /* noop */
      }
    },
    // アドレスの ＋（追加）／編集は、住所録ページへ移動せず、右パネルで開く（メールを見ながら編集）。
    onAddContact: (name, email) => openContactPanel({ kind: 'prefill', prefill: { name, email } }),
    onEditContact: (id) => openContactPanel({ kind: 'existing', id }),
    onComposeTo: (email) => setCompose({ mode: 'new', to: `${email}, ` }),
    // 本文日付の ＋ / ヘッダの「カレンダーに追加」から、右ペインにカレンダー入力を開く。
    onAddCalendar: (init) => openCalendarPanel(init),
    onGreenChange: loadMails,
    onThreadChanged: loadMails,
  };

  const bodyPane = opened ? (
    <Conversation
      openedId={opened.id}
      folder={folder}
      handlers={conversationHandlers}
      query={query.trim() ? query : undefined}
    />
  ) : (
    <div className="flex h-full items-center justify-center text-sm text-white/40">
      {t('mailbox.selectMail')}
    </div>
  );

  // 作成画面の右ペインに並べて表示する元メール（返信/転送、または返信下書きの再編集）。
  // 返信下書きでも元メールを解決できなかった場合は null（全幅表示になる）。
  const composeSource: MailDetail | null =
    compose && 'source' in compose ? (compose.source ?? null) : null;

  // メール作成はページとして表示（別ウィンドウにしない）。
  const composeEl = compose ? (
    <Compose
      accounts={accounts}
      // 返信/転送は元メールを受信したアカウントを既定に。新規は選択中（全ての時は先頭）。
      defaultAccountId={
        (composeSource?.account_id ?? null) ?? queryAccount ?? accounts[0]?.id ?? null
      }
      target={compose}
      onDraftId={onComposeDraftChange}
      onClose={() => {
        setCompose(null);
        // 作成画面を閉じたら復元対象もクリア（次にメールへ戻っても勝手に開かない）。
        onComposeDraftChange?.(null);
        // 送信・下書き保存の結果を一覧へ反映（下書きフォルダ表示中でも見えるように）。
        void loadMails();
      }}
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
        // メール作成はページとして表示（サイドバーは閉じる）。返信/転送や返信下書きの
        // 再編集は、左=下書き・右=元メールの2分割で並べて作成できる。
        composeSource ? (
          <div
            className="grid grid-rows-1 min-h-0 flex-1 overflow-hidden"
            style={{ gridTemplateColumns: '1fr 1fr' }}
          >
            <div className="min-h-0 overflow-hidden">{composeEl}</div>
            <div className="flex min-h-0 flex-col overflow-hidden border-l border-white/10">
              <MailBody detail={composeSource} />
            </div>
          </div>
        ) : (
          <div className="min-h-0 flex-1 overflow-hidden">{composeEl}</div>
        )
      ) : contactPanel ? (
        // 住所録の編集: 左＝閲覧ペイン、右＝編集パネル。一覧サイドバーは隠して2画面にする。
        <div
          ref={panelSplitRef}
          className="grid grid-rows-1 min-h-0 flex-1 overflow-hidden"
          style={{ gridTemplateColumns: `1fr 6px ${contactPanelW}px` }}
        >
          <div className="min-h-0 overflow-hidden">{bodyPane}</div>
          <div
            onMouseDown={startPanelResize}
            title={t('mailbox.resize')}
            className="cursor-col-resize bg-transparent transition-colors hover:bg-sky-400/40"
          />
          <aside className="flex min-h-0 flex-col overflow-hidden border-l border-white/10">
            <div className="flex shrink-0 items-center gap-2 border-b border-white/10 px-3 py-2">
              <UserRoundPlus size={15} className="shrink-0 text-white/60" />
              <span className="min-w-0 flex-1 truncate text-sm font-medium">
                {contactPanel.kind === 'existing'
                  ? t('contact.panelEdit')
                  : t('contact.panelNew')}
              </span>
              <button
                onClick={closeContactPanel}
                title={t('contact.closePanel')}
                aria-label={t('contact.closePanel')}
                className={iconBtn}
              >
                <X size={16} />
              </button>
            </div>
            <div className="min-h-0 flex-1 overflow-hidden">
              <ContactEditor
                request={contactPanel}
                onDeleted={closeContactPanel}
                onOpenContact={(id) => setContactPanel({ kind: 'existing', id })}
              />
            </div>
          </aside>
        </div>
      ) : calendarPanel ? (
        // カレンダー入力: 左＝閲覧ペイン、右＝カレンダーパネル。一覧サイドバーは隠して2画面にする。
        <div
          ref={panelSplitRef}
          className="grid grid-rows-1 min-h-0 flex-1 overflow-hidden"
          style={{ gridTemplateColumns: `1fr 6px ${contactPanelW}px` }}
        >
          <div className="min-h-0 overflow-hidden">{bodyPane}</div>
          <div
            onMouseDown={startPanelResize}
            title={t('mailbox.resize')}
            className="cursor-col-resize bg-transparent transition-colors hover:bg-sky-400/40"
          />
          <aside className="flex min-h-0 flex-col overflow-hidden border-l border-white/10">
            <div className="flex shrink-0 items-center gap-2 border-b border-white/10 px-3 py-2">
              <CalendarPlus size={15} className="shrink-0 text-white/60" />
              <span className="min-w-0 flex-1 truncate text-sm font-medium">
                {t('cal.panelTitle')}
              </span>
              <button
                onClick={closeCalendarPanel}
                title={t('contact.closePanel')}
                aria-label={t('contact.closePanel')}
                className={iconBtn}
              >
                <X size={16} />
              </button>
            </div>
            <div className="min-h-0 flex-1 overflow-hidden">
              <CalendarPanel initial={calendarPanel} />
            </div>
          </aside>
        </div>
      ) : layout === 'side' ? (
        // grid-rows-1（=minmax(0,1fr)）で単一行の高さをコンテナに固定し、内側の h-full スクロール一覧が
        // 確定したビューポート高を持てるようにする（下の自動追い読み込みの scrollHeight 判定の前提）。
        // 他の分割グリッドも同様。
        <div
          ref={splitRef}
          className="grid grid-rows-1 min-h-0 flex-1 overflow-hidden"
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
          {sidebarOpen && <div className="h-1/3 min-h-0 border-b border-white/10">{listPane}</div>}
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
