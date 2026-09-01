import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import { TitleBar, type AppView } from './components/TitleBar';
import { BottomBar } from './components/BottomBar';
import { Home } from './components/Home';
import { MailboxView } from './components/MailboxView';
import type { ComposeTarget } from './components/Compose';
import { AddressBook } from './components/AddressBook';
import { CalendarView } from './components/CalendarView';
import { Settings } from './components/Settings';
import { accountList } from './services/accounts';
import { useAutoSync, MAIL_SYNCED_EVENT } from './hooks/useAutoSync';
import { useReminders } from './hooks/useReminders';
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
  // 作成セッション（新規/返信/転送/下書き再編集）。メール画面はビュー離脱でアンマウント
  // されるため、作成中の状態が消えないよう App 側で保持する。カレンダー等へ移動して戻ると
  // 未編集の返信もそのまま再表示される。
  const [compose, setCompose] = useState<ComposeTarget | null>(null);
  // 編集中の下書き id。書きかけて自動保存された内容を、戻った時に本文込みで復元するのに使う。
  const [composeDraftId, setComposeDraftId] = useState<number | null>(null);
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
  // 背景（かぶせグラデ＋写真）。会話の固定見出しラベル等が「同じ背景を同じ位置で」貼って
  // 透明に見せられるよう、同じ文字列を CSS 変数 --app-backdrop としても配る（bg-fixed で位置合わせ）。
  const backdrop = `linear-gradient(160deg, rgba(10,14,28,${(0.35 + dim * 0.6).toFixed(2)}) 0%, rgba(6,9,20,${(0.55 + dim * 0.5).toFixed(2)}) 100%), url(${backgroundUrl})`;
  // 各画面の面（パネル）に敷く土台。写真の上に文字が直接乗ると、明るい写真では沈むため、
  // 背景よりわずかに濃い面を敷いて内容を浮かせる。濃さはスライダー(dim)に追随させ、
  // 0% でも最低 5% は残す（利用者提案 2026-09-01「スライダー指定＋5%」）。
  // ホームは全面ビジュアルが意匠なので敷かない。
  const panelTint = `rgba(6, 9, 20, ${Math.min(dim + 0.05, 1).toFixed(2)})`;

  const refreshAccounts = useCallback(() => {
    if (!isTauri) return;
    accountList()
      .then(setAccounts)
      // 握り潰すと「アカウントを追加しても一覧に出ない」ように見えて原因が追えない。
      // 一覧は空のままにしつつ、理由はコンソールへ残す。
      .catch((e) => console.error('account_list failed:', e));
  }, []);
  useEffect(refreshAccounts, [refreshAccounts]);

  // ホーム/設定へ戻るたびにアカウント（新着数）を更新
  useEffect(() => {
    if (view !== 'mail') refreshAccounts();
  }, [view, refreshAccounts]);

  // 自動同期: ホーム/メール/カレンダーに入った時＋滞在中は設定間隔（既定30秒）で
  // 全メールアカウント＋Google カレンダーを同期する。
  const syncNow = useAutoSync(
    view === 'home' || view === 'mail' || view === 'calendar',
    accounts,
  );

  // カレンダーのリマインダー通知（アプリ起動中、期限が来たら OS 通知）。
  useReminders();

  // 同期が完了したらアカウント（未読数バッジ）を更新する。
  useEffect(() => {
    window.addEventListener(MAIL_SYNCED_EVENT, refreshAccounts);
    return () => window.removeEventListener(MAIL_SYNCED_EVENT, refreshAccounts);
  }, [refreshAccounts]);

  const openMail = (accountId: number, mailId?: number) => {
    // 特定メールを開く操作（ホームの新着クリック等）では、保持していた作成セッションは畳む
    // （書きかけて保存済みの下書きは drafts に残る）。別のメールを見たい意図を優先する。
    if (mailId != null) {
      setCompose(null);
      setComposeDraftId(null);
    }
    setMailAccountId(accountId);
    setMailOpenId(mailId ?? null);
    setView('mail');
  };

  // フッター左に出す「表示中アカウントのメール総数」（メールモード時のみ。'all'=全合計）。
  const mailTotal =
    view !== 'mail'
      ? null
      : mailSel === 'all'
        ? accounts.reduce((s, a) => s + a.total_count, 0)
        : (accounts.find((a) => a.id === mailSel)?.total_count ?? null);
  // 同じ集計のサーバ総数（IMAP EXISTS の合計）。左下を「ローカル/サーバ」表示にする（取り込み完成度）。
  const mailServerTotal =
    view !== 'mail'
      ? null
      : mailSel === 'all'
        ? accounts.reduce((s, a) => s + a.server_total_count, 0)
        : (accounts.find((a) => a.id === mailSel)?.server_total_count ?? null);

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
            backgroundImage: backdrop,
            // 固定見出しラベルが同じ背景を貼れるよう、同じ文字列を配る（bg-fixed で位置を合わせる）。
            '--app-backdrop': backdrop,
            // 文字色スライダー: Tailwind の白を差し替えて UI 全体を白⇄黒でトーン反転。
            '--color-white': inkColor(ink),
          } as CSSProperties
        }
      >
        <TitleBar onNavigate={navigate} onCycleBackground={cycleBackground} />

        <main
          className="min-h-0 flex-1 overflow-hidden"
          style={view === 'home' ? undefined : { backgroundColor: panelTint }}
        >
          {view === 'home' && (
            <Home
              accounts={accounts}
              onOpenMail={openMail}
              onOpenCalendar={() => navigate('calendar')}
            />
          )}
          {view === 'mail' && (
            <MailboxView
              accounts={accounts}
              initialAccountId={mailAccountId}
              initialMailId={mailOpenId}
              onAccountChange={setMailSel}
              compose={compose}
              setCompose={setCompose}
              restoreDraftId={composeDraftId}
              onComposeDraftChange={setComposeDraftId}
            />
          )}
          {view === 'contacts' && <AddressBook />}
          {view === 'calendar' && <CalendarView />}
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
          mailServerTotal={mailServerTotal}
        />
      </div>
    </SyncProvider>
  );
}
