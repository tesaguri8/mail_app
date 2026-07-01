import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import {
  Briefcase,
  Building2,
  Cake,
  ImageOff,
  Mail,
  MapPin,
  Phone,
  Plus,
  Download,
  Layers,
  Search,
  Star,
  StickyNote,
  Trash2,
  User,
} from 'lucide-react';
import type { ContactSummary } from '@bindings/ContactSummary';
import type { ContactInput } from '@bindings/ContactInput';
import type { ImportReport } from '@bindings/ImportReport';
import { contactDelete, contactImport, contactList, contactUpsert } from '../services/contacts';
import { ContactDuplicates } from './ContactDuplicates';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 空の下書き（新規作成用）。 */
const emptyDraft = (): ContactInput => ({
  id: null,
  display_name: '',
  family_name: null,
  given_name: null,
  phonetic_family: null,
  phonetic_given: null,
  name_kana: null,
  email: null,
  phone: null,
  organization: null,
  address: null,
  birthday: null,
  note: null,
  is_favorite: false,
  is_business: false,
  allow_remote_images: false,
});

const toDraft = (c: ContactSummary): ContactInput => ({
  id: c.id,
  display_name: c.display_name,
  family_name: c.family_name,
  given_name: c.given_name,
  phonetic_family: c.phonetic_family,
  phonetic_given: c.phonetic_given,
  name_kana: c.name_kana,
  email: c.email,
  phone: c.phone,
  organization: c.organization,
  address: c.address,
  birthday: c.birthday,
  note: c.note,
  is_favorite: c.is_favorite,
  is_business: c.is_business,
  allow_remote_images: c.allow_remote_images,
});

/**
 * 住所録（アドレス帳）。左に検索付き一覧、右に詳細・編集フォーム。
 * docs/FEATURE_SPEC.md §2.4。Google/iCloud 連携・グループ編集は後続。
 */
