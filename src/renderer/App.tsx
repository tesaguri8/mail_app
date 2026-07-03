import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import { TitleBar, type AppView } from './components/TitleBar';
import { BottomBar } from './components/BottomBar';
import { Home } from './components/Home';
import { MailboxView } from './components/MailboxView';
import { AddressBook } from './components/AddressBook';
import type { ContactPrefill } from './components/ContactsView';
import { StubView } from './components/StubView';
import { Settings } from './components/Settings';
import { accountList } from './services/accounts';
import { useAutoSync, MAIL_SYNCED_EVENT } from './hooks/useAutoSync';
import { SyncProvider } from './components/SyncProvider';
import type { AccountSummary } from '@bindings/AccountSummary';
// 背景写真プール（同梱サンプル）。docs/UI_UX_DESIGN.md 背景写真システム
import { BACKGROUNDS, getBackgroundIndex, setBackgroundIndex } from './config/backgrounds';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 背景の濃さ・文字色スライダーの共通最大値（85%）。 */
export const BAR_MAX = 0.85;

/** 文字色（インク）の白→黒スライダー(0〜0.85)を実際の色に。0=白, 0.85=黒。
 * これを Tailwind の --color-white に流し込み、UI 全体を白⇄黒でトーン反転させる。 */
function inkColor(ink: number): string {
  const l = Math.round(255 * (1 - Math.min(ink, BAR_MAX) / BAR_MAX));
  return `rgb(${l}, ${l}, ${l})`;
}

export default function App() {
  const [view, setView] = useState<AppView>('home');
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  const [mailAccountId, setMailAccountId] = useState<number | null>(null);
  const [mailOpenId, setMailOpenId] = useState<number | null>(null);
  // メール画面で現在表示中のアカウント選択（'all'=全て）。フッターの件数表示に使う。
  const [mailSel, setMailSel] = useState<number | 'all' | null>(null);
  // メールのアドレスから住所録へ追加するときの初期値（住所録の新規フォームに埋める）。
  const [contactPrefill, setContactPrefill] = useState<ContactPrefill | null>(null);
  // メールのアドレスから既存連絡先を開くときの ID（編集アイコン）。
  const [contactOpenId, setContactOpenId] = useState<number | null>(null);
  // 背景のかぶせ（暗さ）。写真によって文字が見づらい時に上げる。
  const [dim, setDim] = useState<number>(() => Number(localStorage.getItem('rondine.dim') ?? 0));
  useEffect(() => {
    localStorage.setItem('rondine.dim', String(dim));
  }, [dim]);
  // 文字色（インク）。0=白, 0.85=黒。UI 全体を白⇄黒でトーン反転させて明るい写真でも読める。
  const [ink, setInk] = useState<number>(() => Number(localStorage.getItem('rondine.ink') ?? 0));
  useEffect(() => {
    localStorage.setItem('rondine.ink', String(ink));
  }, [ink]);
  // ボトムバーのスライダーがどちらを操作するか（背景の濃さ / 文字色）。
  const [barMode, setBarMode] = useState<'backdrop' | 'ink'>(() =>
    localStorage.getItem('rondine.barMode') === 'ink' ? 'ink' : 'backdrop'
  );
  useEffect(() => {
    localStorage.setItem('rondine.barMode', barMode);
  }, [barMode]);
  // 背景写真の選択（同梱サンプルのプールから）。ボタンで次の候補へ切り替えて決める。
  const [bgIndex, setBgIndex] = useState<number>(getBackgroundIndex);
  const cycleBackground = () => {
    if (BACKGROUNDS.length === 0) return;
    const next = (bgIndex + 1) % BACKGROUNDS.length;
    setBgIndex(next);
    setBackgroundIndex(next);
  };
  const backgroundUrl = BACKGROUNDS[bgIndex] ?? BACKGROUNDS[0] ?? '';

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

  // メールのアドレスから住所録へ: 新規フォームに名前・メールを埋めて住所録へ切り替える。
  const addContactFromMail = (name: string | null, email: string) => {
    setContactPrefill({ name, email });
    setView('contacts');
  };

  // メールのアドレスから既存連絡先を開く（編集アイコン）。住所録へ切り替えてその連絡先を開く。
  const openContactFromMail = (id: number) => {
    setContactOpenId(id);
    setView('contacts');
  };

  // フッター左に出す「表示中アカウントのメール総数」（メールモード時のみ。'all'=全合計）。
  const mailTotal =
    view !== 'mail'
      ? null
      : mailSel === 'all'
        ? accounts.reduce((s, a) => s + a.total_count, 0)
        : (accounts.find((a) => a.id === mailSel)?.total_count ?? null);

  // タイトルバーからの遷移。メールは特定メッセージを開かずに開く。
  // ホーム/メールは押すたびに同期（同じビューを再度押した時も含む）。
  const navigate = (v: AppView) => {
    if (v === 'mail') setMailOpenId(null);
    if ((v === 'home' || v === 'mail') && v === view) syncNow();
    setView(v);
  };

  return (
    <SyncProvider>
      <div
        className="flex h-full flex-col overflow-hidden bg-cover bg-center text-white"
        style={
          {
            // 背景オーバーレイ: 既定でも白文字が読める最低限の暗さを土台にし、スライダー(dim)で
            // さらに濃くする。0% でも明るい写真＋白文字が真っ白にならないようにする。
            backgroundImage: `linear-gradient(160deg, rgba(10,14,28,${(0.35 + dim * 0.6).toFixed(2)}) 0%, rgba(6,9,20,${(0.55 + dim * 0.5).toFixed(2)}) 100%), url(${backgroundUrl})`,
            // 文字色スライダー: Tailwind の白を差し替えて UI 全体を白⇄黒でトーン反転。
            '--color-white': inkColor(ink),
          } as CSSProperties
        }
      >
        <TitleBar onNavigate={navigate} onCycleBackground={cycleBackground} />

        <main className="min-h-0 flex-1 overflow-hidden">
          {view === 'home' && <Home accounts={accounts} onOpenMail={openMail} />}
          {view === 'mail' && (
            <MailboxView
              accounts={accounts}
              initialAccountId={mailAccountId}
              initialMailId={mailOpenId}
              onAccountChange={setMailSel}
              onAddContact={addContactFromMail}
              onOpenContact={openContactFromMail}
            />
          )}
          {view === 'contacts' && (
            <AddressBook
              prefill={contactPrefill}
              onPrefillConsumed={() => setContactPrefill(null)}
              openId={contactOpenId}
              onOpenIdConsumed={() => setContactOpenId(null)}
            />
          )}
          {view === 'calendar' && <StubView titleKey="nav.calendar" />}
          {view === 'settings' && <Settings accounts={accounts} onChanged={refreshAccounts} />}
        </main>

        <BottomBar
          dim={dim}
          onDimChange={setDim}
          ink={ink}
          onInkChange={setInk}
          mode={barMode}
          onModeChange={setBarMode}
          mailTotal={mailTotal}
        />
      </div>
    </SyncProvider>
  );
}
