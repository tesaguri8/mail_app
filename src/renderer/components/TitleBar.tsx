import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getCurrentWindow } from '@tauri-apps/api/window';

/**
 * フレームレス用の自作タイトルバー（docs/UI_UX_DESIGN.md §1.5）。
 * - data-tauri-drag-region でドラッグ移動
 * - 最前面固定（always-on-top）トグル＝常駐用
 * - 最小化 / 最大化 / 閉じる
 * dev:renderer（ブラウザ単体）では Tauri API が無いため安全に no-op。
 */
const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export function TitleBar() {
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
      <div data-tauri-drag-region className="text-xs font-medium tracking-wide">
        {t('app.tagline')}
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
          className={`rounded px-2 py-0.5 text-sm hover:bg-white/20 ${pinned ? 'bg-white/25' : ''}`}
          title={t('titlebar.alwaysOnTop')}
        >
          📌
        </button>
        <button
          onClick={() => isTauri && win().minimize()}
          className="rounded px-2 py-0.5 hover:bg-white/20"
          title={t('titlebar.minimize')}
        >
          —
        </button>
        <button
          onClick={() => isTauri && win().toggleMaximize()}
          className="rounded px-2 py-0.5 hover:bg-white/20"
          title={t('titlebar.maximize')}
        >
          ▢
        </button>
        <button
          onClick={() => isTauri && win().close()}
          className="rounded px-2 py-0.5 hover:bg-red-500/70"
          title={t('titlebar.close')}
        >
          ✕
        </button>
      </div>
    </header>
  );
}
