import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowLeft,
  Briefcase,
  Building2,
  Cake,
  Check,
  ImageOff,
  Mail,
  MapPin,
  Merge,
  Phone,
  RefreshCw,
  Save,
  Star,
  StickyNote,
  User,
  UserX,
} from 'lucide-react';
import type { DuplicateGroup } from '@bindings/DuplicateGroup';
import type { ContactSummary } from '@bindings/ContactSummary';
import type { ContactInput } from '@bindings/ContactInput';
import type { ContactValueInput } from '@bindings/ContactValueInput';
import type { ContactAddressInput } from '@bindings/ContactAddressInput';
import {
  contactGet,
  contactMerge,
  contactUpsert,
  contactFindDuplicates,
} from '../services/contacts';
import { AddressRows, PhoneRows, ValueRows, addressToFlat } from './ContactValueEditor';
import { displayPhone } from '../utils/phone';
import { getPhoneRegion, getPhoneStyle } from '../config/prefs';
import type { CountryCode } from 'libphonenumber-js';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 重複整理（2ペイン）。左＝グループ一覧、右＝候補リスト→[統合する]→統合後の正本カード。
 * 候補を「含める」で選び、[統合する]で全項目を持つ正本カードを生成。編集して「正本として保存」すると、
 * 選んだ連絡先が 1 件にまとまる（他項目・追加メールは保持、編集内容で確定）。
 * どれを残すかは自動決定（代表＝表示名の最多一致→情報量→先頭）。混乱を避け手動選択なし。
 * 含めなかった連絡先はそのまま残る。自動融合はしない（保存で確定）。
 */
