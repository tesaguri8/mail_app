import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  AlertTriangle,
  Briefcase,
  Building2,
  Cake,
  Gem,
  ImageOff,
  Mail,
  MapPin,
  Phone,
  Save,
  StickyNote,
  Trash2,
  User,
} from 'lucide-react';
import type { ContactSummary } from '@bindings/ContactSummary';
import type { ContactInput } from '@bindings/ContactInput';
import type { ContactValue } from '@bindings/ContactValue';
import type { ContactValueInput } from '@bindings/ContactValueInput';
import type { ContactAddressInput } from '@bindings/ContactAddressInput';
import type { ContactMatch } from '@bindings/ContactMatch';
import type { CountryCode } from 'libphonenumber-js';
import {
  contactDelete,
  contactFindMatches,
  contactGet,
  contactUpsert,
} from '../services/contacts';
import { tagList } from '../services/tags';
import { AddressRows, PhoneRows, TagInput, ValueRows, addressToFlat } from './ContactValueEditor';
import { OrgCombobox } from './OrgCombobox';
import { toE164 } from '../utils/phone';
import { formatPostal } from '../utils/postal';
import { getPhoneRegion, getPostalAutoformat } from '../config/prefs';
import { splitPersonName } from '../utils/name';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** メール等から住所録に新規追加するときの初期値（名前・メール）。 */
export type ContactPrefill = { name?: string | null; email?: string | null };

/** 編集フォームに「何を開くか」の指示。参照が変わったときだけ下書きを作り直す。 */
export type EditorRequest =
  | { kind: 'new' }
  | { kind: 'prefill'; prefill: ContactPrefill }
  | { kind: 'existing'; id: number; seed?: ContactSummary };

/** ContactSummary の複数値を入力型（配列）に変換（共有フラグも引き継ぐ）。 */
const toValueInputs = (vs: ContactValue[]): ContactValueInput[] =>
  vs.map((v) => ({ label: v.label, value: v.value, is_shared: v.is_shared }));
const toAddressInputs = (as: ContactAddressInput[]): ContactAddressInput[] =>
  as.map((a) => ({
    label: a.label,
    postal: a.postal,
    region: a.region,
    city: a.city,
    street: a.street,
    extended: a.extended,
    country: a.country,
  }));

/** 保存前に電話を E.164 正準形へ、郵便番号を整形し、flat 主値を配列先頭から導出する。 */
const withPrimaries = (d: ContactInput): ContactInput => {
  const region = getPhoneRegion() as CountryCode;
  const autoPostal = getPostalAutoformat();
  const phones = d.phones.map((p) =>
    p.value.trim() ? { ...p, value: toE164(p.value, region) } : p,
  );
  const addresses = autoPostal
    ? d.addresses.map((a) => (a.postal ? { ...a, postal: formatPostal(a.postal, region) } : a))
    : d.addresses;
  return {
    ...d,
    phones,
    addresses,
    email: d.emails[0]?.value ?? null,
    phone: phones[0]?.value ?? null,
    address: addresses[0] ? addressToFlat(addresses[0]) || null : null,
  };
};

/** 空の下書き（新規作成用）。 */
const emptyDraft = (): ContactInput => ({
  id: null,
  display_name: '',
  family_name: null,
  given_name: null,
  phonetic_family: null,
  phonetic_given: null,
  emails: [],
  phones: [],
  addresses: [],
  tags: [],
  name_kana: null,
  email: null,
  phone: null,
  organization: null,
  org_id: null,
  org_title: null,
  org_department: null,
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
  emails: toValueInputs(c.emails),
  phones: toValueInputs(c.phones),
  addresses: toAddressInputs(c.addresses),
  tags: c.tags,
  name_kana: c.name_kana,
  email: c.email,
  phone: c.phone,
  organization: c.organization,
  org_id: c.org_id,
  org_title: c.org_title,
  org_department: c.org_department,
  address: c.address,
  birthday: c.birthday,
  note: c.note,
  is_favorite: c.is_favorite,
  is_business: c.is_business,
  allow_remote_images: c.allow_remote_images,
});

/** ＋追加のプレフィル（差出人名・メール）から下書きを作る。
 *  表示名は姓・名にも推定分割して、予測できる範囲を自動入力する。 */
export const draftFromPrefill = (prefill: ContactPrefill): ContactInput => {
  const d = emptyDraft();
  const name = prefill.name?.trim() ?? '';
  d.display_name = name;
  if (name) {
    const { family, given } = splitPersonName(name);
    d.family_name = family;
    d.given_name = given;
  }
  const email = prefill.email?.trim();
  if (email) d.emails = [{ label: null, value: email, is_shared: false }];
  return d;
};

