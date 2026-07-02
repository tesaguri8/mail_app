import { useCallback, useEffect, useState } from 'react';
import { TitleBar, type AppView } from './components/TitleBar';
import { BottomBar } from './components/BottomBar';
import { Home } from './components/Home';
import { MailboxView } from './components/MailboxView';
import { ContactsView } from './components/ContactsView';
import { StubView } from './components/StubView';
import { Settings } from './components/Settings';
import { accountList } from './services/accounts';
import { useAutoSync, MAIL_SYNCED_EVENT } from './hooks/useAutoSync';
import type { AccountSummary } from '@bindings/AccountSummary';
// アプリ同梱の背景画像（プレースホルダ。docs/UI_UX_DESIGN.md 背景写真システム）
import backgroundUrl from './assets/background.jpg';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export default function App() {
  const [view, setView] = useState<AppView>('home');
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  const [mailAccountId, setMailAccountId] = useState<number | null>(null);
  const [mailOpenId, setMailOpenId] = useState<number | null>(null);
  // 背景のかぶせ（暗さ）。写真によって文字が見づらい時に上げる。
  const [dim, setDim] = useState<number>(() => Number(localStorage.getItem('rondine.dim') ?? 0));
  useEffect(() => {
    localStorage.setItem('rondine.dim', String(dim));
  }, [dim]);

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

  // 自動同期: ホーム/メールに入った時＋滞在中は設定間隔（既定30秒）で全アカウント同期。
  const syncNow = useAutoSync(view === 'home' || view === 'mail', accounts);

  // 同期が完了したらアカウント（未読数バッジ）を更新する。
  useEffect(() => {
    window.addEventListener(MAIL_SYNCED_EVENT, refreshAccounts);
    return () => window.removeEventListener(MAIL_SYNCED_EVENT, refreshAccounts);
  }, [refreshAccounts]);

  const openMail = (accountId: number, mailId?: number) => {
    setMailAccountId(accountId);
    setMailOpenId(mailId ?? null);
    setView('mail');
  };

  // タイトルバーからの遷移。メールは特定メッセージを開かずに開く。
  // ホーム/メールは押すたびに同期（同じビューを再度押した時も含む）。
  const navigate = (v: AppView) => {
    if (v === 'mail') setMailOpenId(null);
    if ((v === 'home' || v === 'mail') && v === view) syncNow();
    setView(v);
  };

  return (
    <div
      className="flex h-full flex-col overflow-hidden bg-cover bg-center text-white"
      style={{
        backgroundImage: `linear-gradient(160deg, rgba(15,18,35,${(0.35 + dim).toFixed(2)}) 0%, rgba(8,12,28,${(0.55 + dim).toFixed(2)}) 100%), url(${backgroundUrl})`,
      }}
    >
      <TitleBar onNavigate={navigate} />

      <main className="min-h-0 flex-1 overflow-hidden">
        {view === 'home' && <Home accounts={accounts} onOpenMail={openMail} />}
        {view === 'mail' && (
          <MailboxView
            accounts={accounts}
            initialAccountId={mailAccountId}
            initialMailId={mailOpenId}
          />
        )}
        {view === 'contacts' && <ContactsView />}
        {view === 'calendar' && <StubView titleKey="nav.calendar" />}
        {view === 'settings' && <Settings accounts={accounts} onChanged={refreshAccounts} />}
      </main>

      <BottomBar dim={dim} onDimChange={setDim} />
    </div>
  );
}