export function ContactDuplicates({
  onMerged,
  onExit,
}: {
  onMerged: () => void;
  onExit: () => void;
}) {
  const { t } = useTranslation();
  const [groups, setGroups] = useState<DuplicateGroup[]>([]);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState(0);
  const [included, setIncluded] = useState<Set<number>>(new Set());
  const [draft, setDraft] = useState<ContactInput | null>(null); // 統合後の正本（編集可）
  const [busy, setBusy] = useState(false);

  const load = () => {
    if (!isTauri) return;
    setLoading(true);
    contactFindDuplicates()
      .then((g) => {
        setGroups(g);
        setSelected(0);
      })
      .catch(() => undefined)
      .finally(() => setLoading(false));
  };
  useEffect(load, []);

  const group: DuplicateGroup | undefined = groups[selected];

  // グループを選び直したら全員を含める（正本カードは上の効果で自動再生成される）。
  useEffect(() => {
    setIncluded(group ? new Set(group.contacts.map((c) => c.id)) : new Set());
  }, [group]);

  const includedMembers = useMemo(
    () => (group ? group.contacts.filter((c) => included.has(c.id)) : []),
    [group, included],
  );
  const representative = useMemo(() => pickRepresentative(includedMembers), [includedMembers]);
  const includedCount = included.size;

  // 2件以上選ばれていれば統合後の正本カードを自動生成。各メンバーの全項目（複数値）を
  // 取得して束ねる（一覧は軽量で複数値が空のため詳細を取り直す）。選択が変わると作り直す。
  useEffect(() => {
    if (!representative || includedMembers.length < 2) {
      setDraft(null);
      return;
    }
    let alive = true;
    Promise.all(includedMembers.map((m) => contactGet(m.id).catch(() => null)))
      .then((full) => {
        if (!alive) return;
        const members = full.filter((m): m is ContactSummary => m !== null);
        if (members.length < 2) {
          setDraft(buildDraft(includedMembers, representative));
          return;
        }
        const rep = members.find((m) => m.id === representative.id) ?? members[0];
        setDraft(buildDraft(members, rep));
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [includedMembers, representative]);

  const toggleInclude = (id: number) => {
    setIncluded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const dropCurrent = () => {
    setGroups((prev) => prev.filter((_, i) => i !== selected));
    setSelected((i) => Math.max(0, Math.min(i, groups.length - 2)));
  };

  const patch = (p: Partial<ContactInput>) => setDraft((d) => (d ? { ...d, ...p } : d));
  const nullify = (s: string) => (s.trim() === '' ? null : s);

  // [正本として保存]: 代表へ統合（追加メール等を保持）→ 編集内容で確定。
  const saveMaster = async () => {
    if (!draft || !representative || busy || draft.display_name.trim() === '') return;
    const dropIds = includedMembers.map((c) => c.id).filter((id) => id !== representative.id);
    setBusy(true);
    try {
      if (dropIds.length > 0) {
        await contactMerge(representative.id, dropIds);
      }
      await contactUpsert({ ...draft, id: representative.id });
      dropCurrent();
      onMerged();
    } catch {
      /* noop */
    } finally {
      setBusy(false);
    }
  };

  const totalMergeable = groups.reduce((n, g) => n + g.contacts.length - 1, 0);

  return (
    <div className="flex h-full min-h-0">
      {/* 左：グループ一覧 */}
      <aside className="flex w-72 shrink-0 flex-col border-r border-white/10">
        <div className="flex items-center gap-2 p-3">
          <button
            onClick={onExit}
            title={t('dupes.back')}
            aria-label={t('dupes.back')}
            className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white"
          >
            <ArrowLeft size={17} />
          </button>
          <div className="min-w-0 flex-1">
            <div className="text-sm font-semibold">{t('dupes.title')}</div>
            <div className="truncate text-xs text-white/45">
              {loading
                ? t('dupes.scanning')
                : groups.length === 0
                  ? t('dupes.none')
                  : t('dupes.summary', { groups: groups.length, extra: totalMergeable })}
            </div>
          </div>
          <button
            onClick={load}
            disabled={loading}
            title={t('dupes.rescan')}
            aria-label={t('dupes.rescan')}
            className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 disabled:opacity-40"
          >
            <RefreshCw size={16} className={loading ? 'animate-spin' : ''} />
          </button>
        </div>

        <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {groups.map((g, i) => (
            <li key={g.contacts[0].id}>
              <button
                onClick={() => setSelected(i)}
                className={`flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left ${
                  i === selected ? 'bg-white/20' : 'hover:bg-white/10'
                }`}
              >
                <ConfidenceBadge confidence={g.confidence} />
                <span className="min-w-0 flex-1 truncate text-sm">{g.label}</span>
                <span className="shrink-0 text-xs text-white/40">
                  {t('dupes.count', { count: g.contacts.length })}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </aside>

      {/* 右：候補リスト → 統合 → 正本カード */}
      <section className="min-h-0 flex-1 overflow-y-auto">
        {!group ? (
          <div className="flex h-full flex-col items-center justify-center gap-1 text-center">
            <User size={40} className="text-white/25" />
            <p className="text-sm text-white/45">
              {groups.length === 0 ? t('dupes.none') : t('dupes.pickGroup')}
            </p>
          </div>
        ) : (
          <div className="mx-auto max-w-2xl p-6">
            <div className="mb-1 flex items-center gap-2">
              <ConfidenceBadge confidence={group.confidence} />
              <h2 className="text-lg font-semibold">{group.label}</h2>
              <span className="text-sm text-white/40">
                {t('dupes.count', { count: group.contacts.length })}
              </span>
            </div>
            <p className="mb-4 text-xs text-white/45">{t('dupes.pickMembers')}</p>

            {/* 候補リスト */}
            <ul className="space-y-2">
              {group.contacts.map((c) => {
                const inc = included.has(c.id);
                return (
                  <li
                    key={c.id}
                    className={`rounded-lg border px-3 py-2.5 ${
                      inc ? 'border-white/15 bg-white/5' : 'border-white/10 bg-transparent opacity-45'
                    }`}
                  >
                    <button
                      onClick={() => toggleInclude(c.id)}
                      className="flex w-full items-center gap-2.5 text-left"
                    >
                      <span
                        className={`flex h-5 w-5 shrink-0 items-center justify-center rounded ${
                          inc ? 'bg-sky-500 text-white' : 'border border-white/30'
                        }`}
                      >
                        {inc && <Check size={13} />}
                      </span>
                      <span className="min-w-0 flex-1 truncate text-sm font-medium">
                        {c.display_name}
                        {c.organization && (
                          <span className="font-normal text-white/40"> · {c.organization}</span>
                        )}
                      </span>
                    </button>
                    <div className="mt-1.5 flex min-w-0 flex-wrap gap-x-4 gap-y-1 pl-7 text-xs text-white/55">
                      <DetailChip icon={<User size={12} />} value={c.name_kana} />
                      <DetailChip icon={<Mail size={12} />} value={c.email} />
                      <DetailChip
                        icon={<Phone size={12} />}
                        value={
                          c.phone
                            ? displayPhone(c.phone, getPhoneStyle(), getPhoneRegion() as CountryCode)
                            : null
                        }
                      />
                      <DetailChip icon={<Building2 size={12} />} value={c.organization} />
                      <DetailChip icon={<MapPin size={12} />} value={c.address} />
                      <DetailChip icon={<Cake size={12} />} value={c.birthday} />
                      <DetailChip icon={<StickyNote size={12} />} value={c.note} />
                    </div>
                  </li>
                );
              })}
            </ul>

            {/* 操作: 統合して保存／別人（統合しない）を正本パネルの前に 1 行で置く。
                長い編集フォームを下までスクロールしなくても実行できるようにするため。 */}
            <div className="mt-4 flex flex-wrap items-center gap-3">
              <button
                onClick={saveMaster}
                disabled={busy || !draft || draft.display_name.trim() === ''}
                className="flex items-center gap-1.5 rounded-md bg-emerald-500/80 px-4 py-2 text-sm font-medium text-white hover:bg-emerald-500 disabled:cursor-not-allowed disabled:opacity-40"
              >
                <Save size={15} />
                {t('dupes.saveMaster')}
              </button>
              <button
                onClick={dropCurrent}
                className="flex items-center gap-1.5 rounded-md border border-white/20 px-3 py-2 text-sm text-white/70 hover:bg-white/10"
              >
                <UserX size={15} />
                {t('dupes.notDup')}
              </button>
              {includedCount < 2 && (
                <span className="text-xs text-white/40">{t('dupes.pickAtLeast2')}</span>
              )}
            </div>

            {/* 統合後の正本カード（全項目・編集可） */}
            {draft && (
              <div className="mt-5 rounded-lg border border-emerald-400/30 bg-emerald-400/5 p-4">
                <div className="mb-3 flex items-center gap-2">
                  <Merge size={15} className="text-emerald-300" />
                  <span className="text-sm font-semibold text-emerald-200">
                    {t('dupes.master', { count: includedCount })}
                  </span>
                </div>

                <div className="space-y-2.5">
                  <EditField icon={<User size={14} />} label={t('contact.namePlaceholder')}>
                    <input
                      className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm font-medium outline-none focus:bg-white/15"
                      value={draft.display_name}
                      onChange={(e) => patch({ display_name: e.target.value })}
                    />
                  </EditField>
                  <EditField icon={<User size={14} />} label={t('contact.nameLabel')}>
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
                  </EditField>
                  <EditField icon={<User size={14} />} label={t('contact.phoneticLabel')}>
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
                  </EditField>
                  <ValueRows
                    icon={<Mail size={13} />}
                    label={t('contact.email')}
                    inputType="email"
                    values={draft.emails}
                    onChange={(emails) => patch({ emails })}
                  />
                  <PhoneRows
                    icon={<Phone size={13} />}
                    label={t('contact.phone')}
                    values={draft.phones}
                    onChange={(phones) => patch({ phones })}
                  />
                  <EditField icon={<Building2 size={14} />} label={t('contact.organization')}>
                    <input
                      className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                      value={draft.organization ?? ''}
                      onChange={(e) => patch({ organization: nullify(e.target.value) })}
                    />
                  </EditField>
                  <AddressRows
                    icon={<MapPin size={13} />}
                    label={t('contact.address')}
                    addresses={draft.addresses}
                    onChange={(addresses) => patch({ addresses })}
                  />
                  <EditField icon={<Cake size={14} />} label={t('contact.birthday')}>
                    <input
                      type="date"
                      className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15 [color-scheme:dark]"
                      value={draft.birthday ?? ''}
                      onChange={(e) => patch({ birthday: nullify(e.target.value) })}
                    />
                  </EditField>
                  <EditField icon={<StickyNote size={14} />} label={t('contact.note')}>
                    <textarea
                      rows={2}
                      className="w-full resize-y rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                      value={draft.note ?? ''}
                      onChange={(e) => patch({ note: nullify(e.target.value) })}
                    />
                  </EditField>

                  <div className="flex flex-wrap gap-2 pt-1">
                    <FlagToggle
                      icon={<Star size={13} />}
                      label={t('contact.favorite')}
                      on={draft.is_favorite}
                      onClick={() => patch({ is_favorite: !draft.is_favorite })}
                    />
                    <FlagToggle
                      icon={<Briefcase size={13} />}
                      label={t('contact.business')}
                      on={draft.is_business}
                      onClick={() => patch({ is_business: !draft.is_business })}
                    />
                    <FlagToggle
                      icon={<ImageOff size={13} />}
                      label={t('contact.allowRemoteImages')}
                      on={draft.allow_remote_images}
                      onClick={() => patch({ allow_remote_images: !draft.allow_remote_images })}
                    />
                  </div>
                </div>
                {/* 保存/統合ボタンは候補リスト直後（正本パネルの上）へ移動した。 */}
              </div>
            )}
          </div>
        )}
      </section>
    </div>
  );
}

function EditField({
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
      <span className="mb-0.5 flex items-center gap-1.5 text-[11px] text-white/50">
        {icon}
        {label}
      </span>
      {children}
    </label>
  );
}

function FlagToggle({
  icon,
  label,
  on,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  on: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-1.5 rounded-full px-3 py-1 text-xs ${
        on ? 'bg-emerald-400/80 text-black' : 'border border-white/20 text-white/60 hover:bg-white/10'
      }`}
    >
      {icon}
      {label}
    </button>
  );
}

/** 非空の項目だけをアイコン付きで表示。 */
function DetailChip({ icon, value }: { icon: React.ReactNode; value: string | null }) {
  if (!value || !value.trim()) return null;
  return (
    <span className="flex min-w-0 max-w-full items-start gap-1">
      <span className="mt-0.5 shrink-0 text-white/35">{icon}</span>
      <span className="min-w-0 break-words">{value.replace(/\n/g, ' ')}</span>
    </span>
  );
}

/** 確信度バッジ（high=緑 / medium=琥珀 / low=灰）。 */
function ConfidenceBadge({ confidence }: { confidence: string }) {
  const { t } = useTranslation();
  const style =
    confidence === 'high'
      ? 'bg-emerald-400/20 text-emerald-200'
      : confidence === 'medium'
        ? 'bg-amber-400/20 text-amber-200'
        : 'bg-white/10 text-white/50';
  return (
    <span className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-semibold ${style}`}>
      {t(`dupes.conf.${confidence}`)}
    </span>
  );
}

/** 情報の非空項目数。 */
function fieldCount(c: ContactSummary): number {
  return [c.name_kana, c.email, c.phone, c.organization, c.address, c.birthday, c.note].filter(
    (v) => v && v.trim(),
  ).length;
}

/** 代表（統合後の主）を選ぶ: 表示名の最多一致 → 情報量 → 先頭（安定ソート）。 */
function pickRepresentative(members: ContactSummary[]): ContactSummary | null {
  if (members.length === 0) return null;
  const freq = new Map<string, number>();
  for (const m of members) freq.set(m.display_name, (freq.get(m.display_name) ?? 0) + 1);
  return [...members].sort(
    (a, b) =>
      (freq.get(b.display_name) ?? 0) - (freq.get(a.display_name) ?? 0) ||
      fieldCount(b) - fieldCount(a),
  )[0];
}

/** 統合後の正本の下書きを作る（代表優先・空欄は他から補完、フラグは OR）。 */
function buildDraft(members: ContactSummary[], representative: ContactSummary): ContactInput {
  const ordered = [representative, ...members.filter((m) => m.id !== representative.id)];
  const pick = (get: (c: ContactSummary) => string | null): string | null => {
    for (const m of ordered) {
      const v = get(m);
      if (v && v.trim()) return v;
    }
    return null;
  };
  // 全メンバーのメール/電話/住所を値で重複排除して統合（代表を先頭に）。
  const emails: ContactValueInput[] = [];
  const phones: ContactValueInput[] = [];
  const addresses: ContactAddressInput[] = [];
  for (const m of ordered) {
    for (const e of m.emails) {
      if (e.value && !emails.some((x) => x.value.toLowerCase() === e.value.toLowerCase())) {
        emails.push({ label: e.label, value: e.value });
      }
    }
    for (const p of m.phones) {
      if (p.value && !phones.some((x) => x.value === p.value)) {
        phones.push({ label: p.label, value: p.value });
      }
    }
    for (const a of m.addresses) {
      const key = [a.postal, a.region, a.city, a.street].join('|');
      if (!addresses.some((x) => [x.postal, x.region, x.city, x.street].join('|') === key)) {
        addresses.push({
          label: a.label,
          postal: a.postal,
          region: a.region,
          city: a.city,
          street: a.street,
          extended: a.extended,
          country: a.country,
        });
      }
    }
  }
  return {
    id: null,
    display_name: representative.display_name,
    family_name: pick((c) => c.family_name),
    given_name: pick((c) => c.given_name),
    phonetic_family: pick((c) => c.phonetic_family),
    phonetic_given: pick((c) => c.phonetic_given),
    emails,
    phones,
    addresses,
    tags: [...new Set(ordered.flatMap((m) => m.tags))],
    name_kana: pick((c) => c.name_kana),
    email: emails[0]?.value ?? pick((c) => c.email),
    phone: phones[0]?.value ?? pick((c) => c.phone),
    organization: pick((c) => c.organization),
    org_title: pick((c) => c.org_title),
    org_department: pick((c) => c.org_department),
    address: addresses[0] ? addressToFlat(addresses[0]) || null : pick((c) => c.address),
    birthday: pick((c) => c.birthday),
    note: pick((c) => c.note),
    is_favorite: members.some((m) => m.is_favorite),
    is_business: members.some((m) => m.is_business),
    allow_remote_images: members.some((m) => m.allow_remote_images),
  };
}
