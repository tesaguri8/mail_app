import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { openUrl } from '@tauri-apps/plugin-opener';
import {
  Building2,
  Globe,
  Mail,
  MapPin,
  Pencil,
  Phone,
  Printer,
  Save,
  StickyNote,
  X,
} from 'lucide-react';
import type { CountryCode } from 'libphonenumber-js';
import type { OrgAddress } from '@bindings/OrgAddress';
import type { OrganizationInput } from '@bindings/OrganizationInput';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import { organizationUpsert } from '../services/organizations';
import { Field } from './ContactValueEditor';
import { displayPhone, parseStored, toE164 } from '../utils/phone';
import { formatPostal } from '../utils/postal';
import { getPhoneRegion, getPhoneStyle, getPostalAutoformat } from '../config/prefs';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 空の所在地。 */
export const emptyOrgAddress = (): OrgAddress => ({
  postal: null,
  region: null,
  city: null,
  street: null,
  extended: null,
  country: null,
});

/** 組織カードの下書きを作る（既存の組織から、または新規用の空）。 */
export const orgDraft = (org?: OrganizationSummary | null): OrganizationInput =>
  org
    ? {
        id: org.id,
        name: org.name,
        name_kana: org.name_kana,
        note: org.note,
        phone: org.phone,
        fax: org.fax,
        email: org.email,
        url: org.url,
        address: { ...org.address },
      }
    : {
        id: null,
        name: '',
        name_kana: null,
        note: null,
        phone: null,
        fax: null,
        email: null,
        url: null,
        address: emptyOrgAddress(),
      };

/** 所在地を 1 行の文字列へ（郵便番号は設定に応じて整形。連絡先の住所と同じ並び）。 */
export function orgAddressToFlat(a: OrgAddress): string {
  const postalRegion = getPostalAutoformat() ? getPhoneRegion() : '';
  const postal = formatPostal(a.postal ?? '', postalRegion);
  return [postal, a.region, a.city, a.street, a.extended, a.country]
    .map((s) => (s ?? '').trim())
    .filter(Boolean)
    .join(' ');
}

/** 組織カードに代表連絡先が 1 つでも入っているか（ラベル表示の有無を決める）。 */
export function hasOrgCardInfo(org: OrganizationSummary): boolean {
  return Boolean(
    (org.phone ?? '').trim() ||
    (org.fax ?? '').trim() ||
    (org.email ?? '').trim() ||
    (org.url ?? '').trim() ||
    orgAddressToFlat(org.address)
  );
}

/** 空文字は null に寄せる（未入力の一貫性のため）。 */
const nullify = (s: string) => (s.trim() === '' ? null : s);

/** 電話/FAX の 1 行入力。表示は国内表記、保存は E.164 正準形（既定の国で解釈）。 */
function PhoneField({
  value,
  onChange,
}: {
  value: string | null;
  onChange: (v: string | null) => void;
}) {
  const region = getPhoneRegion() as CountryCode;
  const parsed = parseStored(value ?? '', region);
  return (
    <input
      type="tel"
      className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
      value={parsed.national}
      onChange={(e) => onChange(nullify(e.target.value))}
      onBlur={() => onChange(nullify(toE164(value ?? '', parsed.region)))}
    />
  );
}

/**
 * 組織カードの入力欄（代表電話・FAX・代表メール・URL・所在地・メモ）。
 * 住所録の「組織」タブと、連絡先から開く編集ダイアログで共有する。
 */
export function OrgCardFields({
  draft,
  onChange,
}: {
  draft: OrganizationInput;
  onChange: (next: OrganizationInput) => void;
}) {
  const { t } = useTranslation();
  const postalRegion = getPostalAutoformat() ? getPhoneRegion() : '';
  const a = draft.address;
  const setAddress = (patch: Partial<OrgAddress>) =>
    onChange({ ...draft, address: { ...a, ...patch } });
  const addressField = (key: keyof OrgAddress, ph: string, w = '') => (
    <input
      className={`rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15 ${w}`}
      placeholder={ph}
      value={a[key] ?? ''}
      onChange={(e) => setAddress({ [key]: nullify(e.target.value) })}
    />
  );
  return (
    <div className="space-y-3">
      <div className="flex gap-2">
        <Field icon={<Phone size={15} />} label={t('org.mainPhone')}>
          <PhoneField value={draft.phone} onChange={(phone) => onChange({ ...draft, phone })} />
        </Field>
        <Field icon={<Printer size={15} />} label={t('org.fax')}>
          <PhoneField value={draft.fax} onChange={(fax) => onChange({ ...draft, fax })} />
        </Field>
      </div>
      <Field icon={<Mail size={15} />} label={t('org.mainEmail')}>
        <input
          type="email"
          className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
          value={draft.email ?? ''}
          onChange={(e) => onChange({ ...draft, email: nullify(e.target.value) })}
        />
      </Field>
      <Field icon={<Globe size={15} />} label={t('org.url')}>
        <input
          type="url"
          className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
          placeholder="https://"
          value={draft.url ?? ''}
          onChange={(e) => onChange({ ...draft, url: nullify(e.target.value) })}
        />
      </Field>
      <div>
        <span className="mb-1 flex items-center gap-1.5 text-xs text-white/50">
          <MapPin size={15} />
          {t('org.location')}
        </span>
        <div className="grid grid-cols-2 gap-1.5">
          <input
            className="rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15"
            placeholder={t('contact.postal')}
            value={formatPostal(a.postal ?? '', postalRegion)}
            onChange={(e) => setAddress({ postal: nullify(e.target.value) })}
          />
          {addressField('region', t('contact.region'))}
          {addressField('city', t('contact.city'))}
          {addressField('street', t('contact.street'))}
          {addressField('extended', t('contact.extended'), 'col-span-2')}
          {addressField('country', t('contact.country'), 'col-span-2')}
        </div>
      </div>
      <Field icon={<StickyNote size={15} />} label={t('contact.note')}>
        <textarea
          rows={3}
          className="w-full resize-y rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
          value={draft.note ?? ''}
          onChange={(e) => onChange({ ...draft, note: nullify(e.target.value) })}
        />
      </Field>
    </div>
  );
}

