import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  AlertTriangle,
  Building2,
  Gem,
  Mail,
  Phone,
  Plus,
  RotateCcw,
  Save,
  Search,
  Trash2,
  User,
  Users,
  X,
} from 'lucide-react';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import type { OrganizationInput } from '@bindings/OrganizationInput';
import type { OrganizationDetail } from '@bindings/OrganizationDetail';
import {
  organizationDelete,
  organizationDetail,
  organizationList,
  organizationMerge,
  organizationRestore,
  organizationUpsert,
} from '../services/organizations';
import { trashRetentionGet } from '../services/trash';
import { trashDaysLeft } from '../utils/trash';
import { OrgCardFields, orgDraft } from './OrgCard';
import { OrgAutocomplete } from './OrgCombobox';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 住所録の「組織」タブ。左＝組織一覧（検索）、右＝組織カード（会社名・代表電話・FAX・
 * 代表メール・URL・所在地・メモ）の編集と、所属連絡先・共有アドレス（共有件数つき）。
 * 共有アドレスの指定自体は連絡先編集の「共有」トグルで行う。
 */
export function OrganizationsView({
  focusId,
  onOpenContact,
}: {
  /** 起動時に開く組織（重複バナー等からの遷移用）。 */
  focusId?: number | null;
  /** 所属連絡先をクリックしたら連絡先タブで開く。 */
  onOpenContact?: (id: number) => void;
}) {
  const { t } = useTranslation();
  const [items, setItems] = useState<OrganizationSummary[]>([]);
  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [detail, setDetail] = useState<OrganizationDetail | null>(null);
  // 編集中の組織カード。null＝何も開いていない。id:null＝新規。
  const [draft, setDraft] = useState<OrganizationInput | null>(null);
  // 変更検知の基準（読み込み/保存直後の状態）。
  const [baseline, setBaseline] = useState('');
  const [saved, setSaved] = useState(false);
  // 会社名オートコンプリートの候補（自分自身は除外）と、統合確認ダイアログの表示。
  const [orgResults, setOrgResults] = useState<OrganizationSummary[]>([]);
  const [confirmMerge, setConfirmMerge] = useState(false);
  // 削除済み（ゴミ箱）を表示するか、と保持日数。
  const [showDeleted, setShowDeleted] = useState(false);
  const [retention, setRetention] = useState(7);

  useEffect(() => {
    if (!isTauri) return;
    trashRetentionGet()
      .then(setRetention)
      .catch(() => undefined);
  }, []);

  const load = useCallback(
    (q: string) => {
      if (!isTauri) return;
      organizationList(q, showDeleted)
        .then((r) => setItems(showDeleted ? r.filter((o) => o.deleted_at != null) : r))
        .catch(() => undefined);
    },
    [showDeleted],
  );

  useEffect(() => {
    const h = setTimeout(() => load(query), 150);
    return () => clearTimeout(h);
  }, [query, load]);

  const openDraft = (d: OrganizationInput) => {
    setDraft(d);
    setBaseline(JSON.stringify(d));
  };

  const open = useCallback((id: number) => {
    if (!isTauri) return;
    setSelectedId(id);
    setSaved(false);
    setOrgResults([]); // 前の検索結果による誤った統合判定を避ける
    organizationDetail(id)
      .then((d) => {
        setDetail(d);
        openDraft(orgDraft(d.org));
      })
      .catch(() => undefined);
  }, []);

  // 起動時フォーカス（重複バナーからの遷移）。
  useEffect(() => {
    if (focusId != null) open(focusId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusId]);

  const startNew = () => {
    setSelectedId(null);
    setDetail(null);
    openDraft(orgDraft(null));
    setSaved(false);
    setOrgResults([]);
  };

  const name = draft?.name ?? '';
  const creating = draft !== null && draft.id === null;
  const dirty = draft ? JSON.stringify(draft) !== baseline : false;

  const patch = (next: OrganizationInput) => {
    setDraft(next);
    setSaved(false);
  };

  // 会社名が別の既存組織の名前に一致するか（候補は自分自身を除外済み）。
  const nameMatch =
    orgResults.find((o) => o.name.trim().toLowerCase() === name.trim().toLowerCase()) ?? null;
  // 既存組織を編集していて別組織名に一致したら「統合」になる。
  const mergeTarget = detail && !creating ? nameMatch : null;

  const doSave = async () => {
    if (!draft) return;
    try {
      const result = await organizationUpsert({ ...draft, name: draft.name.trim() });
      load(query);
      open(result.id);
      setSaved(true);
    } catch {
      /* noop */
    }
  };

  // 現在の組織を mergeTarget に統合（keep=統合先, drop=現在, 統一名=入力名）。
  const doMerge = async () => {
    if (!detail || !mergeTarget || !isTauri) return;
    try {
      await organizationMerge(mergeTarget.id, [detail.org.id], name.trim());
      setConfirmMerge(false);
      load(query);
      open(mergeTarget.id);
    } catch {
      /* noop */
    }
  };

  const save = () => {
    if (name.trim() === '' || !isTauri) return;
    if (mergeTarget) {
      // 別組織名に一致 → 統合。確定前に確認する。
      setConfirmMerge(true);
      return;
    }
    if (creating && nameMatch) {
      // 新規作成で既存名に一致 → 重複作成を避け、その組織を開く。
      open(nameMatch.id);
      return;
    }
    void doSave();
  };

  // 削除できるのは、所属している連絡先が 0 の既存組織だけ。
  const canDelete = detail !== null && detail.members.length === 0;
  const remove = async () => {
    if (!detail || !canDelete || !isTauri) return;
    if (!window.confirm(t('org.deleteConfirm', { name: detail.org.name }))) return;
    try {
      await organizationDelete(detail.org.id);
      setSelectedId(null);
      setDetail(null);
      setDraft(null);
      setBaseline('');
      load(query);
    } catch {
      /* noop */
    }
  };

  // ゴミ箱からの復元。
  const restore = async (id: number) => {
    try {
      await organizationRestore(id);
      load(query);
    } catch {
      /* noop */
    }
  };

  return (
    <div className="flex h-full min-h-0">
      {/* 左：検索 + 組織一覧 */}
      <aside className="flex w-72 shrink-0 flex-col border-r border-white/10">
        <div className="flex items-center gap-2 p-3">
          <div className="flex min-w-0 flex-1 items-center gap-2 rounded-md bg-white/10 px-2.5 py-1.5">
            <Search size={15} className="shrink-0 text-white/50" />
            <input
              className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-white/40"
              placeholder={t('org.search')}
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
            onClick={startNew}
            title={t('org.new')}
            aria-label={t('org.new')}
            className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white"
          >
            <Plus size={18} />
          </button>
        </div>

        {showDeleted && (
          <div className="mx-3 mb-1 flex items-center gap-1.5 text-[11px] text-red-200/80">
            <Trash2 size={12} />
            {t('contact.trashHint', { days: retention })}
          </div>
        )}
        <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {items.length === 0 ? (
            <li className="px-2 py-6 text-center text-sm text-white/45">
              {showDeleted ? t('contact.trashEmpty') : t('org.empty')}
            </li>
          ) : (
            items.map((o) =>
              o.deleted_at ? (
                <li key={o.id}>
                  <div className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-2">
                    <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-red-500/15 text-red-200">
                      <Building2 size={15} />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium text-red-200">
                        {o.name}
                      </span>
                      <span className="block truncate text-xs text-red-300/70">
                        {t('contact.trashDaysLeft', {
                          count: trashDaysLeft(o.deleted_at, retention),
                        })}
                      </span>
                    </span>
                    <button
                      onClick={() => restore(o.id)}
                      title={t('contact.restore')}
                      aria-label={t('contact.restore')}
                      className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white"
                    >
                      <RotateCcw size={15} />
                    </button>
                  </div>
                </li>
              ) : (
                <li key={o.id}>
                  <button
                    onClick={() => open(o.id)}
                    className={`flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left ${
                      selectedId === o.id ? 'bg-white/20' : 'hover:bg-white/10'
                    }`}
                  >
                    <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-white/15">
                      <Building2 size={15} />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium">{o.name}</span>
                      <span className="block truncate text-xs text-white/45">
                        {t('org.members', { count: o.member_count })}
                      </span>
                    </span>
                  </button>
                </li>
              ),
            )
          )}
        </ul>
      </aside>

      {/* 右：組織の詳細・編集 */}
      <section className="min-h-0 flex-1 overflow-y-auto">
        {!draft ? (
          <div className="flex h-full flex-col items-center justify-center gap-1 text-center">
            <Building2 size={40} className="text-white/25" />
            <p className="text-sm text-white/45">{t('org.noSelection')}</p>
          </div>
        ) : (
          <div className="mx-auto max-w-xl p-6">
            <div className="mb-1 flex items-center gap-2">
              <Building2 size={22} className="shrink-0 text-white/50" />
              <div className="min-w-0 flex-1">
                <OrgAutocomplete
                  value={name}
                  excludeId={detail?.org.id ?? null}
                  placeholder={t('org.namePlaceholder')}
                  ariaLabel={t('org.namePlaceholder')}
                  inputClassName="w-full rounded bg-transparent px-1 py-1 pr-9 text-xl font-semibold outline-none focus:bg-white/10"
                  onResults={setOrgResults}
                  onChange={(text) => patch({ ...draft, name: text })}
                  onSelect={(o) => patch({ ...draft, name: o.name })}
                />
              </div>
              <button
                onClick={save}
                disabled={name.trim() === '' || !dirty}
                title={t('contact.save')}
                aria-label={t('contact.save')}
                className="flex h-9 shrink-0 items-center gap-1.5 rounded-full bg-white/20 px-3.5 text-sm font-medium hover:bg-white/30 disabled:cursor-not-allowed disabled:opacity-40"
              >
                <Save size={16} />
                {t('contact.save')}
              </button>
              {saved && !dirty && (
                <span className="shrink-0 text-sm text-emerald-300">{t('contact.saved')}</span>
              )}
            </div>

            {/* 別の既存組織名に一致 → 保存すると統合になる旨をその場で知らせる */}
            {mergeTarget && (
              <div className="mb-4 mt-1 flex items-center gap-1.5 text-[11px] text-amber-200">
                <AlertTriangle size={12} className="shrink-0" />
                {t('org.willMerge', { name: mergeTarget.name })}
              </div>
            )}
            {!mergeTarget && <div className="mb-4" />}

            {/* 所属 0 名: 注意を促し、削除できるようにする（削除は所属 0 のときだけ） */}
            {canDelete && (
              <div className="mb-4 flex items-center gap-2 rounded-md border border-amber-300/30 bg-amber-300/10 px-3 py-2.5">
                <AlertTriangle size={16} className="shrink-0 text-amber-300" />
                <span className="flex-1 text-xs text-amber-100">{t('org.emptyWarn')}</span>
                <button
                  onClick={remove}
                  className="flex shrink-0 items-center gap-1 rounded-md border border-red-400/50 bg-red-500/20 px-2.5 py-1 text-xs text-red-100 hover:bg-red-500/40"
                >
                  <Trash2 size={13} />
                  {t('org.delete')}
                </button>
              </div>
            )}

            {/* 組織カード（代表電話・FAX・代表メール・URL・所在地・メモ）。
                所属している連絡先では、この内容をラベル表示する。 */}
            <div className="mb-5">
              <OrgCardFields draft={draft} onChange={patch} />
            </div>

            {/* 共有アドレス（組織 ＋ 値 ＋ 共有件数） */}
            {detail && detail.shared_values.length > 0 && (
              <div className="mb-5">
                <span className="mb-1.5 flex items-center gap-1.5 text-xs text-white/50">
                  <Users size={14} />
                  {t('org.sharedValues')}
                </span>
                <ul className="space-y-1.5">
                  {detail.shared_values.map((v, i) => (
                    <li
                      key={`${v.kind}-${v.value}-${i}`}
                      className="flex items-center gap-2.5 rounded-md bg-white/5 px-3 py-2"
                    >
                      {v.kind === 'email' ? (
                        <Mail size={14} className="shrink-0 text-white/45" />
                      ) : (
                        <Phone size={14} className="shrink-0 text-white/45" />
                      )}
                      {v.label && (
                        <span className="shrink-0 rounded bg-white/10 px-1.5 py-0.5 text-[10px] text-white/60">
                          {v.label}
                        </span>
                      )}
                      <span className="min-w-0 flex-1 truncate text-sm">{v.value}</span>
                      <span className="flex shrink-0 items-center gap-1 rounded-full bg-amber-400/20 px-2 py-0.5 text-[11px] font-medium text-amber-200">
                        <Users size={11} />
                        {t('org.sharedCount', { count: v.count })}
                      </span>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {/* 所属連絡先 */}
            {detail && (
              <div className="mb-5">
                <span className="mb-1.5 flex items-center gap-1.5 text-xs text-white/50">
                  <User size={14} />
                  {t('org.membersLabel', { count: detail.members.length })}
                </span>
                {detail.members.length === 0 ? (
                  <p className="rounded-md bg-white/5 px-3 py-2 text-sm text-white/40">
                    {t('org.noMembers')}
                  </p>
                ) : (
                  <ul className="space-y-1">
                    {detail.members.map((m) => (
                      <li key={m.id}>
                        <button
                          onClick={() => onOpenContact?.(m.id)}
                          className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left hover:bg-white/10"
                        >
                          <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-white/15 text-xs font-semibold uppercase">
                            {m.display_name.trim().charAt(0) || <User size={13} />}
                          </span>
                          <span className="flex min-w-0 flex-1 items-center gap-1 truncate text-sm">
                            {m.is_favorite && (
                              <Gem size={11} className="shrink-0 fill-sky-300/30 text-sky-300" />
                            )}
                            {m.display_name}
                          </span>
                          {m.email && (
                            <span className="shrink-0 truncate text-xs text-white/40">{m.email}</span>
                          )}
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}

          </div>
        )}
      </section>

      {/* 統合の確認（別の既存組織名に一致した会社名で保存したとき） */}
      {confirmMerge && detail && mergeTarget && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
          onClick={() => setConfirmMerge(false)}
        >
          <div
            className="w-full max-w-md rounded-lg border border-white/15 bg-[#141a2e] p-5 shadow-xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mb-2 flex items-center gap-2 text-amber-200">
              <AlertTriangle size={18} />
              <h3 className="text-base font-semibold">{t('org.mergeTitle')}</h3>
            </div>
            <p className="mb-4 text-sm text-white/70">
              {t('org.mergeBody', {
                from: detail.org.name,
                to: mergeTarget.name,
                count: detail.members.length,
              })}
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setConfirmMerge(false)}
                className="rounded-md border border-white/20 px-3 py-1.5 text-sm text-white/70 hover:bg-white/10"
              >
                {t('org.cancel')}
              </button>
              <button
                onClick={doMerge}
                className="rounded-md bg-emerald-500/80 px-3 py-1.5 text-sm font-medium text-white hover:bg-emerald-500"
              >
                {t('org.mergeConfirm')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
