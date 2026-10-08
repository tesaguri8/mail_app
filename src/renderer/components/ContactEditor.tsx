import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  AlertTriangle,
  Briefcase,
  Building2,
  Cake,
  CalendarHeart,
  Gem,
  Globe,
  HeartHandshake,
  ImageOff,
  ListPlus,
  Mail,
  MapPin,
  MessageCircle,
  Phone,
  Plus,
  Save,
  Smile,
  StickyNote,
  Trash2,
  User,
  UserRound,
} from 'lucide-react';
import type { ContactSummary } from '@bindings/ContactSummary';
import type { ContactInput } from '@bindings/ContactInput';
import type { ContactMatch } from '@bindings/ContactMatch';
import type { ContactLink } from '@bindings/ContactLink';
import type { CountryCode } from 'libphonenumber-js';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import {
  contactDelete,
  contactFindMatches,
  contactGet,
  contactUpsert,
} from '../services/contacts';
import { organizationGet } from '../services/organizations';
import { tagList } from '../services/tags';
import {
  AddressRows,
  Field,
  LabelDatalists,
  PhoneRows,
  TagInput,
  ValueRows,
} from './ContactValueEditor';
import { HandleRows, PairRows } from './ContactExtraRows';
import { ContactLinkChips } from './ContactLinks';
import { ConfirmDialog } from './ConfirmDialog';
import { OrgRows } from './ContactOrgRows';
import { OrgCardDialog, OrgCardInfo, OrgOverlapNotice } from './OrgCard';
import { LABEL_LIST_IDS } from '../utils/contactLabels';
import { toE164 } from '../utils/phone';
import { findOrgOverlap, hasOrgOverlap, mergeOrgOverlap } from '../utils/orgOverlap';
import { formatPostal } from '../utils/postal';
import { getPhoneRegion, getPostalAutoformat } from '../config/prefs';
import { joinPersonName, splitPersonName } from '../utils/name';
import {
  contactToInput,
  emptyContactInput,
  isBlankOrganization,
  primaryOrganization,
} from '../utils/contactDraft';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** メール等から住所録に新規追加するときの初期値（名前・メール）。 */
export type ContactPrefill = { name?: string | null; email?: string | null };

/** 編集フォームに「何を開くか」の指示。参照が変わったときだけ下書きを作り直す。 */
export type EditorRequest =
  | { kind: 'new' }
  | { kind: 'prefill'; prefill: ContactPrefill }
  | { kind: 'existing'; id: number; seed?: ContactSummary };

/** 保存前に電話を E.164 正準形へ、郵便番号を整形し、空のまま追加した行を落とす。 */
const normalizeForSave = (d: ContactInput): ContactInput => {
  const region = getPhoneRegion() as CountryCode;
  const autoPostal = getPostalAutoformat();
  const phones = d.phones.map((p) =>
    p.value.trim() ? { ...p, value: toE164(p.value, region) } : p,
  );
  const addresses = autoPostal
    ? d.addresses.map((a) => (a.postal ? { ...a, postal: formatPostal(a.postal, region) } : a))
    : d.addresses;
  const filled = (v: string) => v.trim() !== '';
  return {
    ...d,
    phones,
    addresses,
    organizations: d.organizations.filter((o) => !isBlankOrganization(o)),
    urls: d.urls.filter((u) => filled(u.value)),
    dates: d.dates.filter((x) => filled(x.date)),
    relations: d.relations.filter((r) => filled(r.name)),
    handles: d.handles.filter((h) => filled(h.value)),
    custom_fields: d.custom_fields.filter((c) => filled(c.key) || filled(c.value)),
  };
};

/** 日付の表記（`YYYY-MM-DD` か年なしの `--MM-DD`）として読めるか。 */
const isContactDate = (v: string) => /^(\d{4}|-)-\d{2}-\d{2}$/.test(v.trim());

/** 普段は畳んでおき、「項目を追加」から出す項目。値があれば最初から出す。 */
const OPTIONAL_SECTIONS = [
  'nameDetails',
  'nickname',
  'maidenName',
  'urls',
  'dates',
  'relations',
  'handles',
  'customFields',
] as const;
type OptionalSection = (typeof OPTIONAL_SECTIONS)[number];