/**
 * 連絡先の編集フォーム（住所録の右ペイン／メール画面の右パネルで共有）。
 * request が変わったら、その指示（新規／プレフィル／既存 ID）に沿って下書きを作り直す。
 * 保存・削除・重複判定はこのコンポーネントが持ち、結果は onSaved/onDeleted で親へ通知する。
 */
export function ContactEditor({
  request,
  onSaved,
  onDeleted,
  onOpenContact,
  onReviewDuplicate,
  onDirtyChange,
  placeholder,
}: {
  request: EditorRequest | null;
  /** 保存完了（新規の初回保存も含む）。親は一覧・タグを取り直す。 */
  onSaved?: (contact: ContactSummary) => void;
  onDeleted?: (id: number) => void;
  /** 重複ダイアログ「開く」／（onReviewDuplicate 未指定時の）バナーから既存を開く。 */
  onOpenContact?: (id: number) => void;
  /** 重複バナーのクリック。指定があればこちらを優先（住所録の「重複の整理」へ誘導）。 */
  onReviewDuplicate?: (m: ContactMatch) => void;
  /** 未保存の変更有無を親へ通知（閉じる前の確認などに使う）。 */
  onDirtyChange?: (dirty: boolean) => void;
  /** 何も開いていない時の中央プレースホルダ文言。 */
  placeholder?: string;
}) {
  const { t } = useTranslation();
  // 編集中の下書き。null＝何も開いていない。id:null＝新規。
  const [draft, setDraft] = useState<ContactInput | null>(null);
  // 変更検知の基準（読み込み/保存直後の状態）。
  const [baseline, setBaseline] = useState<string>('');
  const [saved, setSaved] = useState(false);
  // 編集中の値に一致する既存連絡先（重複警告・新規保存前チェック用）。
  const [matches, setMatches] = useState<ContactMatch[]>([]);
  // 新規保存前の重複確認ダイアログの表示。
  const [confirmDup, setConfirmDup] = useState(false);
  // タグ入力の候補（既存タグ名）。保存でタグが増えることがあるので取り直す。
  const [tagNames, setTagNames] = useState<string[]>([]);

  const loadTags = useCallback(() => {
    if (!isTauri) return;
    tagList()
      .then((ts) => setTagNames(ts.map((tg) => tg.name)))
      .catch(() => undefined);
  }, []);
  useEffect(loadTags, [loadTags]);

  const openDraft = (d: ContactInput) => {
    setDraft(d);
    setBaseline(JSON.stringify(d));
  };

  // request（何を開くか）に沿って下書きを作り直す。
  useEffect(() => {
    setSaved(false);
    setMatches([]);
    setConfirmDup(false);
    if (!request) {
      setDraft(null);
      setBaseline('');
      return;
    }
    if (request.kind === 'new') {
      openDraft(emptyDraft());
      return;
    }
    if (request.kind === 'prefill') {
      openDraft(draftFromPrefill(request.prefill));
      return;
    }
    // 既存: seed があれば即表示し、フル取得で上書きする。
    if (request.seed) openDraft(toDraft(request.seed));
    if (!isTauri) return;
    let alive = true;
    contactGet(request.id)
      .then((full) => {
        if (alive) openDraft(toDraft(full));
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
    // openDraft は安定。request の変化だけをトリガにする。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [request]);

  const dirty = useMemo(
    () => (draft ? JSON.stringify(draft) !== baseline : false),
    [draft, baseline],
  );
  useEffect(() => {
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);

  // 編集中の氏名/メール/電話に一致する既存連絡先を軽いデバウンスで検索。
  // 共有指定した自分の値は手掛かりにしない（送らない）。
  const checkEmails = draft
    ? draft.emails.filter((e) => !e.is_shared).map((e) => e.value.trim()).filter(Boolean)
    : [];
  const checkPhones = draft
    ? draft.phones.filter((p) => !p.is_shared).map((p) => p.value.trim()).filter(Boolean)
    : [];
  const checkName = draft?.display_name.trim() ?? '';
  const checkKey = `${draft?.id ?? 'new'}|${checkName}|${checkEmails.join(',')}|${checkPhones.join(',')}`;
  useEffect(() => {
    if (!isTauri || !draft) {
      setMatches([]);
      return;
    }
    if (!checkName && checkEmails.length === 0 && checkPhones.length === 0) {
      setMatches([]);
      return;
    }
    let alive = true;
    const h = setTimeout(() => {
      contactFindMatches(checkEmails, checkPhones, checkName || null, draft.id ?? null)
        .then((m) => alive && setMatches(m))
        .catch(() => alive && setMatches([]));
    }, 250);
    return () => {
      alive = false;
      clearTimeout(h);
    };
    // checkKey に必要な値をまとめている。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [checkKey]);

  // 赤字判定用の集合（バックエンドは渡した文字列をそのまま返す）。
  const emailConflicts = useMemo(() => new Set(matches.flatMap((m) => m.matched_emails)), [matches]);
  const phoneConflicts = useMemo(() => new Set(matches.flatMap((m) => m.matched_phones)), [matches]);
  // 「同名」の一致だけを本当の重複候補とみなす。共有メール/電話だけの一致（＝名前が違う）は
  // 別人（役所の代表アドレスの引き継ぎ等）の可能性が高いので、統合を強制しない。
  const nameMatches = useMemo(() => matches.filter((m) => m.matched_name), [matches]);
  const nameConflict = nameMatches.length > 0;

  const patch = (p: Partial<ContactInput>) => {
    setDraft((d) => (d ? { ...d, ...p } : d));
    setSaved(false);
  };

  // 空文字は NULL に寄せてから送る（検索・並び替えの一貫性のため）。
  const nullify = (s: string) => (s.trim() === '' ? null : s);

  const doSave = async () => {
    if (!draft || draft.display_name.trim() === '') return;
    setConfirmDup(false);
    try {
      const result = await contactUpsert(withPrimaries(draft));
      setSaved(true);
      openDraft(toDraft(result));
      loadTags();
      onSaved?.(result);
    } catch {
      /* noop */
    }
  };

  // 新規（id:null）で「同名の」既存があるときだけ、保存前に統合の確認ダイアログを出す。
  // メール/電話だけの一致（名前が違う＝別人の可能性が高い）は、そのまま別の連絡先として登録する。
  const save = () => {
    if (!draft || draft.display_name.trim() === '') return;
    if (draft.id === null && nameMatches.length > 0) {
      setConfirmDup(true);
      return;
    }
    void doSave();
  };

  const remove = async (id: number) => {
    if (!window.confirm(t('contact.deleteConfirm'))) return;
    try {
      await contactDelete(id);
      onDeleted?.(id);
    } catch {
      /* noop */
    }
  };

  // 重複バナー: 親が「整理」誘導を持てばそこへ、無ければその連絡先を開く。
  const handleReview = (m: ContactMatch) => {
    setConfirmDup(false);
    if (onReviewDuplicate) onReviewDuplicate(m);
    else onOpenContact?.(m.id);
  };

  if (!draft) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-1 text-center">
        <User size={40} className="text-white/25" />
        <p className="text-sm text-white/45">{placeholder ?? t('contact.noSelection')}</p>
      </div>
    );
  }

  return (
    <div className="h-full min-h-0 overflow-y-auto">
      <div className="mx-auto max-w-xl p-6">
        <div className="mb-5 flex items-center gap-2">
          <button
            onClick={() => patch({ is_favorite: !draft.is_favorite })}
            title={t('contact.favorite')}
            aria-label={t('contact.favorite')}
            className="flex h-9 w-9 items-center justify-center rounded-full hover:bg-white/10"
          >
            <Gem
              size={20}
              className={draft.is_favorite ? 'fill-sky-300/30 text-sky-300' : 'text-white/50'}
            />
          </button>
          <input
            className={`min-w-0 flex-1 rounded px-1 py-1 text-xl font-semibold outline-none ${
              nameConflict
                ? 'bg-red-500/10 text-red-100 ring-1 ring-red-400/60 focus:bg-red-500/15'
                : 'bg-transparent focus:bg-white/10'
            }`}
            placeholder={t('contact.namePlaceholder')}
            value={draft.display_name}
            onChange={(e) => patch({ display_name: e.target.value })}
            title={nameConflict ? t('contact.dupInline') : undefined}
          />
          {/* 上部の保存（長い編集フォームの先頭でも保存できる。下部にも同じボタンあり） */}
          <button
            onClick={save}
            disabled={draft.display_name.trim() === '' || (draft.id !== null && !dirty)}
            title={t('contact.save')}
            aria-label={t('contact.save')}
            className="flex h-9 shrink-0 items-center gap-1.5 rounded-full bg-white/20 px-3.5 text-sm font-medium hover:bg-white/30 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Save size={16} />
            {t('contact.save')}
          </button>
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

        {/* 重複候補（既存連絡先と一致）。クリックでその連絡先を開ける。 */}
        {matches.length > 0 && (
          <div className="mb-4 rounded-md border border-amber-300/30 bg-amber-300/10 px-3 py-2.5">
            <div className="mb-1.5 flex items-center gap-1.5 text-xs font-medium text-amber-100">
              <AlertTriangle size={14} />
              {t('contact.dupBanner', { count: matches.length })}
            </div>
            <ul className="space-y-1">
              {matches.map((m) => (
                <li key={m.id}>
                  <button
                    onClick={() => handleReview(m)}
                    className="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs hover:bg-white/10"
                  >
                    <span className="min-w-0 flex-1 truncate">
                      <span className="font-medium">{m.display_name}</span>
                      {(m.organization || m.email) && (
                        <span className="text-white/50"> · {m.organization || m.email}</span>
                      )}
                    </span>
                    <span className="shrink-0 text-[10px] text-amber-200/80">
                      {[
                        m.matched_emails.length > 0 ? t('contact.email') : null,
                        m.matched_phones.length > 0 ? t('contact.phone') : null,
                        m.matched_name ? t('contact.namePlaceholder') : null,
                      ]
                        .filter(Boolean)
                        .join('・')}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}

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
          <TagInput
            tags={draft.tags}
            onChange={(tags) => patch({ tags })}
            suggestions={tagNames}
          />
          <ValueRows
            icon={<Mail size={14} />}
            label={t('contact.email')}
            inputType="email"
            values={draft.emails}
            onChange={(emails) => patch({ emails })}
            shareable
            conflicts={(v) => emailConflicts.has(v.trim())}
          />
          <PhoneRows
            icon={<Phone size={14} />}
            label={t('contact.phone')}
            values={draft.phones}
            onChange={(phones) => patch({ phones })}
            shareable
            conflicts={(v) => phoneConflicts.has(v.trim())}
          />
          <OrgCombobox
            orgId={draft.org_id}
            name={draft.organization ?? ''}
            onChange={(org_id, name) => patch({ org_id, organization: nullify(name) })}
          />
          <div className="flex gap-2">
            <Field icon={<Briefcase size={15} />} label={t('contact.orgTitle')}>
              <input
                className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                value={draft.org_title ?? ''}
                onChange={(e) => patch({ org_title: nullify(e.target.value) })}
              />
            </Field>
            <Field icon={<Building2 size={15} />} label={t('contact.orgDepartment')}>
              <input
                className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                value={draft.org_department ?? ''}
                onChange={(e) => patch({ org_department: nullify(e.target.value) })}
              />
            </Field>
          </div>
          <AddressRows
            icon={<MapPin size={14} />}
            label={t('contact.address')}
            addresses={draft.addresses}
            onChange={(addresses) => patch({ addresses })}
          />
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
            disabled={draft.display_name.trim() === '' || (draft.id !== null && !dirty)}
            className="rounded-md bg-white/20 px-4 py-2 text-sm font-medium hover:bg-white/30 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {t('contact.save')}
          </button>
          {saved && !dirty && <span className="text-sm text-emerald-300">{t('contact.saved')}</span>}
        </div>
      </div>

      {/* 新規登録前の重複確認ダイアログ。 */}
      {confirmDup && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
          onClick={() => setConfirmDup(false)}
        >
          <div
            className="w-full max-w-md rounded-lg border border-white/15 bg-[#141a2e] p-5 shadow-xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mb-2 flex items-center gap-2 text-amber-200">
              <AlertTriangle size={18} />
              <h3 className="text-base font-semibold">{t('contact.dupDialogTitle')}</h3>
            </div>
            <p className="mb-3 text-sm text-white/60">
              {t('contact.dupDialogBody', { count: nameMatches.length })}
            </p>
            <ul className="mb-4 max-h-48 space-y-1 overflow-y-auto">
              {nameMatches.map((m) => (
                <li key={m.id}>
                  <button
                    onClick={() => {
                      setConfirmDup(false);
                      onOpenContact?.(m.id);
                    }}
                    className="flex w-full items-center gap-2 rounded-md bg-white/5 px-3 py-2 text-left text-sm hover:bg-white/10"
                  >
                    <User size={14} className="shrink-0 text-white/40" />
                    <span className="min-w-0 flex-1 truncate">
                      <span className="font-medium">{m.display_name}</span>
                      {(m.organization || m.email) && (
                        <span className="text-white/50"> · {m.organization || m.email}</span>
                      )}
                    </span>
                    <span className="shrink-0 text-xs text-sky-300">{t('contact.dupOpen')}</span>
                  </button>
                </li>
              ))}
            </ul>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setConfirmDup(false)}
                className="rounded-md border border-white/20 px-3 py-1.5 text-sm text-white/70 hover:bg-white/10"
              >
                {t('contact.dupCancel')}
              </button>
              <button
                onClick={doSave}
                className="rounded-md bg-white/20 px-3 py-1.5 text-sm font-medium hover:bg-white/30"
              >
                {t('contact.dupSaveAnyway')}
              </button>
            </div>
          </div>
        </div>
      )}
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