export function ContactsView() {
  const { t } = useTranslation();
  const [items, setItems] = useState<ContactSummary[]>([]);
  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<number | null>(null);
  // 編集中の下書き。null＝何も開いていない。id:null＝新規。
  const [draft, setDraft] = useState<ContactInput | null>(null);
  const [saved, setSaved] = useState(false);
  const [importing, setImporting] = useState(false);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [importError, setImportError] = useState<string | null>(null);
  const [cleanup, setCleanup] = useState(false);

  const load = useCallback(
    (q: string) => {
      if (!isTauri) return;
      contactList(q)
        .then(setItems)
        .catch(() => undefined);
    },
    [],
  );

  // 検索語の変化に追随（軽いデバウンス）。
  useEffect(() => {
    const h = setTimeout(() => load(query), 150);
    return () => clearTimeout(h);
  }, [query, load]);

  const dirty = useMemo(() => {
    if (!draft) return false;
    const original = items.find((c) => c.id === draft.id);
    if (!original) return true; // 新規・未保存
    return JSON.stringify(toDraft(original)) !== JSON.stringify(draft);
  }, [draft, items]);

  const openContact = (c: ContactSummary) => {
    setSelectedId(c.id);
    setDraft(toDraft(c));
    setSaved(false);
  };

  const startNew = () => {
    setSelectedId(null);
    setDraft(emptyDraft());
    setSaved(false);
  };

  const patch = (p: Partial<ContactInput>) => {
    setDraft((d) => (d ? { ...d, ...p } : d));
    setSaved(false);
  };

  // 空文字は NULL に寄せてから送る（検索・並び替えの一貫性のため）。
  const nullify = (s: string) => (s.trim() === '' ? null : s);

  const save = async () => {
    if (!draft || draft.display_name.trim() === '') return;
    try {
      const result = await contactUpsert(draft);
      setSaved(true);
      setSelectedId(result.id);
      setDraft(toDraft(result));
      // 一覧を取り直して並び順・件数を反映。
      contactList(query)
        .then(setItems)
        .catch(() => undefined);
    } catch {
      /* noop */
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
      load(query); // 取り込み後に一覧を更新
    } catch (e) {
      setImportError(`取り込みに失敗しました: ${String(e)}`);
    } finally {
      setImporting(false);
    }
  };

  const remove = async (id: number) => {
    if (!window.confirm(t('contact.deleteConfirm'))) return;
    try {
      await contactDelete(id);
      setItems((prev) => prev.filter((c) => c.id !== id));
      if (selectedId === id) {
        setSelectedId(null);
        setDraft(null);
      }
    } catch {
      /* noop */
    }
  };

  // 整理モードは専用の2ペイン画面を全幅で表示する。
  if (cleanup) {
    return <ContactDuplicates onMerged={() => load(query)} onExit={() => setCleanup(false)} />;
  }

  return (
    <div className="flex h-full min-h-0">
      {/* 左：検索 + 一覧 */}
      <aside className="flex w-72 shrink-0 flex-col border-r border-white/10">
        <div className="flex items-center gap-2 p-3">
          <div className="flex min-w-0 flex-1 items-center gap-2 rounded-md bg-white/10 px-2.5 py-1.5">
            <Search size={15} className="shrink-0 text-white/50" />
            <input
              className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-white/40"
              placeholder={t('contact.search')}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <button
            onClick={() => setCleanup((v) => !v)}
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

        <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {items.length === 0 ? (
            <li className="px-2 py-6 text-center text-sm text-white/45">{t('contact.empty')}</li>
          ) : (
            items.map((c) => (
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
                        <Star size={12} className="shrink-0 fill-amber-300 text-amber-300" />
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
            ))
          )}
        </ul>
      </aside>

      {/* 右：詳細・編集 */}
      <section className="min-h-0 flex-1 overflow-y-auto">
        {!draft ? (
          <div className="flex h-full flex-col items-center justify-center gap-1 text-center">
            <User size={40} className="text-white/25" />
            <p className="text-sm text-white/45">{t('contact.noSelection')}</p>
          </div>
        ) : (
          <div className="mx-auto max-w-xl p-6">
            <div className="mb-5 flex items-center gap-2">
              <button
                onClick={() => patch({ is_favorite: !draft.is_favorite })}
                title={t('contact.favorite')}
                aria-label={t('contact.favorite')}
                className="flex h-9 w-9 items-center justify-center rounded-full hover:bg-white/10"
              >
                <Star
                  size={20}
                  className={draft.is_favorite ? 'fill-amber-300 text-amber-300' : 'text-white/50'}
                />
              </button>
              <input
                className="min-w-0 flex-1 rounded bg-transparent px-1 py-1 text-xl font-semibold outline-none focus:bg-white/10"
                placeholder={t('contact.namePlaceholder')}
                value={draft.display_name}
                onChange={(e) => patch({ display_name: e.target.value })}
              />
              {draft.id !== null && (
                <button
                  onClick={() => remove(draft.id as number)}
                  title={t('contact.delete')}
                  aria-label={t('contact.delete')}
                  className="flex h-9 w-9 items-center justify-center rounded-full border border-white/20 text-white/60 hover:border-red-400/60 hover:bg-red-500/30 hover:text-white"
                >
                  <Trash2 size={17} />
                </button>
              )}
            </div>

            <div className="space-y-3">
              <Field icon={<User size={15} />} label={t('contact.nameLabel')}>
                <div className="flex gap-2">
                  <input
                    className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                    placeholder={t('contact.familyName')}
                    value={draft.family_name ?? ''}
                    onChange={(e) => patch({ family_name: nullify(e.target.value) })}
                  />
                  <input
                    className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                    placeholder={t('contact.givenName')}
                    value={draft.given_name ?? ''}
                    onChange={(e) => patch({ given_name: nullify(e.target.value) })}
                  />
                </div>
              </Field>
              <Field icon={<User size={15} />} label={t('contact.phoneticLabel')}>
                <div className="flex gap-2">
                  <input
                    className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                    placeholder={t('contact.familyName')}
                    value={draft.phonetic_family ?? ''}
                    onChange={(e) => patch({ phonetic_family: nullify(e.target.value) })}
                  />
                  <input
                    className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                    placeholder={t('contact.givenName')}
                    value={draft.phonetic_given ?? ''}
                    onChange={(e) => patch({ phonetic_given: nullify(e.target.value) })}
                  />
                </div>
              </Field>
              <Field icon={<Mail size={15} />} label={t('contact.email')}>
                <input
                  type="email"
                  className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                  value={draft.email ?? ''}
                  onChange={(e) => patch({ email: nullify(e.target.value) })}
                />
              </Field>
              <Field icon={<Phone size={15} />} label={t('contact.phone')}>
                <input
                  className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                  value={draft.phone ?? ''}
                  onChange={(e) => patch({ phone: nullify(e.target.value) })}
                />
              </Field>
              <Field icon={<Building2 size={15} />} label={t('contact.organization')}>
                <input
                  className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                  value={draft.organization ?? ''}
                  onChange={(e) => patch({ organization: nullify(e.target.value) })}
                />
              </Field>
              <Field icon={<MapPin size={15} />} label={t('contact.address')}>
                <input
                  className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                  value={draft.address ?? ''}
                  onChange={(e) => patch({ address: nullify(e.target.value) })}
                />
              </Field>
              <Field icon={<Cake size={15} />} label={t('contact.birthday')}>
                <input
                  type="date"
                  className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15 [color-scheme:dark]"
                  value={draft.birthday ?? ''}
                  onChange={(e) => patch({ birthday: nullify(e.target.value) })}
                />
              </Field>
              <Field icon={<StickyNote size={15} />} label={t('contact.note')}>
                <textarea
                  rows={3}
                  className="w-full resize-y rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                  value={draft.note ?? ''}
                  onChange={(e) => patch({ note: nullify(e.target.value) })}
                />
              </Field>
            </div>

            <div className="mt-4 space-y-2">
              <Toggle
                icon={<Briefcase size={15} />}
                label={t('contact.business')}
                hint={t('contact.businessHint')}
                checked={draft.is_business}
                onChange={(v) => patch({ is_business: v })}
              />
              <Toggle
                icon={<ImageOff size={15} />}
                label={t('contact.allowRemoteImages')}
                hint={t('contact.allowRemoteImagesHint')}
                checked={draft.allow_remote_images}
                onChange={(v) => patch({ allow_remote_images: v })}
              />
            </div>

            <div className="mt-6 flex items-center gap-3">
              <button
                onClick={save}
                disabled={draft.display_name.trim() === '' || !dirty}
                className="rounded-md bg-white/20 px-4 py-2 text-sm font-medium hover:bg-white/30 disabled:cursor-not-allowed disabled:opacity-40"
              >
                {t('contact.save')}
              </button>
              {saved && !dirty && <span className="text-sm text-emerald-300">{t('contact.saved')}</span>}
            </div>
          </div>
        )}
      </section>
    </div>
  );
}

function Field({
  icon,
  label,
  children,
}: {
  icon: React.ReactNode;
  label: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <span className="mb-1 flex items-center gap-1.5 text-xs text-white/50">
        {icon}
        {label}
      </span>
      {children}
    </label>
  );
}

function Toggle({
  icon,
  label,
  hint,
  checked,
  onChange,
}: {
  icon: React.ReactNode;
  label: string;
  hint: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <button
      onClick={() => onChange(!checked)}
      className="flex w-full items-start gap-2.5 rounded-md bg-white/5 px-3 py-2 text-left hover:bg-white/10"
    >
      <span
        className={`mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded ${
          checked ? 'bg-emerald-400/80 text-black' : 'border border-white/30'
        }`}
      >
        {checked && '✓'}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5 text-sm font-medium">
          {icon}
          {label}
        </span>
        <span className="block text-xs text-white/40">{hint}</span>
      </span>
    </button>
  );
}