/** その項目に値が入っているか（入っていれば畳まずに出す）。 */
const sectionHasData = (d: ContactInput, s: OptionalSection): boolean => {
  switch (s) {
    case 'nameDetails':
      return !!(d.name_prefix || d.middle_name || d.name_suffix || d.phonetic_middle);
    case 'nickname':
      return !!d.nickname;
    case 'maidenName':
      return !!d.maiden_name;
    case 'urls':
      return d.urls.length > 0;
    case 'dates':
      return d.dates.length > 0;
    case 'relations':
      return d.relations.length > 0;
    case 'handles':
      return d.handles.length > 0;
    case 'customFields':
      return d.custom_fields.length > 0;
  }
};

/** 「項目を追加」で出したとき、行の項目なら空の行を 1 つ足す（すぐ入力できるように）。 */
const withFirstRow = (d: ContactInput, s: OptionalSection): Partial<ContactInput> => {
  switch (s) {
    case 'urls':
      return { urls: [...d.urls, { label: null, value: '' }] };
    case 'dates':
      return { dates: [...d.dates, { label: null, date: '' }] };
    case 'relations':
      return { relations: [...d.relations, { label: null, name: '' }] };
    case 'handles':
      return { handles: [...d.handles, { kind: 'im', service: null, value: '', label: null }] };
    case 'customFields':
      return { custom_fields: [...d.custom_fields, { key: '', value: '' }] };
    default:
      return {};
  }
};

const emptyDraft = emptyContactInput;

