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
}: {
  prefill?: ContactPrefill | null;
  onPrefillConsumed?: () => void;
}) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<'contacts' | 'orgs' | 'green'>('contacts');
  // 組織タブの所属クリックから連絡先タブで開く連絡先 ID。
  const [contactOpenId, setContactOpenId] = useState<number | null>(null);

  // メールからの＋追加（prefill）が来たら連絡先タブへ。
  useEffect(() => {
    if (prefill) setTab('contacts');
  }, [prefill]);

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
            openId={contactOpenId}
            onOpenIdConsumed={() => setContactOpenId(null)}
          />
        )}
        {tab === 'orgs' && (
          <OrganizationsView
            onOpenContact={(id) => {
              setContactOpenId(id);
              setTab('contacts');
            }}
          />
        )}
        {tab === 'green' && <GreenDomainsView />}
      </div>
    </div>
  );
}
