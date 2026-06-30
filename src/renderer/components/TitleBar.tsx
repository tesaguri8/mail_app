import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { House, Minus, Pin, Settings, Square, X } from 'lucide-react';

const ICON = 15;

/**
 * フレームレス用の自作タイトルバー（docs/UI_UX_DESIGN.md §1.5）。
 * - data-tauri-drag-region でドラッグ移動
 * - 最前面固定（always-on-top）トグル＝常駐用
 * - 最小化 / 最大化 / 閉じる
 * dev:renderer（ブラウザ単体）では Tauri API が無いため安全に no-op。
 */
const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export type AppView = 'home' | 'mail' | 'settings';

export function TitleBar({
  view,
  onNavigate,
}: {
  view: AppView;
  onNavigate: (v: AppView) => void;
}) {
  const { t, i18n } = useTranslation();
  const [pinned, setPinned] = useState(false);

  useEffect(() => {
    if (!isTauri) return;
    getCurrentWindow()
      .isAlwaysOnTop()
      .then(setPinned)
      .catch(() => undefined);
  }, []);

  const win = () => getCurrentWindow();
  const togglePin = async () => {
    if (!isTauri) return;
    const next = !pinned;
    await win().setAlwaysOnTop(next);
    setPinned(next);
  };

  return (
    <header
      data-tauri-drag-region
      className="flex h-9 select-none items-center justify-between px-3 text-white/90"
    >
      <div className="flex items-center gap-1">
        <button
          onClick={() => onNavigate('home')}
          className={`flex items-center justify-center rounded p-1.5 hover:bg-white/20 ${view === 'home' ? 'bg-white/25' : ''}`}
          title={t('nav.home')}
        >
          <House size={ICON} />
        </button>
        <button
          onClick={() => onNavigate('settings')}
          className={`flex items-center justify-center rounded p-1.5 hover:bg-white/20 ${view === 'settings' ? 'bg-white/25' : ''}`}
          title={t('nav.settings')}
        >
          <Settings size={ICON} />
        </button>
        <span data-tauri-drag-region className="ml-2 text-xs font-medium tracking-wide text-white/80">
          {t('app.tagline')}
        </span>
      </div>
      <div className="flex items-center gap-1">
        <button
          onClick={() => i18n.changeLanguage(i18n.language === 'ja' ? 'en' : 'ja')}
          className="rounded px-2 py-0.5 text-xs hover:bg-white/20"
          title={t('lang.switch')}
        >
          {t('lang.switch')}
        </button>
        <button
          onClick={togglePin}
          className={`flex items-center justify-center rounded p-1.5 hover:bg-white/20 ${pinned ? 'bg-white/25' : ''}`}
          title={t('titlebar.alwaysOnTop')}
        >
          <Pin size={ICON} className={pinned ? 'fill-current' : ''} />
        </button>
        <button
          onClick={() => isTauri && win().minimize()}
          className="flex items-center justify-center rounded p-1.5 hover:bg-white/20"
          title={t('titlebar.minimize')}
        >
          <Minus size={ICON} />
        </button>
        <button
          onClick={() => isTauri && win().toggleMaximize()}
          className="flex items-center justify-center rounded p-1.5 hover:bg-white/20"
          title={t('titlebar.maximize')}
        >
          <Square size={ICON - 2} />
        </button>
        <button
          onClick={() => isTauri && win().close()}
          className="flex items-center justify-center rounded p-1.5 hover:bg-red-500/70"
          title={t('titlebar.close')}
        >
          <X size={ICON} />
        </button>
      </div>
    </header>
  );
}