const INPUT = 'rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15';
const INPUT_FULL = `w-full ${INPUT}`;
const toDraft = contactToInput;

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
  // 削除の確認ダイアログの表示と、削除中か。
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  // タグ入力の候補（既存タグ名）。保存でタグが増えることがあるので取り直す。
  const [tagNames, setTagNames] = useState<string[]>([]);
  // 所属組織のカード（会社共通の代表連絡先。ここではラベル表示のみ）。
  const [org, setOrg] = useState<OrganizationSummary | null>(null);
  // 組織カードの編集ダイアログの表示。
  const [editOrg, setEditOrg] = useState(false);
  // 「組織に登録されています。統合しますか？」を「このままにする」で閉じた組織（編集中だけ覚える）。
  const [overlapKept, setOverlapKept] = useState<number | null>(null);
  // 「項目を追加」で出した項目（値が無くても出しておく）と、その選択肢の表示。
  const [shown, setShown] = useState<Set<OptionalSection>>(new Set());
  const [addMenu, setAddMenu] = useState(false);
  // この連絡先に保存済みの会社名（小文字）。保存済みの名前は保存しても組織カードにならない。
  const [savedOrgNames, setSavedOrgNames] = useState<Set<string>>(new Set());
  // 開いている連絡先がつながっているサービス（見出しの印。新規は空＝Rondine のみ）。
  const [links, setLinks] = useState<ContactLink[]>([]);

  const loadTags = useCallback(() => {
    if (!isTauri) return;
    tagList()
      .then((ts) => setTagNames(ts.map((tg) => tg.name)))
      .catch(() => undefined);
  }, []);
  useEffect(loadTags, [loadTags]);

  // 所属組織のカードを取り込む（組織を選び直したら追従。未所属・新規組織なら消す）。
  const orgId = draft ? primaryOrganization(draft).org_id : null;
  useEffect(() => {
    if (!isTauri || orgId == null) {
      setOrg(null);
      return;
    }
    let alive = true;
    organizationGet(orgId)
      .then((o) => alive && setOrg(o))
      .catch(() => alive && setOrg(null));
    return () => {
      alive = false;
    };
  }, [orgId]);

  const openDraft = (d: ContactInput) => {
    setDraft(d);
    setBaseline(JSON.stringify(d));
    setSavedOrgNames(
      new Set(
        d.id === null
          ? []
          : d.organizations.map((o) => (o.name ?? '').trim().toLowerCase()).filter(Boolean),
      ),
    );
  };

  // request（何を開くか）に沿って下書きを作り直す。
  useEffect(() => {
    setSaved(false);
    setMatches([]);
    setConfirmDup(false);
    setConfirmDelete(false);
    setEditOrg(false);
    setOverlapKept(null);
    setShown(new Set());
    setAddMenu(false);
    setLinks(request?.kind === 'existing' ? (request.seed?.links ?? []) : []);
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
        if (!alive) return;
        openDraft(toDraft(full));
        setLinks(full.links);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
    // openDraft は安定。request の変化だけをトリガにする。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [request]);

  // 個人の値のうち、所属組織のカードと同じもの（代表電話・FAX・代表メール・所在地）。
  const overlap = useMemo(() => {
    if (!draft || !org || org.id !== orgId || overlapKept === org.id) return null;
    const region = getPhoneRegion() as CountryCode;
    const o = findOrgOverlap(draft, org, (v) => toE164(v, region));
    return hasOrgOverlap(o) ? o : null;
  }, [draft, org, orgId, overlapKept]);

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

  // 姓・名の入力に表示名を追従させる。保存には表示名が要るので、姓・名だけ埋めた
  // 状態で保存できなくなるのを防ぐ。表示名を手で書き換えた後は追従しない。
  const patchName = (p: Pick<Partial<ContactInput>, 'family_name' | 'given_name'>) => {
    setDraft((d) => {
      if (!d) return d;
      const next = { ...d, ...p };
      const auto = joinPersonName(d.family_name, d.given_name);
      if (d.display_name.trim() === '' || d.display_name === auto) {
        next.display_name = joinPersonName(next.family_name, next.given_name);
      }
      return next;
    });
    setSaved(false);
  };

  // 空文字は NULL に寄せてから送る（検索・並び替えの一貫性のため）。
  const nullify = (s: string) => (s.trim() === '' ? null : s);

  const doSave = async () => {
    if (!draft || draft.display_name.trim() === '') return;
    setConfirmDup(false);
    try {
      const result = await contactUpsert(normalizeForSave(draft));
      setSaved(true);
      openDraft(toDraft(result));
      setLinks(result.links);
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

  // 削除は画面内で確認してから（window.confirm は Linux で素通りする）。つながっている
  // サービスがあれば、そちらの連絡先も消えることを添える（次の同期で削除が送られる）。
  const remove = async (id: number) => {
    setDeleting(true);
    try {
      await contactDelete(id);
      setConfirmDelete(false);
      onDeleted?.(id);
    } catch {
      /* noop */
    } finally {
      setDeleting(false);
    }
  };
  const deleteNotes = [...new Set(links.map((l) => l.provider))].map((p) =>
    t('contact.deleteAlsoRemote', { service: t(`contact.link.${p}`) }),
  );

  // 重複バナー: 親が「整理」誘導を持てばそこへ、無ければその連絡先を開く。
  const handleReview = (m: ContactMatch) => {
    setConfirmDup(false);
    if (onReviewDuplicate) onReviewDuplicate(m);
    else onOpenContact?.(m.id);
  };

  // 畳んでいる項目の出し入れ。
  const visible = (sec: OptionalSection) =>
    !!draft && (shown.has(sec) || sectionHasData(draft, sec));
  const hidden = OPTIONAL_SECTIONS.filter((sec) => !visible(sec));
  const reveal = (sec: OptionalSection) => {
    setShown((prev) => new Set(prev).add(sec));
    if (draft) patch(withFirstRow(draft, sec));
    setAddMenu(false);
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
      <LabelDatalists />
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
            title={draft.display_name.trim() === '' ? t('contact.nameRequired') : t('contact.save')}
            aria-label={t('contact.save')}
            className="flex h-9 shrink-0 items-center gap-1.5 rounded-full bg-white/20 px-3.5 text-sm font-medium hover:bg-white/30 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Save size={16} />
            {t('contact.save')}
          </button>
          {draft.id !== null && (
            <button
              onClick={() => setConfirmDelete(true)}
              title={t('contact.delete')}
              aria-label={t('contact.delete')}
              className="flex h-9 w-9 items-center justify-center rounded-full border border-white/20 text-white/60 hover:border-red-400/60 hover:bg-red-500/30 hover:text-white"
            >
              <Trash2 size={17} />
            </button>
          )}
        </div>

        {/* どのサービスと同期しているか（アカウント名つき）。 */}
        <div className="-mt-3 mb-4 pl-11">
          <ContactLinkChips links={links} />
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
          {visible('nameDetails') ? (
            <>
              <Field icon={<User size={15} />} label={t('contact.nameLabelFull')}>
                <div className="flex gap-2">
                  <input
                    className={`${INPUT} w-16 shrink-0`}
                    placeholder={t('contact.namePrefix')}
                    value={draft.name_prefix ?? ''}
                    onChange={(e) => patch({ name_prefix: nullify(e.target.value) })}
                  />
                  <input
                    className={`${INPUT} min-w-0 flex-1`}
                    placeholder={t('contact.familyName')}
                    value={draft.family_name ?? ''}
                    onChange={(e) => patchName({ family_name: nullify(e.target.value) })}
                  />
                  <input
                    className={`${INPUT} min-w-0 flex-1`}
                    placeholder={t('contact.middleName')}
                    value={draft.middle_name ?? ''}
                    onChange={(e) => patch({ middle_name: nullify(e.target.value) })}
                  />
                  <input
                    className={`${INPUT} min-w-0 flex-1`}
                    placeholder={t('contact.givenName')}
                    value={draft.given_name ?? ''}
                    onChange={(e) => patchName({ given_name: nullify(e.target.value) })}
                  />
                  <input
                    className={`${INPUT} w-16 shrink-0`}
                    placeholder={t('contact.nameSuffix')}
                    value={draft.name_suffix ?? ''}
                    onChange={(e) => patch({ name_suffix: nullify(e.target.value) })}
                  />
                </div>
              </Field>
              <Field icon={<User size={15} />} label={t('contact.phoneticLabelFull')}>
                <div className="flex gap-2">
                  <input
                    className={`${INPUT} min-w-0 flex-1`}
                    placeholder={t('contact.familyName')}
                    value={draft.phonetic_family ?? ''}
                    onChange={(e) => patch({ phonetic_family: nullify(e.target.value) })}
                  />
                  <input
                    className={`${INPUT} min-w-0 flex-1`}
                    placeholder={t('contact.middleName')}
                    value={draft.phonetic_middle ?? ''}
                    onChange={(e) => patch({ phonetic_middle: nullify(e.target.value) })}
                  />
                  <input
                    className={`${INPUT} min-w-0 flex-1`}
                    placeholder={t('contact.givenName')}
                    value={draft.phonetic_given ?? ''}
                    onChange={(e) => patch({ phonetic_given: nullify(e.target.value) })}
                  />
                </div>
              </Field>
            </>
          ) : (
            <>
              <Field icon={<User size={15} />} label={t('contact.nameLabel')}>
                <div className="flex gap-2">
                  <input
                    className={INPUT_FULL}
                    placeholder={t('contact.familyName')}
                    value={draft.family_name ?? ''}
                    onChange={(e) => patchName({ family_name: nullify(e.target.value) })}
                  />
                  <input
                    className={INPUT_FULL}
                    placeholder={t('contact.givenName')}
                    value={draft.given_name ?? ''}
                    onChange={(e) => patchName({ given_name: nullify(e.target.value) })}
                  />
                </div>
              </Field>
              <Field icon={<User size={15} />} label={t('contact.phoneticLabel')}>
                <div className="flex gap-2">
                  <input
                    className={INPUT_FULL}
                    placeholder={t('contact.familyName')}
                    value={draft.phonetic_family ?? ''}
                    onChange={(e) => patch({ phonetic_family: nullify(e.target.value) })}
                  />
                  <input
                    className={INPUT_FULL}
                    placeholder={t('contact.givenName')}
                    value={draft.phonetic_given ?? ''}
                    onChange={(e) => patch({ phonetic_given: nullify(e.target.value) })}
                  />
                </div>
              </Field>
            </>
          )}
          {(visible('nickname') || visible('maidenName')) && (
            <div className="flex gap-2">
              {visible('nickname') && (
                <Field icon={<Smile size={15} />} label={t('contact.nickname')}>
                  <input
                    className={INPUT_FULL}
                    value={draft.nickname ?? ''}
                    onChange={(e) => patch({ nickname: nullify(e.target.value) })}
                  />
                </Field>
              )}
              {visible('maidenName') && (
                <Field icon={<UserRound size={15} />} label={t('contact.maidenName')}>
                  <input
                    className={INPUT_FULL}
                    value={draft.maiden_name ?? ''}
                    onChange={(e) => patch({ maiden_name: nullify(e.target.value) })}
                  />
                </Field>
              )}
            </div>
          )}
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
          <OrgRows
            organizations={draft.organizations}
            onChange={(organizations) => patch({ organizations })}
            savedNames={savedOrgNames}
            showPhonetic={visible('nameDetails')}
          >
            {/* 会社共通の情報（代表電話・FAX・代表メール・URL・所在地）はラベル表示。
                変更は所属する全員に効くので、［編集］で組織カードを開いて行う。 */}
            {org && org.id === orgId && (
              <OrgCardInfo org={org} onEdit={() => setEditOrg(true)} />
            )}
            {org && overlap && (
              <OrgOverlapNotice
                org={org}
                overlap={overlap}
                onMerge={() => {
                  setDraft((d) => (d ? mergeOrgOverlap(d, overlap) : d));
                  setSaved(false);
                }}
                onKeep={() => setOverlapKept(org.id)}
              />
            )}
          </OrgRows>
          <AddressRows
            icon={<MapPin size={14} />}
            label={t('contact.address')}
            addresses={draft.addresses}
            onChange={(addresses) => patch({ addresses })}
          />
          <Field icon={<Cake size={15} />} label={t('contact.birthday')}>
            <input
              type="date"
              className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
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
          {visible('urls') && (
            <PairRows
              icon={<Globe size={14} />}
              label={t('contact.urls')}
              items={draft.urls}
              onChange={(urls) => patch({ urls })}
              empty={() => ({ label: null, value: '' })}
              nullable={['label']}
              columns={[
                {
                  key: 'label',
                  placeholder: t('contact.labelPlaceholder'),
                  list: LABEL_LIST_IDS.url,
                  width: 'w-24 shrink-0 text-xs',
                },
                { key: 'value', placeholder: 'https://' },
              ]}
            />
          )}
          {visible('dates') && (
            <PairRows
              icon={<CalendarHeart size={14} />}
              label={t('contact.dates')}
              items={draft.dates}
              onChange={(dates) => patch({ dates })}
              empty={() => ({ label: null, date: '' })}
              nullable={['label']}
              columns={[
                {
                  key: 'label',
                  placeholder: t('contact.labelPlaceholder'),
                  list: LABEL_LIST_IDS.date,
                  width: 'w-24 shrink-0 text-xs',
                },
                {
                  key: 'date',
                  placeholder: t('contact.datePlaceholder'),
                  invalid: (v) => !isContactDate(v),
                },
              ]}
            />
          )}
          {visible('relations') && (
            <PairRows
              icon={<HeartHandshake size={14} />}
              label={t('contact.relations')}
              items={draft.relations}
              onChange={(relations) => patch({ relations })}
              empty={() => ({ label: null, name: '' })}
              nullable={['label']}
              columns={[
                {
                  key: 'label',
                  placeholder: t('contact.labelPlaceholder'),
                  list: LABEL_LIST_IDS.relation,
                  width: 'w-24 shrink-0 text-xs',
                },
                { key: 'name', placeholder: t('contact.relationName') },
              ]}
            />
          )}
          {visible('handles') && (
            <HandleRows
              icon={<MessageCircle size={14} />}
              label={t('contact.handles')}
              handles={draft.handles}
              onChange={(handles) => patch({ handles })}
            />
          )}
          {visible('customFields') && (
            <PairRows
              icon={<ListPlus size={14} />}
              label={t('contact.customFields')}
              items={draft.custom_fields}
              onChange={(custom_fields) => patch({ custom_fields })}
              empty={() => ({ key: '', value: '' })}
              nullable={[]}
              columns={[
                { key: 'key', placeholder: t('contact.customKey'), width: 'w-28 shrink-0' },
                { key: 'value', placeholder: t('contact.customValue') },
              ]}
            />
          )}

          {/* 普段使わない項目は畳んでおき、ここから出す（画面を長くしすぎない）。 */}
          {hidden.length > 0 && (
            <div>
              <button
                onClick={() => setAddMenu((v) => !v)}
                aria-expanded={addMenu}
                className="flex items-center gap-1 text-xs text-sky-300 hover:text-sky-200"
              >
                <Plus size={13} />
                {t('contact.addField')}
              </button>
              {addMenu && (
                <div className="mt-1.5 flex flex-wrap gap-1.5">
                  {hidden.map((sec) => (
                    <button
                      key={sec}
                      onClick={() => reveal(sec)}
                      className="rounded-full border border-white/20 px-2.5 py-1 text-xs text-white/75 hover:bg-white/10 hover:text-white"
                    >
                      {t(`contact.section.${sec}`)}
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}
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
            icon={<Building2 size={15} />}
            label={t('contact.showAsCompany')}
            hint={t('contact.showAsCompanyHint')}
            checked={draft.show_as_company}
            onChange={(v) => patch({ show_as_company: v })}
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
          {/* 保存できない理由を明示する（名前が空だと保存ボタンは無効） */}
          {draft.display_name.trim() === '' && (
            <span className="text-sm text-white/45">{t('contact.nameRequired')}</span>
          )}
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

      {confirmDelete && draft.id !== null && (
        <ConfirmDialog
          title={t('contact.deleteTitle')}
          body={t('contact.deleteConfirm', { name: draft.display_name || t('contact.untitled') })}
          notes={deleteNotes}
          confirmLabel={t('contact.delete')}
          danger
          busy={deleting}
          onConfirm={() => void remove(draft.id as number)}
          onCancel={() => setConfirmDelete(false)}
        />
      )}

      {/* 組織カードの編集（会社共通の情報なので、所属している全員に反映される）。 */}
      {editOrg && org && (
        <OrgCardDialog org={org} onClose={() => setEditOrg(false)} onSaved={setOrg} />
      )}
    </div>
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
