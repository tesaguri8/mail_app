import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Building2, Check, Plus } from 'lucide-react';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import { organizationList } from '../services/organizations';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 会社・組織のコンボボックス。既存を選べば org_id を確定（照合はID）、
 * 一覧に無い名前を入力すれば新規登録（保存時に組織を作成）扱いにする。
 */
export function OrgCombobox({
  orgId,
  name,
  onChange,
}: {
  orgId: number | null;
  name: string;
  onChange: (orgId: number | null, name: string) => void;
}) {
  const { t } = useTranslation();
  const [orgs, setOrgs] = useState<OrganizationSummary[]>([]);

  useEffect(() => {
    if (!isTauri) return;
    organizationList()
      .then(setOrgs)
      .catch(() => undefined);
  }, []);

  const trimmed = name.trim();
  const exact = useMemo(
    () => orgs.find((o) => o.name.toLowerCase() === trimmed.toLowerCase()) ?? null,
    [orgs, trimmed],
  );

  // 入力のたびに既存一致を判定して org_id を確定/解除する。
  const handle = (text: string) => {
    const key = text.trim().toLowerCase();
    const match = orgs.find((o) => o.name.toLowerCase() === key);
    onChange(match ? match.id : null, text);
  };

  const isExisting = orgId != null || exact != null;

  return (
    <div>
      <span className="mb-1 flex items-center gap-1.5 text-xs text-white/50">
        <Building2 size={15} />
        {t('contact.organization')}
      </span>
      <input
        className="w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
        list="org-options"
        value={name}
        placeholder={t('contact.orgPlaceholder')}
        onChange={(e) => handle(e.target.value)}
      />
      {trimmed !== '' && (
        <span
          className={`mt-1 flex items-center gap-1 text-[11px] ${
            isExisting ? 'text-emerald-300' : 'text-sky-300'
          }`}
        >
          {isExisting ? <Check size={12} /> : <Plus size={12} />}
          {isExisting
            ? exact
              ? t('contact.orgExistingN', { count: exact.member_count })
              : t('contact.orgExisting')
            : t('contact.orgNew')}
        </span>
      )}
      <datalist id="org-options">
        {orgs.map((o) => (
          <option key={o.id} value={o.name} />
        ))}
      </datalist>
    </div>
  );
}
