import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountSummary } from '@bindings/AccountSummary';
import { APP } from '../config/appIdentity';
import { AccountSetup } from './AccountSetup';
import { SignatureManager } from './SignatureManager';
import { TagManager } from './TagManager';

type Section = 'accounts' | 'signatures' | 'tags' | 'display' | 'about';

/**
 * 設定ページ: 左サイドバー（項目）＋右コンテンツの2カラム。
 */
export function Settings({
  accounts,
  onChanged,
}: {
  accounts: AccountSummary[];
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [section, setSection] = useState<Section>('accounts');

  const items: { key: Section; label: string }[] = [
    { key: 'accounts', label: t('settings.accounts') },
    { key: 'signatures', label: t('settings.signatures') },
    { key: 'tags', label: t('settings.tags') },
    { key: 'display', label: t('settings.display') },
    { key: 'about', label: t('settings.about') },
  ];

  return (
    <div className="grid h-full min-h-0 grid-cols-[200px_1fr] overflow-hidden">
      <nav className="min-h-0 space-y-1 overflow-y-auto border-r border-white/10 p-2">
        <div className="px-2 py-1 text-sm font-semibold text-white/80">{t('settings.title')}</div>
        {items.map((it) => (
          <button
            key={it.key}
            onClick={() => setSection(it.key)}
            className={`block w-full rounded-md px-3 py-2 text-left text-sm hover:bg-white/10 ${
              section === it.key ? 'bg-white/15 text-white' : 'text-white/70'
            }`}
          >
            {it.label}
          </button>
        ))}
      </nav>

      <div className="min-h-0 overflow-y-auto p-5">
        {section === 'accounts' && <AccountSetup accounts={accounts} onChanged={onChanged} />}
        {section === 'signatures' && <SignatureManager />}
        {section === 'tags' && <TagManager />}
        {section === 'display' && (
          <p className="text-sm text-white/60">表示設定は今後追加します。</p>
        )}
        {section === 'about' && (
          <div className="space-y-1 text-sm text-white/70">
            <div className="text-base font-semibold text-white">{APP.productName}</div>
            <div>{t('app.tagline')}</div>
            <div className="text-xs text-white/40">
              {APP.identifier} · {APP.channel}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
