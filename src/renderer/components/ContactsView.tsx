import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { Download, Gem, Layers, Plus, RotateCcw, Search, Trash2, User, X } from 'lucide-react';
import type { ContactSummary } from '@bindings/ContactSummary';
import type { ContactMatch } from '@bindings/ContactMatch';
import type { ImportReport } from '@bindings/ImportReport';
import { contactFindDuplicates, contactImport, contactList, contactRestore } from '../services/contacts';
import { trashRetentionGet } from '../services/trash';
import { trashDaysLeft } from '../utils/trash';
import { ContactDuplicates } from './ContactDuplicates';
import { DupModeToggle, OrgDuplicates } from './OrgDuplicates';
import { ContactEditor, type EditorRequest, type ContactPrefill } from './ContactEditor';
import { TagFilter } from './TagFilter';
import { tagList } from '../services/tags';
import type { TagSummary } from '@bindings/TagSummary';
import { DEFAULT_TAG_COLOR } from '../utils/tagColors';
import { CONTACTS_SYNCED_EVENT } from '../hooks/useAutoSync';
import { useVirtualRows } from '../hooks/useVirtualRows';
import { useContactsStore } from '../stores/contacts';
import {
  CONTACT_SOURCE_FILTERS,
  ContactLinkMarks,
  matchesSource,
  type ContactSourceFilter,
} from './ContactLinks';

// メール等からの＋追加の初期値。編集フォーム側で定義し、ここでは再輸出する。
export type { ContactPrefill };

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 既定の一覧（検索語・タグ絞り込み・ゴミ箱表示が無い）か。この一覧だけを手元に写しておく。 */
function isDefaultQuery(query: string, tags: Set<number>, showDeleted: boolean): boolean {
  return !query.trim() && tags.size === 0 && !showDeleted;
}

/** 一覧の 1 行の高さ（px）。仮想スクロールのため全行で揃える（アバター 32 ＋ 2 行の文字 ＋ 上下の余白）。 */
const ROW_HEIGHT = 52;

/**
 * 住所録（アドレス帳）。左に検索付き一覧、右に詳細・編集フォーム（ContactEditor）。
 * docs/FEATURE_SPEC.md §2.4。Google/iCloud 連携・グループ編集は後続。
 * prefill: メールの＋追加などから渡す新規作成の初期値（消費後に onPrefillConsumed を呼ぶ）。
 */