/** ラベル表示の 1 行（値が空なら何も描かない）。 */
function InfoRow({
  icon,
  label,
  value,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  onClick?: () => void;
}) {
  if (!value) return null;
  return (
    <div className="flex items-start gap-2 px-1 py-1">
      <span className="mt-0.5 shrink-0 text-white/40">{icon}</span>
      <span className="w-20 shrink-0 text-xs text-white/45">{label}</span>
      {onClick ? (
        <button
          onClick={onClick}
          className="min-w-0 flex-1 break-all text-left text-sm text-sky-300 hover:text-sky-200 hover:underline"
        >
          {value}
        </button>
      ) : (
        <span className="min-w-0 flex-1 break-all text-sm">{value}</span>
      )}
    </div>
  );
}

/**
 * 組織カードのラベル表示（読み取り専用）。個人の連絡先では会社共通の情報を編集欄ではなく
 * ラベルで見せ、［編集］ボタンから組織カードを直接編集する。
 */
export function OrgCardInfo({ org, onEdit }: { org: OrganizationSummary; onEdit?: () => void }) {
  const { t } = useTranslation();
  const region = getPhoneRegion() as CountryCode;
  const style = getPhoneStyle();
  const address = orgAddressToFlat(org.address);
  const url = (org.url ?? '').trim();
  const openSite = () => {
    if (!url) return;
    const href = /^https?:\/\//i.test(url) ? url : `https://${url}`;
    if (isTauri) openUrl(href).catch(() => undefined);
    else window.open(href, '_blank', 'noopener,noreferrer');
  };
  return (
    <div className="rounded-md border border-white/10 bg-white/5 px-3 py-2.5">
      <div className="mb-1 flex items-center gap-2">
        <Building2 size={14} className="shrink-0 text-white/45" />
        <span className="min-w-0 flex-1 truncate text-sm font-medium">{org.name}</span>
        {onEdit && (
          <button
            onClick={onEdit}
            title={t('org.cardEdit')}
            className="flex shrink-0 items-center gap-1 rounded-full border border-white/20 px-2.5 py-1 text-xs text-white/70 hover:bg-white/10 hover:text-white"
          >
            <Pencil size={12} />
            {t('org.cardEditShort')}
          </button>
        )}
      </div>
      {hasOrgCardInfo(org) ? (
        <div className="mt-1">
          <InfoRow
            icon={<Phone size={13} />}
            label={t('org.mainPhone')}
            value={displayPhone(org.phone ?? '', style, region)}
          />
          <InfoRow
            icon={<Printer size={13} />}
            label={t('org.fax')}
            value={displayPhone(org.fax ?? '', style, region)}
          />
          <InfoRow
            icon={<Mail size={13} />}
            label={t('org.mainEmail')}
            value={(org.email ?? '').trim()}
          />
          <InfoRow icon={<Globe size={13} />} label={t('org.url')} value={url} onClick={openSite} />
          <InfoRow icon={<MapPin size={13} />} label={t('org.location')} value={address} />
        </div>
      ) : (
        <p className="px-1 py-1 text-xs text-white/40">{t('org.cardEmpty')}</p>
      )}
    </div>
  );
}

/**
 * 組織カードの編集ダイアログ（連絡先から開く）。会社共通の情報だけを編集し、
 * 会社名の変更・統合は「組織」タブに任せる（所属している全員に影響するため）。
 */
export function OrgCardDialog({
  org,
  onClose,
  onSaved,
}: {
  org: OrganizationSummary;
  onClose: () => void;
  onSaved: (org: OrganizationSummary) => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<OrganizationInput>(() => orgDraft(org));
  const [saving, setSaving] = useState(false);

  const save = async () => {
    if (!isTauri) return;
    setSaving(true);
    try {
      onSaved(await organizationUpsert(draft));
      onClose();
    } catch {
      /* noop */
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      onClick={onClose}
    >
      <div
        className="max-h-[85vh] w-full max-w-md overflow-y-auto rounded-lg border border-white/15 bg-[#141a2e] p-5 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-1 flex items-center gap-2">
          <Building2 size={18} className="shrink-0 text-white/50" />
          <h3 className="min-w-0 flex-1 truncate text-base font-semibold">{org.name}</h3>
          <button
            onClick={onClose}
            title={t('org.cancel')}
            aria-label={t('org.cancel')}
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-white/50 hover:bg-white/10 hover:text-white"
          >
            <X size={16} />
          </button>
        </div>
        <p className="mb-3 text-[11px] text-white/40">
          {t('org.cardShared', { count: org.member_count })}
        </p>

        <OrgCardFields draft={draft} onChange={setDraft} />

        <p className="mt-3 text-[11px] text-white/35">{t('org.cardNameHint')}</p>
        <div className="mt-3 flex justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded-md border border-white/20 px-3 py-1.5 text-sm text-white/70 hover:bg-white/10"
          >
            {t('org.cancel')}
          </button>
          <button
            onClick={save}
            disabled={saving}
            className="flex items-center gap-1.5 rounded-md bg-white/20 px-3 py-1.5 text-sm font-medium hover:bg-white/30 disabled:opacity-40"
          >
            <Save size={15} />
            {t('contact.save')}
          </button>
        </div>
      </div>
    </div>
  );
}
