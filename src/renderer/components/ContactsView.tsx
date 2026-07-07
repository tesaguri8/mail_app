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
import { OrgDuplicates } from './OrgDuplicates';
import { ContactEditor, type EditorRequest, type ContactPrefill } from './ContactEditor';
import { TagFilter } from './TagFilter';
import { tagList } from '../services/tags';
import type { TagSummary } from '@bindings/TagSummary';
import { DEFAULT_TAG_COLOR } from '../utils/tagColors';

// メール等からの＋追加の初期値。編集フォーム側で定義し、ここでは再輸出する。
export type { ContactPrefill };

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

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
  const [items, setItems] = useState<ContactSummary[]>([]);
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

  useEffect(() => {
    if (!isTauri) return;
    trashRetentionGet()
      .then(setRetention)
      .catch(() => undefined);
  }, []);

  const load = useCallback(
    (q: string, groups: Set<number>) => {
      if (!isTauri) return;
      // 削除済み表示のときはゴミ箱（削除済みのみ）を出す。
      contactList(q, [...groups], showDeleted)
        .then((r) => setItems(showDeleted ? r.filter((c) => c.deleted_at != null) : r))
        .catch(() => undefined);
    },
    [showDeleted],
  );

  const reloadTags = useCallback(() => {
    if (!isTauri) return;
    tagList()
      .then(setTags)
      .catch(() => undefined);
  }, []);
  useEffect(reloadTags, [reloadTags]);

  // 検索語・タグ絞り込みの変化に追随（軽いデバウンス）。
  useEffect(() => {
    const h = setTimeout(() => load(query, tagFilter), 150);
    return () => clearTimeout(h);
  }, [query, tagFilter, load]);

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
    load(query, tagFilter);
    reloadTags();
  };

  // 削除完了: 一覧から外し、開いていたのがそれなら閉じる。
  const handleDeleted = (id: number) => {
    setItems((prev) => prev.filter((c) => c.id !== id));
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
      load(query, tagFilter); // 取り込み後に一覧を更新
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
      load(query, tagFilter);
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
        mode={dupMode}
        onModeChange={setDupMode}
        onMerged={() => load(query, tagFilter)}
        onExit={exitCleanup}
      />
    ) : (
      <ContactDuplicates
        focusContactId={cleanupFocusId}
        mode={dupMode}
        onModeChange={setDupMode}
        onMerged={() => load(query, tagFilter)}
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
        <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {items.length === 0 ? (
            <li className="px-2 py-6 text-center text-sm text-white/45">
              {showDeleted ? t('contact.trashEmpty') : t('contact.empty')}
            </li>
          ) : (
            items.map((c) =>
              c.deleted_at ? (
                // 削除済み（ゴミ箱）: 赤字＋残り日数＋復元。
                <li key={c.id}>
                  <div className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-2">
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
                <li key={c.id}>
                  <button
                    onClick={() => openContact(c)}
                    className={`flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left ${
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
                      {(c.organization || c.email) && (
                        <span className="truncate text-xs text-white/45">
                          {c.organization || c.email}
                        </span>
                      )}
                    </span>
                  </button>
                </li>
              ),
            )
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
