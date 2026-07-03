import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Building2, LeafyGreen, Users } from 'lucide-react';
import { ContactsView, type ContactPrefill } from './ContactsView';
import { OrganizationsView } from './OrganizationsView';
import { GreenDomainsView } from './GreenDomainsView';

/**
 * 住所録。「連絡先」と「組織」のタブを束ねる。
 * 連絡先タブ＝人の住所録、組織タブ＝会社・組織（所属・共有アドレス・組織名の編集）。
 */
export function AddressBook({
  prefill,
  onPrefillConsumed,
  openId,
  onOpenIdConsumed,
}: {
  prefill?: ContactPrefill | null;
  onPrefillConsumed?: () => void;
  /** メール等から既存連絡先を開く ID（外部指定）。 */
  openId?: number | null;
  onOpenIdConsumed?: () => void;
}) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<'contacts' | 'orgs' | 'green'>('contacts');
  // 組織タブの所属クリックから連絡先タブで開く連絡先 ID。
  const [orgOpenId, setOrgOpenId] = useState<number | null>(null);
  // 外部（メール）指定と組織由来のどちらかを ContactsView へ渡す。
  const effectiveOpenId = openId ?? orgOpenId;
  const consumeOpen = () => {
    setOrgOpenId(null);
    onOpenIdConsumed?.();
  };

  // メールからの＋追加（prefill）／既存を開く（openId）が来たら連絡先タブへ。
  useEffect(() => {
    if (prefill) setTab('contacts');
  }, [prefill]);
  useEffect(() => {
    if (openId != null) setTab('contacts');
  }, [openId]);

  const tabBtn = (key: 'contacts' | 'orgs' | 'green', icon: React.ReactNode, label: string) => (
    <button
      onClick={() => setTab(key)}
      className={`flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium ${
        tab === key ? 'bg-white/15 text-white' : 'text-white/55 hover:bg-white/10 hover:text-white/80'
      }`}
    >
      {icon}
      {label}
    </button>
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center gap-1.5 border-b border-white/10 px-3 py-1.5">
        {tabBtn('contacts', <Users size={15} />, t('org.tabContacts'))}
        {tabBtn('orgs', <Building2 size={15} />, t('org.tab'))}
        {tabBtn('green', <LeafyGreen size={15} />, t('green.tab'))}
      </div>
      <div className="min-h-0 flex-1">
        {tab === 'contacts' && (
          <ContactsView
            prefill={prefill}
            onPrefillConsumed={onPrefillConsumed}
            openId={effectiveOpenId}
            onOpenIdConsumed={consumeOpen}
          />
        )}
        {tab === 'orgs' && (
          <OrganizationsView
            onOpenContact={(id) => {
              setOrgOpenId(id);
              setTab('contacts');
            }}
          />
        )}
        {tab === 'green' && <GreenDomainsView />}
      </div>
    </div>
  );
}