export function ContactsView({
  prefill,
  onPrefillConsumed,
  openId,
  onOpenIdConsumed,
}: {
  prefill?: ContactPrefill | null;
  onPrefillConsumed?: () => void;
  /** この ID の連絡先を開く（組織タブの所属クリックからの遷移用）。 */
  openId?: number | null;
  onOpenIdConsumed?: () => void;
} = {}) {
  const { t } = useTranslation();
  // 既定の一覧（検索・絞り込み・ゴミ箱なし）は手元の写しをすぐ出し、裏で取り直す。
  const cached = useContactsStore((s) => s.items);
  const refreshCached = useContactsStore((s) => s.refresh);
  const removeCached = useContactsStore((s) => s.remove);
  // 検索・タグ絞り込み・ゴミ箱表示のときの結果（その都度取りに行く）。
  const [filtered, setFiltered] = useState<ContactSummary[] | null>(null);
  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<number | null>(null);
  // 編集フォームに「何を開くか」の指示。null＝何も開いていない。
  const [request, setRequest] = useState<EditorRequest | null>(null);
  const [importing, setImporting] = useState(false);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [importError, setImportError] = useState<string | null>(null);
  const [cleanup, setCleanup] = useState(false);
  // 重複整理のモード（連絡先の重複／組織名の統一）。
  const [dupMode, setDupMode] = useState<'contacts' | 'orgs'>('contacts');
  // 重複整理を開くとき、最初に選択したい連絡先（重複バナーからの遷移）。
  const [cleanupFocusId, setCleanupFocusId] = useState<number | null>(null);
  const [tags, setTags] = useState<TagSummary[]>([]);
  const [tagFilter, setTagFilter] = useState<Set<number>>(new Set());
  // 削除済み（ゴミ箱）を表示するか、と保持日数（残り日数表示用）。
  const [showDeleted, setShowDeleted] = useState(false);
  const [retention, setRetention] = useState(7);
  // 同期先での絞り込み（一覧が links を持つので画面側で絞る）。
  const [source, setSource] = useState<ContactSourceFilter>('all');
  const isDefaultView = isDefaultQuery(query, tagFilter, showDeleted);
  // 絞り込みの結果が届くまでは、既定の一覧を出しておく（空の画面で待たせない）。
  const listed = isDefaultView ? cached : (filtered ?? cached);
  const items = listed ?? [];
  const shownItems = items.filter((c) => matchesSource(c.links, source));
  // 数千件を全部描くと重いので、見えている行だけ描く。
  const rows = useVirtualRows<HTMLUListElement>(shownItems.length, ROW_HEIGHT);

  useEffect(() => {
    if (!isTauri) return;
    trashRetentionGet()
      .then(setRetention)
      .catch(() => undefined);
  }, []);

  const load = useCallback(
    (q: string, groups: Set<number>) => {
      if (!isTauri) return;
      if (isDefaultQuery(q, groups, showDeleted)) {
        void refreshCached();
        return;
      }
      // 削除済み表示のときはゴミ箱（削除済みのみ）を出す。
      contactList(q, [...groups], showDeleted)
        .then((r) => setFiltered(showDeleted ? r.filter((c) => c.deleted_at != null) : r))
        .catch(() => undefined);
    },
    [showDeleted, refreshCached],
  );

  // 保存・削除・同期などのあと: いま見ている一覧と、既定の一覧の写しの両方を取り直す。
  const reload = useCallback(() => {
    load(query, tagFilter);
    if (!isDefaultQuery(query, tagFilter, showDeleted)) void refreshCached();
  }, [load, query, tagFilter, showDeleted, refreshCached]);

  const reloadTags = useCallback(() => {
    if (!isTauri) return;
    tagList()
      .then(setTags)
      .catch(() => undefined);
  }, []);
  useEffect(reloadTags, [reloadTags]);

  // 検索語・タグ絞り込みの変化に追随（打鍵中は軽いデバウンス）。既定の一覧（タブに入ったとき・
  // 検索を消したとき）は待たずに取り直す（写しは既に出ているので、差し替えるだけ）。
  useEffect(() => {
    if (isDefaultQuery(query, tagFilter, showDeleted)) {
      load(query, tagFilter);
      return;
    }
    const h = setTimeout(() => load(query, tagFilter), 150);
    return () => clearTimeout(h);
  }, [query, tagFilter, showDeleted, load]);

  // 自動同期が Google の連絡先を住所録へ反映したら、絞り込みの結果とタグを取り直す（タグも増えうる）。
  // 既定の一覧の写しは App が同じ契機で取り直す。
  useEffect(() => {
    const onSynced = () => {
      if (!isDefaultQuery(query, tagFilter, showDeleted)) load(query, tagFilter);
      reloadTags();
    };
    window.addEventListener(CONTACTS_SYNCED_EVENT, onSynced);
    return () => window.removeEventListener(CONTACTS_SYNCED_EVENT, onSynced);
  }, [query, tagFilter, showDeleted, load, reloadTags]);

  const openContact = (c: ContactSummary) => {
    setSelectedId(c.id);
    // 一覧は軽量（複数値が空）なので、seed を渡しつつ編集フォーム側でフル取得させる。
    setRequest({ kind: 'existing', id: c.id, seed: c });
  };

  // id だけ分かっている連絡先（重複候補バナー等）を開く。
  const openContactById = (id: number) => {
    setSelectedId(id);
    setRequest({ kind: 'existing', id });
  };

  // 重複バナー/ダイアログのクリック: 保存済み同士で重複グループがあれば
  // 「重複の整理」へ遷移して統合につなげる。無ければその連絡先を開く。
  const reviewDuplicate = async (m: ContactMatch) => {
    if (isTauri) {
      try {
        const groups = await contactFindDuplicates();
        const hasGroup = groups.some((g) => g.contacts.some((c) => c.id === m.id));
        if (hasGroup) {
          setCleanupFocusId(m.id);
          setCleanup(true);
          return;
        }
      } catch {
        /* noop: 取得失敗時は連絡先を開くだけにフォールバック */
      }
    }
    openContactById(m.id);
  };

  const startNew = () => {
    setSelectedId(null);
    setRequest({ kind: 'new' });
  };

  // メール等からの＋追加: 名前・メールを埋めた新規フォームを開く（消費したら親へ通知）。
  useEffect(() => {
    if (!prefill) return;
    setSelectedId(null);
    setRequest({ kind: 'prefill', prefill });
    onPrefillConsumed?.();
    // prefill オブジェクトの変化だけをトリガにする。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [prefill]);

  // 組織タブの所属クリックなどから、特定の連絡先を開く。
  useEffect(() => {
    if (openId == null) return;
    openContactById(openId);
    onOpenIdConsumed?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [openId]);

  // 保存完了: 選択を保存された連絡先に合わせ、一覧・タグを取り直す。
  const handleSaved = (c: ContactSummary) => {
    setSelectedId(c.id);
    reload();
    reloadTags();
  };

  // 削除完了: 一覧から外し、開いていたのがそれなら閉じる。
  const handleDeleted = (id: number) => {
    removeCached(id);
    setFiltered((prev) => prev?.filter((c) => c.id !== id) ?? null);
    reload();
    if (selectedId === id) {
      setSelectedId(null);
      setRequest(null);
    }
  };

  const runImport = async () => {
    if (!isTauri || importing) return;
    setImportError(null);
    let path: string | null = null;
    try {
      const picked = await open({
        multiple: false,
        filters: [{ name: 'vCard / Google CSV', extensions: ['vcf', 'csv'] }],
      });
      path = typeof picked === 'string' ? picked : null;
    } catch (e) {
      setImportError(`ファイル選択に失敗しました: ${String(e)}`);
      return;
    }
    if (!path) return; // キャンセル
    setImporting(true);
    setReport(null);
    try {
      const result = await contactImport(path);
      setReport(result);
      reload(); // 取り込み後に一覧を更新
      reloadTags(); // 取り込みで作られたタグを反映
    } catch (e) {
      setImportError(`取り込みに失敗しました: ${String(e)}`);
    } finally {
      setImporting(false);
    }
  };

  // ゴミ箱からの復元。
  const restore = async (id: number) => {
    try {
      await contactRestore(id);
      reload();
    } catch {
      /* noop */
    }
  };

  // 整理モードは専用の2ペイン画面を全幅で表示する。
  if (cleanup) {
    const exitCleanup = () => {
      setCleanup(false);
      setCleanupFocusId(null);
    };
    return dupMode === 'orgs' ? (
      <OrgDuplicates
        modeToggle={<DupModeToggle mode={dupMode} onChange={setDupMode} />}
        onMerged={reload}
        onExit={exitCleanup}
      />
    ) : (
      <ContactDuplicates
        focusContactId={cleanupFocusId}
        mode={dupMode}
        onModeChange={setDupMode}
        onMerged={reload}
        onExit={exitCleanup}
      />
    );
  }

  return (
    <div className="flex h-full min-h-0">
      {/* 左：検索 + 一覧 */}
      <aside className="flex w-72 shrink-0 flex-col border-r border-white/10">
        {/* 1行目: 検索（全幅）。2行目: タグ絞り込み＋整理/取り込み/追加のアイコン群。 */}
        <div className="flex flex-col gap-2 p-3">
          <div className="flex items-center gap-2 rounded-md bg-white/10 px-2.5 py-1.5">
            <Search size={15} className="shrink-0 text-white/50" />
            <input
              className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-white/40"
              placeholder={t('contact.search')}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            {query && (
              <button
                onClick={() => setQuery('')}
                title={t('contact.clearSearch')}
                aria-label={t('contact.clearSearch')}
                className="flex h-4 w-4 shrink-0 items-center justify-center rounded-full text-white/40 hover:bg-white/20 hover:text-white"
              >
                <X size={12} />
              </button>
            )}
          </div>
          <div className="flex items-center gap-2">
            {tags.length > 0 && (
              <div className="shrink-0">
                <TagFilter tags={tags} value={tagFilter} onChange={setTagFilter} variant="round" />
              </div>
            )}
            <span className="flex-1" />
            <button
              onClick={() => setShowDeleted((v) => !v)}
              title={t('contact.showDeleted')}
              aria-label={t('contact.showDeleted')}
              className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 hover:bg-white/10 hover:text-white ${
                showDeleted ? 'bg-red-500/25 text-red-200' : 'text-white/70'
              }`}
            >
              <Trash2 size={16} />
            </button>
            <button
              onClick={() => {
                setCleanupFocusId(null);
                setCleanup((v) => !v);
              }}
              title={t('dupes.title')}
              aria-label={t('dupes.title')}
              className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 hover:bg-white/10 hover:text-white ${
                cleanup ? 'bg-white/25 text-white' : 'text-white/70'
              }`}
            >
              <Layers size={17} />
            </button>
            <button
              onClick={runImport}
              disabled={importing}
              title={t('contact.import')}
              aria-label={t('contact.import')}
              className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white disabled:opacity-40"
            >
              <Download size={17} />
            </button>
            <button
              onClick={startNew}
              title={t('contact.new')}
              aria-label={t('contact.new')}
              className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white"
            >
              <Plus size={18} />
            </button>
          </div>
        </div>

        {importError && (
          <div className="mx-3 mb-2 flex items-start justify-between gap-2 rounded-md bg-red-500/20 px-3 py-2 text-xs text-red-100">
            <span className="break-all">{importError}</span>
            <button
              onClick={() => setImportError(null)}
              className="shrink-0 text-red-200/60 hover:text-white"
            >
              ×
            </button>
          </div>
        )}

        {(importing || report) && (
          <div className="mx-3 mb-2 rounded-md bg-white/10 px-3 py-2 text-xs text-white/70">
            {importing
              ? t('contact.importing')
              : report && (
                  <span className="flex items-center justify-between gap-2">
                    <span>
                      {t('contact.importResult', {
                        imported: report.imported,
                        updated: report.updated,
                        skipped: report.skipped,
                      })}
                    </span>
                    <button
                      onClick={() => setReport(null)}
                      className="shrink-0 text-white/40 hover:text-white/80"
                    >
                      ×
                    </button>
                  </span>
                )}
          </div>
        )}

        {/* 選択中のタグ: チップで並べ、× で個別に解除できる（メールのサイドバーと同じ） */}
        {tagFilter.size > 0 && (
          <div className="flex flex-wrap gap-1 px-3 pb-2">
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
          </div>
        )}

        {showDeleted && (
          <div className="mx-3 mb-1 flex items-center gap-1.5 text-[11px] text-red-200/80">
            <Trash2 size={12} />
            {t('contact.trashHint', { days: retention })}
          </div>
        )}
        {/* 同期先での絞り込み（すべて / Google / iCloud / Rondine のみ） */}
        <div className="mx-3 mb-2 flex rounded-md bg-white/5 p-0.5" role="group">
          {CONTACT_SOURCE_FILTERS.map((f) => (
            <button
              key={f}
              onClick={() => setSource(f)}
              aria-pressed={source === f}
              title={t(`contact.sourceHint.${f}`)}
              className={`min-w-0 flex-1 truncate rounded px-1.5 py-1 text-[11px] ${
                source === f ? 'bg-white/20 text-white' : 'text-white/55 hover:text-white/80'
              }`}
            >
              {t(`contact.source.${f}`)}
            </button>
          ))}
        </div>
        <ul ref={rows.ref} onScroll={rows.onScroll} className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {listed === null ? null : shownItems.length === 0 ? (
            <li className="px-2 py-6 text-center text-sm text-white/45">
              {showDeleted
                ? t('contact.trashEmpty')
                : items.length > 0
                  ? t('contact.sourceEmpty')
                  : t('contact.empty')}
            </li>
          ) : (
            <>
              <li aria-hidden style={{ height: rows.padTop }} />
              {shownItems.slice(rows.start, rows.end).map((c) =>
                c.deleted_at ? (
                  // 削除済み（ゴミ箱）: 赤字＋残り日数＋復元。
                  <li key={c.id} style={{ height: ROW_HEIGHT }}>
                    <div className="flex h-full w-full items-center gap-2.5 rounded-md px-2.5 py-2">
                      <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-red-500/15 text-xs font-semibold uppercase text-red-200">
                        {c.display_name.trim().charAt(0) || <User size={15} />}
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-sm font-medium text-red-200">
                          {c.display_name || t('contact.untitled')}
                        </span>
                        <span className="block truncate text-xs text-red-300/70">
                          {t('contact.trashDaysLeft', {
                            count: trashDaysLeft(c.deleted_at, retention),
                          })}
                        </span>
                      </span>
                      <button
                        onClick={() => restore(c.id)}
                        title={t('contact.restore')}
                        aria-label={t('contact.restore')}
                        className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white"
                      >
                        <RotateCcw size={15} />
                      </button>
                    </div>
                  </li>
                ) : (
                  <li key={c.id} style={{ height: ROW_HEIGHT }}>
                    <button
                      onClick={() => openContact(c)}
                      className={`flex h-full w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left ${
                        selectedId === c.id ? 'bg-white/20' : 'hover:bg-white/10'
                      }`}
                    >
                      <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-white/15 text-xs font-semibold uppercase">
                        {c.display_name.trim().charAt(0) || <User size={15} />}
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="flex items-center gap-1 truncate text-sm font-medium">
                          {c.is_favorite && (
                            <Gem size={12} className="shrink-0 fill-sky-300/30 text-sky-300" />
                          )}
                          {c.display_name || t('contact.untitled')}
                        </span>
                        {(c.primary_organization || c.primary_email) && (
                          <span className="block truncate text-xs text-white/45">
                            {c.primary_organization || c.primary_email}
                          </span>
                        )}
                      </span>
                      <ContactLinkMarks links={c.links} />
                    </button>
                  </li>
                ),
              )}
              <li aria-hidden style={{ height: rows.padBottom }} />
            </>
          )}
        </ul>
      </aside>

      {/* 右：詳細・編集（住所録ページとメール画面の右パネルで共有するフォーム） */}
      <section className="min-h-0 flex-1 overflow-hidden">
        <ContactEditor
          request={request}
          onSaved={handleSaved}
          onDeleted={handleDeleted}
          onOpenContact={openContactById}
          onReviewDuplicate={(m) => void reviewDuplicate(m)}
        />
      </section>
    </div>
  );
}
