import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Users } from 'lucide-react';
import type { ContactSummary } from '@bindings/ContactSummary';
import { organizationDetail } from '../services/organizations';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** その人の、この組織での役職・部署（無ければ空）。 */
function roleIn(c: ContactSummary, orgId: number): string {
  const o = c.organizations.find((x) => x.org_id === orgId);
  return [o?.department, o?.title]
    .map((v) => (v ?? '').trim())
    .filter(Boolean)
    .join(' ');
}

/**
 * 組織のメンバー（この組織につながっている人）。組織の連絡先の編集画面で、読み取り専用で出す。
 * つなぐ・外すは人の連絡先の会社欄と、組織タブの「整理」で行う（docs/ORG_CONTACTS.md §4-2）。
 */
export function OrgMembers({
  orgId,
  onOpenContact,
}: {
  orgId: number;
  onOpenContact?: (id: number) => void;
}) {
  const { t } = useTranslation();
  const [members, setMembers] = useState<ContactSummary[] | null>(null);

  useEffect(() => {
    if (!isTauri) return;
    let alive = true;
    organizationDetail(orgId)
      .then((d) => alive && setMembers(d.members))
      .catch(() => alive && setMembers([]));
    return () => {
      alive = false;
    };
  }, [orgId]);

  if (members === null) return null;
  return (
    <div className="mt-4 rounded-md border border-white/10 bg-white/5 px-3 py-2.5">
      <div className="mb-1 flex items-center gap-1.5 text-xs font-medium text-white/70">
        <Users size={14} className="shrink-0 text-white/45" />
        {t('org.members', { count: members.length })}
      </div>
      {members.length === 0 ? (
        <p className="px-1 py-1 text-xs text-white/40">{t('org.noMembers')}</p>
      ) : (
        <ul className="space-y-0.5">
          {members.map((m) => (
            <li key={m.id}>
              <button
                onClick={() => onOpenContact?.(m.id)}
                disabled={!onOpenContact}
                className="flex w-full items-baseline gap-2 rounded px-1.5 py-1 text-left text-sm hover:bg-white/10 disabled:hover:bg-transparent"
              >
                <span className="min-w-0 flex-1 truncate">{m.display_name}</span>
                <span className="shrink-0 truncate text-xs text-white/45">{roleIn(m, orgId)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
