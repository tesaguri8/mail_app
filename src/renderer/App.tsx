import { useCallback, useEffect, useState } from 'react';
import { TitleBar, type AppView } from './components/TitleBar';
import { Home } from './components/Home';
import { MailboxView } from './components/MailboxView';
import { Settings } from './components/Settings';
import { accountList } from './services/accounts';
import type { AccountSummary } from '@bindings/AccountSummary';
// アプリ同梱の背景画像（プレースホルダ。docs/UI_UX_DESIGN.md 背景写真システム）
import backgroundUrl from './assets/background.jpg';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export default function App() {
  const [view, setView] = useState<AppView>('home');
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  const [mailAccountId, setMailAccountId] = useState<number | null>(null);
  const [mailOpenId, setMailOpenId] = useState<number | null>(null);

  const refreshAccounts = useCallback(() => {
    if (!isTauri) return;
    accountList()
      .then(setAccounts)
      .catch(() => undefined);
  }, []);
  useEffect(refreshAccounts, [refreshAccounts]);

  // ホーム/設定へ戻るたびにアカウント（新着数）を更新
  useEffect(() => {
    if (view !== 'mail') refreshAccounts();
  }, [view, refreshAccounts]);

  const openMail = (accountId: number, mailId?: number) => {
    setMailAccountId(accountId);
    setMailOpenId(mailId ?? null);
    setView('mail');
  };

  return (
    <div
      className="flex h-full flex-col overflow-hidden bg-cover bg-center text-white"
      style={{
        backgroundImage: `linear-gradient(160deg, rgba(20,20,40,0.45) 0%, rgba(10,15,35,0.65) 100%), url(${backgroundUrl})`,
      }}
    >
      <TitleBar onNavigate={setView} />

      <main className="min-h-0 flex-1 overflow-hidden">
        {view === 'home' && <Home accounts={accounts} onOpenMail={openMail} />}
        {view === 'mail' && (
          <MailboxView
            accounts={accounts}
            initialAccountId={mailAccountId}
            initialMailId={mailOpenId}
          />
        )}
        {view === 'settings' && <Settings accounts={accounts} onChanged={refreshAccounts} />}
      </main>
    </div>
  );
}
