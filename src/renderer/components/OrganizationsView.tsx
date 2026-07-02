import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  AlertTriangle,
  Building2,
  Gem,
  Mail,
  Phone,
  Plus,
  Save,
  Search,
  StickyNote,
  Trash2,
  User,
  Users,
  X,
} from 'lucide-react';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import type { OrganizationDetail } from '@bindings/OrganizationDetail';
import {
  organizationDelete,
  organizationDetail,
  organizationList,
  organizationUpsert,
} from '../services/organizations';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 住所録の「組織」タブ。左＝組織一覧（検索）、右＝所属連絡先・共有アドレス（共有件数つき）・
 * 組織名/メモの編集。共有アドレスの指定自体は連絡先編集の「共有」トグルで行う。
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
  const [name, setName] = useState('');
  const [note, setNote] = useState('');
  const [creating, setCreating] = useState(false);
  const [saved, setSaved] = useState(false);

  const load = useCallback((q: string) => {
    if (!isTauri) return;
    organizationList(q)
      .then(setItems)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    const h = setTimeout(() => load(query), 150);
    return () => clearTimeout(h);
  }, [query, load]);

  const open = useCallback((id: number) => {
    if (!isTauri) return;
    setSelectedId(id);
    setCreating(false);
    setSaved(false);
    organizationDetail(id)
      .then((d) => {
        setDetail(d);
        setName(d.org.name);
        setNote(d.org.note ?? '');
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
    setName('');
    setNote('');
    setCreating(true);
    setSaved(false);
  };

  const dirty = detail
    ? name.trim() !== detail.org.name || note.trim() !== (detail.org.note ?? '').trim()
    : creating && name.trim() !== '';

  const save = async () => {
    if (name.trim() === '' || !isTauri) return;
    try {
      const result = await organizationUpsert(
        selectedId,
        name.trim(),
        null,
        note.trim() === '' ? null : note.trim(),
      );
      setSaved(true);
      setCreating(false);
      load(query);
      open(result.id);
    } catch {
      /* noop */
    }
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
      setName('');
      setNote('');
      setCreating(false);
      load(query);
    } catch {
      /* noop */
    }
  };

  const editing = detail !== null || creating;

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
            onClick={startNew}
            title={t('org.new')}
            aria-label={t('org.new')}
            className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white"
          >
            <Plus size={18} />
          </button>
        </div>

        <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {items.length === 0 ? (
            <li className="px-2 py-6 text-center text-sm text-white/45">{t('org.empty')}</li>
          ) : (
            items.map((o) => (
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
            ))
          )}
        </ul>
      </aside>

      {/* 右：組織の詳細・編集 */}
      <section className="min-h-0 flex-1 overflow-y-auto">
        {!editing ? (
          <div className="flex h-full flex-col items-center justify-center gap-1 text-center">
            <Building2 size={40} className="text-white/25" />
            <p className="text-sm text-white/45">{t('org.noSelection')}</p>
          </div>
        ) : (
          <div className="mx-auto max-w-xl p-6">
            <div className="mb-5 flex items-center gap-2">
              <Building2 size={22} className="shrink-0 text-white/50" />
              <input
                className="min-w-0 flex-1 rounded bg-transparent px-1 py-1 text-xl font-semibold outline-none focus:bg-white/10"
                placeholder={t('org.namePlaceholder')}
                value={name}
                onChange={(e) => {
                  setName(e.target.value);
                  setSaved(false);
                }}
              />
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

            {/* メモ */}
            <label className="block">
              <span className="mb-1 flex items-center gap-1.5 text-xs text-white/50">
                <StickyNote size={14} />
                {t('contact.note')}
              </span>
              <textarea
                rows={3}
                className="w-full resize-y rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                value={note}
                onChange={(e) => {
                  setNote(e.target.value);
                  setSaved(false);
                }}
              />
            </label>
          </div>
        )}
      </section>
    </div>
  );
}
