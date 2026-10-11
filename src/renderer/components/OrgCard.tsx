import { useTranslation } from 'react-i18next';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Building2, Combine, Globe, Mail, MapPin, Pencil, Phone, Printer } from 'lucide-react';
import type { CountryCode } from 'libphonenumber-js';
import type { OrgAddress } from '@bindings/OrgAddress';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import type { OrgOverlap } from '../utils/orgOverlap';
import { displayPhone } from '../utils/phone';
import { formatPostal } from '../utils/postal';
import { getPhoneRegion, getPhoneStyle, getPostalAutoformat } from '../config/prefs';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 所在地を 1 行の文字列へ（郵便番号は設定に応じて整形。連絡先の住所と同じ並び）。 */
function orgAddressToFlat(a: OrgAddress): string {
  const postalRegion = getPostalAutoformat() ? getPhoneRegion() : '';
  const postal = formatPostal(a.postal ?? '', postalRegion);
  return [postal, a.region, a.city, a.street, a.extended, a.country]
    .map((s) => (s ?? '').trim())
    .filter(Boolean)
    .join(' ');
}

/** 組織に代表の値が 1 つでも入っているか（ラベル表示の有無を決める）。 */
function hasOrgCardInfo(org: OrganizationSummary): boolean {
  return Boolean(
    (org.phone ?? '').trim() ||
    (org.fax ?? '').trim() ||
    (org.email ?? '').trim() ||
    (org.url ?? '').trim() ||
    orgAddressToFlat(org.address)
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
 * 組織の情報のラベル表示（読み取り専用）。人の連絡先では会社共通の情報を編集欄ではなく
 * ラベルで見せ、［編集］で組織の連絡先を開いて直す（組織の情報は連絡先の画面でだけ編集する /
 * docs/ORG_CONTACTS.md §4-2）。組織タブの詳細でも使う。
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
 * 人の連絡先に組織と同じ値があるときの確認（「組織に登録されています。統合しますか？」）。
 * 統合の中身は utils/orgOverlap の mergeOrgOverlap（電話・住所は外し、メールは共有の印を付ける）。
 */
export function OrgOverlapNotice({
  org,
  overlap,
  onMerge,
  onKeep,
}: {
  org: OrganizationSummary;
  overlap: OrgOverlap;
  onMerge: () => void;
  onKeep: () => void;
}) {
  const { t } = useTranslation();
  const kinds = [
    overlap.phones.length > 0 ? t('contact.phone') : null,
    overlap.addresses.length > 0 ? t('contact.address') : null,
    overlap.emails.length > 0 ? t('contact.email') : null,
  ]
    .filter(Boolean)
    .join('・');
  return (
    <div className="rounded-md border border-sky-300/30 bg-sky-300/10 px-3 py-2.5">
      <div className="flex items-center gap-1.5 text-xs font-medium text-sky-100">
        <Combine size={14} className="shrink-0" />
        {t('org.overlapTitle', { name: org.name, kinds })}
      </div>
      <p className="mt-1 text-[11px] leading-relaxed text-white/55">{t('org.overlapHint')}</p>
      <div className="mt-2 flex justify-end gap-2">
        <button
          onClick={onKeep}
          className="rounded-full px-3 py-1 text-xs text-white/60 hover:bg-white/10 hover:text-white"
        >
          {t('org.overlapKeep')}
        </button>
        <button
          onClick={onMerge}
          className="rounded-full bg-sky-500/80 px-3 py-1 text-xs text-white hover:bg-sky-500"
        >
          {t('org.overlapMerge')}
        </button>
      </div>
    </div>
  );
}
