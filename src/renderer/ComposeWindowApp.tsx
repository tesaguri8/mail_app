import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { emit } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { X } from 'lucide-react';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { ComposeHandoff } from '@bindings/ComposeHandoff';
import type { MailDetail } from '@bindings/MailDetail';
import { Compose, type ComposeTarget } from './components/Compose';
import { handoffTarget } from './components/composeHandoff';
import { MailBody } from './components/MailBody';
import { SyncProvider } from './components/SyncProvider';
import { accountList } from './services/accounts';
import { COMPOSE_WINDOW_DONE_EVENT, composeWindowHandoff } from './services/composeWindow';
import { mailGet, mailGetDraft } from './services/mail';
import { BACKGROUNDS, getBackgroundIndex } from './config/backgrounds';
import {
  appearanceStyle,
  backdropCss,
  isAppearanceKey,
  panelTintCss,
  readDim,
  readInk,
} from './config/appearance';

type Loaded =
  | { kind: 'loading' }
  | { kind: 'missing' }
  | {
      kind: 'ready';
      handoff: ComposeHandoff;
      target: ComposeTarget;
      source: MailDetail | null;
      accounts: AccountSummary[];
    };

/** ウィンドウを閉じる。メインへ一覧の読み直しを知らせてから閉じる（閉じる確認は Compose 側で済ませる）。 */
function closeThisWindow() {
  void emit(COMPOSE_WINDOW_DONE_EVENT)
    .catch(() => undefined)
    .finally(() => void getCurrentWindow().destroy());
}

/**
 * 作成の別ウィンドウ（label `compose-<連番>`。docs/COMPOSE.md §別ウィンドウ）。
 * メインの作成画面が Rust に預けた書きかけを受け取り、元メール・下書き・アカウント一覧を
 * 自分で読み直して作成画面を出す。見た目（背景写真・濃さ・文字色）はメインと同じ設定を使う。
 * 同期・リマインダーはメインのウィンドウだけが回す（ここでは回さない）。
 */
export default function ComposeWindowApp() {
  const { t } = useTranslation();
  const [loaded, setLoaded] = useState<Loaded>({ kind: 'loading' });
  // 見た目はメインと同じ保存値を読み、メインで変えたら追随する（localStorage は同じ origin で共有）。
  const [dim, setDim] = useState(readDim);
  const [ink, setInk] = useState(readInk);
  const [bgIndex, setBgIndex] = useState(getBackgroundIndex);
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (isAppearanceKey(e.key)) {
        setDim(readDim());
        setInk(readInk());
      } else {
        setBgIndex(getBackgroundIndex());
      }
    };
    window.addEventListener('storage', onStorage);
    return () => window.removeEventListener('storage', onStorage);
  }, []);

  useEffect(() => {
    let alive = true;
    (async () => {
      const handoff = await composeWindowHandoff();
      if (!handoff) {
        if (alive) setLoaded({ kind: 'missing' });
        return;
      }
      const [accounts, source, draft] = await Promise.all([
        accountList(),
        handoff.source_id != null ? mailGet(handoff.source_id).catch(() => null) : null,
        handoff.mode === 'draft' && handoff.draft_id != null
          ? mailGetDraft(handoff.draft_id)
          : null,
      ]);
      const target = handoffTarget(handoff, source, draft);
      if (!alive) return;
      setLoaded(
        target ? { kind: 'ready', handoff, target, source, accounts } : { kind: 'missing' }
      );
    })().catch(() => {
      if (alive) setLoaded({ kind: 'missing' });
    });
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    if (loaded.kind === 'ready') document.title = t(`compose.${loaded.target.mode}`);
  }, [loaded, t]);

  const backdrop = backdropCss(dim, BACKGROUNDS[bgIndex] ?? BACKGROUNDS[0] ?? '');

  return (
    <SyncProvider>
      <div
        className="flex h-full flex-col overflow-hidden bg-cover bg-center text-white"
        style={appearanceStyle(backdrop, ink)}
      >
        <main
          className="relative min-h-0 flex-1 overflow-hidden"
          style={{ backgroundColor: panelTintCss(dim) }}
        >
          {loaded.kind === 'ready' ? (
            <ComposeWindowBody loaded={loaded} />
          ) : (
            <div className="flex h-full flex-col">
              {/* 作成画面を出せない間も、移動と閉じるはできるようにする。 */}
              <div
                data-tauri-drag-region
                className="flex select-none items-center justify-end border-b border-white/10 px-4 py-2.5"
              >
                <button
                  type="button"
                  onClick={closeThisWindow}
                  title={t('titlebar.close')}
                  aria-label={t('titlebar.close')}
                  className="flex h-7 w-7 items-center justify-center rounded-md text-white/55 hover:bg-red-500/70 hover:text-white"
                >
                  <X size={16} />
                </button>
              </div>
              <div className="flex flex-1 items-center justify-center p-6 text-center text-sm text-white/60">
                {loaded.kind === 'loading'
                  ? t('compose.windowLoading')
                  : t('compose.windowMissing')}
              </div>
            </div>
          )}
        </main>
      </div>
    </SyncProvider>
  );
}

/** 作成画面。返信・転送や返信下書きは、メインと同じく左=下書き・右=元メールの 2 分割。 */
function ComposeWindowBody({ loaded }: { loaded: Extract<Loaded, { kind: 'ready' }> }) {
  const { handoff, target, source, accounts } = loaded;
  const compose = (
    <Compose
      accounts={accounts}
      defaultAccountId={handoff.account_id}
      target={target}
      resume={handoff}
      windowed
      onClose={closeThisWindow}
    />
  );
  if (!source) return compose;
  return (
    <div
      className="grid h-full grid-rows-1 min-h-0 overflow-hidden"
      style={{ gridTemplateColumns: '1fr 1fr' }}
    >
      <div className="min-h-0 overflow-hidden">{compose}</div>
      <div className="flex min-h-0 flex-col overflow-hidden border-l border-white/10">
        <MailBody detail={source} />
      </div>
    </div>
  );
}
