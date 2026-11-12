import { useCallback, useEffect, useState } from 'react';
import { TitleBar, type AppView } from './components/TitleBar';
import { BottomBar } from './components/BottomBar';
import { Home } from './components/Home';
import { MailboxView, type MailOpenRequest } from './components/MailboxView';
import { KeepAlivePanel } from './components/KeepAlivePanel';
import { AddressBook } from './components/AddressBook';
import { CalendarView } from './components/CalendarView';
import { Settings } from './components/Settings';
import { accountList } from './services/accounts';
import { useAutoSync, MAIL_SYNCED_EVENT, CONTACTS_SYNCED_EVENT } from './hooks/useAutoSync';
import { useContactsStore } from './stores/contacts';
import { useContactCountStore } from './stores/contactCount';
import { useReminders } from './hooks/useReminders';
import { SyncProvider } from './components/SyncProvider';
import type { AccountSummary } from '@bindings/AccountSummary';
// 背景写真プール（同梱サンプル）。docs/UI_UX_DESIGN.md 背景写真システム
import { BACKGROUNDS, getBackgroundIndex, setBackgroundIndex } from './config/backgrounds';
import { BodyZoomLayer } from './components/BodyZoom';
import {
  appearanceStyle,
  backdropCss,
  panelTintCss,
  readDim,
  readInk,
  writeDim,
  writeInk,
} from './config/appearance';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 起動から連絡先一覧を先読みするまでの待ち（ms）。 */
const CONTACTS_PREFETCH_DELAY_MS = 3000;

export default function App() {
  const [view, setView] = useState<AppView>('home');
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  // ホーム等から「このアカウント（このメール）を開く」の依頼。メール画面は捨てずに隠しておく
  // ので、一度だけ読む初期値ではなく、依頼ごとに番号を進めて届ける（同じメールでも効く）。
  const [mailOpen, setMailOpen] = useState<MailOpenRequest | null>(null);
  // メール画面で現在表示中のアカウント選択（'all'=全て）。フッターの件数表示に使う。
  const [mailSel, setMailSel] = useState<number | 'all' | null>(null);
  // メール画面で絞り込み中のスレッドの件数（絞り込んでいなければ null）。
  const [mailFiltered, setMailFiltered] = useState<number | null>(null);
  // 背景のかぶせ（暗さ）。写真によって文字が見づらい時に上げる。
  const [dim, setDim] = useState<number>(readDim);
  useEffect(() => writeDim(dim), [dim]);
  // 文字色（インク）。0=白, 0.85=黒。UI 全体を白⇄黒でトーン反転させて明るい写真でも読める。
  const [ink, setInk] = useState<number>(readInk);
  useEffect(() => writeInk(ink), [ink]);
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
  const backdrop = backdropCss(dim, backgroundUrl);
  // 各画面の面（パネル）に敷く土台（config/appearance）。ホームは全面ビジュアルが意匠なので敷かない。
  const panelTint = panelTintCss(dim);

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

  // 自動同期: 起動中はどの画面にいても設定間隔（既定30秒）で全メールアカウント＋Google
  // （カレンダー・連絡先）を同期し、連絡先を変えたら即送る（利用者の判断 2026-10-09）。
  const syncNow = useAutoSync(accounts);

  // カレンダーのリマインダー通知（アプリ起動中、期限が来たら OS 通知）。
  useReminders();

  // 同期が完了したらアカウント（未読数バッジ）を更新する。
  useEffect(() => {
    window.addEventListener(MAIL_SYNCED_EVENT, refreshAccounts);
    return () => window.removeEventListener(MAIL_SYNCED_EVENT, refreshAccounts);
  }, [refreshAccounts]);

  // 連絡先の一覧を先に読んでおく（初めて連絡先タブに入ったときも待たせない）。起動直後の
  // 同期・描画と取り合わないよう少し遅らせる。自動同期で連絡先が変わったら写しも取り直す。
  useEffect(() => {
    if (!isTauri) return;
    const refresh = () => void useContactsStore.getState().refresh();
    const h = setTimeout(refresh, CONTACTS_PREFETCH_DELAY_MS);
    window.addEventListener(CONTACTS_SYNCED_EVENT, refresh);
    return () => {
      clearTimeout(h);
      window.removeEventListener(CONTACTS_SYNCED_EVENT, refresh);
    };
  }, []);

  const openMail = (accountId: number, mailId?: number) => {
    setMailOpen((prev) => ({ accountId, mailId: mailId ?? null, seq: (prev?.seq ?? 0) + 1 }));
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

  // フッター左に出す連絡先の件数（連絡先の一覧を出しているときだけ）。
  const contactCount = useContactCountStore((s) => s.count);

  // タイトルバーからの遷移。各画面は最後の作業状態のまま表示する（KeepAlivePanel）。
  // ホーム/メールは押すたびに同期（同じビューを再度押した時も含む）。
  const navigate = (v: AppView) => {
    if ((v === 'home' || v === 'mail') && v === view) syncNow();
    setView(v);
  };

  return (
    <SyncProvider>
      <div
        className="flex h-full flex-col overflow-hidden bg-cover bg-center text-white"
        style={appearanceStyle(backdrop, ink)}
      >
        <TitleBar onNavigate={navigate} onCycleBackground={cycleBackground} />
        <BodyZoomLayer />

        <main
          className="relative min-h-0 flex-1 overflow-hidden"
          style={view === 'home' ? undefined : { backgroundColor: panelTint }}
        >
          {/* 一度開いた画面は捨てずに隠しておく（タブを移って戻っても作業状態が残る）。 */}
          <KeepAlivePanel active={view === 'home'}>
            <Home
              accounts={accounts}
              onOpenMail={openMail}
              onOpenCalendar={() => navigate('calendar')}
            />
          </KeepAlivePanel>
          <KeepAlivePanel active={view === 'mail'}>
            <MailboxView
              accounts={accounts}
              openRequest={mailOpen}
              onAccountChange={setMailSel}
              onFilteredCountChange={setMailFiltered}
            />
          </KeepAlivePanel>
          <KeepAlivePanel active={view === 'contacts'}>
            <AddressBook />
          </KeepAlivePanel>
          <KeepAlivePanel active={view === 'calendar'}>
            <CalendarView />
          </KeepAlivePanel>
          <KeepAlivePanel active={view === 'settings'}>
            <Settings accounts={accounts} onChanged={refreshAccounts} />
          </KeepAlivePanel>
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
          mailFiltered={view === 'mail' ? mailFiltered : null}
          contactCount={view === 'contacts' ? contactCount : null}
        />
      </div>
    </SyncProvider>
  );
}
