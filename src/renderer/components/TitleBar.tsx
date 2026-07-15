import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { getVersion } from '@tauri-apps/api/app';
import {
  Contact,
  House,
  Image,
  Mail,
  Minus,
  Pin,
  Settings,
  Square,
  X,
} from 'lucide-react';
import { APP } from '../config/appIdentity';
import { CalendarDateIcon } from './CalendarDateIcon';

const ICON = 18;

/**
 * フレームレス用の自作タイトルバー（docs/UI_UX_DESIGN.md §1.5）。
 * - data-tauri-drag-region でドラッグ移動
 * - 最前面固定（always-on-top）トグル＝常駐用
 * - 最小化 / 最大化 / 閉じる
 * dev:renderer（ブラウザ単体）では Tauri API が無いため安全に no-op。
 */
const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export type AppView = 'home' | 'mail' | 'contacts' | 'calendar' | 'settings';

export function TitleBar({
  onNavigate,
  onCycleBackground,
}: {
  onNavigate: (v: AppView) => void;
  /** 背景写真を次の候補へ切り替える（気に入るまでクリックで送る）。 */
  onCycleBackground?: () => void;
}) {
  const { t, i18n } = useTranslation();
  const [pinned, setPinned] = useState(false);
  // 「Rondine」ラベルのクリックでバージョン名（tauri.conf.json 由来）をトグル表示する。
  const [version, setVersion] = useState<string | null>(null);
  const [showVersion, setShowVersion] = useState(false);

  useEffect(() => {
    if (!isTauri) return;
    getCurrentWindow()
      .isAlwaysOnTop()
      .then(setPinned)
      .catch(() => undefined);
    getVersion()
      .then(setVersion)
      .catch(() => undefined);
  }, []);

  // バージョン表示は 2 秒後に自動的に消す（再クリックで即座に非表示）。
  useEffect(() => {
    if (!showVersion) return;
    const id = setTimeout(() => setShowVersion(false), 2000);
    return () => clearTimeout(id);
  }, [showVersion]);

  const win = () => getCurrentWindow();

  // Ctrl+Q（Mac は Cmd+Q）でアプリを終了する。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && (e.key === 'q' || e.key === 'Q')) {
        e.preventDefault();
        if (isTauri) win().close();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

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
      <button
        type="button"
        onClick={() => setShowVersion((v) => !v)}
        title={t('titlebar.showVersion')}
        className="rounded px-1 text-sm font-semibold tracking-wide hover:bg-white/10"
      >
        {APP.productName}
        {/* 開発モード（tauri dev / vite dev）では ( Dev ) を併記して本番と区別する。 */}
        {import.meta.env.DEV && ' ( Dev )'}
        {showVersion && version && (
          <span className="ml-2 font-normal text-white/60">v{version}</span>
        )}
      </button>
      <div className="flex items-center gap-1">
        <button
          onClick={(e) => {
            onNavigate('home');
            e.currentTarget.blur();
          }}
          title={t('nav.home')}
          className="flex items-center justify-center rounded p-1.5 hover:bg-white/20 focus:bg-white/20"
        >
          <House size={ICON} />
        </button>
        <button
          onClick={(e) => {
            onNavigate('mail');
            e.currentTarget.blur();
          }}
          title={t('nav.mail')}
          className="flex items-center justify-center rounded p-1.5 hover:bg-white/20 focus:bg-white/20"
        >
          <Mail size={ICON} />
        </button>
        <button
          onClick={(e) => {
            onNavigate('contacts');
            e.currentTarget.blur();
          }}
          title={t('nav.contacts')}
          className="flex items-center justify-center rounded p-1.5 hover:bg-white/20 focus:bg-white/20"
        >
          <Contact size={ICON} />
        </button>
        <button
          onClick={(e) => {
            onNavigate('calendar');
            e.currentTarget.blur();
          }}
          title={t('nav.calendar')}
          className="flex items-center justify-center rounded p-1.5 hover:bg-white/20 focus:bg-white/20"
        >
          <CalendarDateIcon size={ICON} />
        </button>
        <button
          onClick={(e) => {
            onNavigate('settings');
            e.currentTarget.blur();
          }}
          title={t('nav.settings')}
          className="flex items-center justify-center rounded p-1.5 hover:bg-white/20 focus:bg-white/20"
        >
          <Settings size={ICON} />
        </button>
        <button
          onClick={() => onCycleBackground?.()}
          title={t('titlebar.cycleBackground')}
          className="flex items-center justify-center rounded p-1.5 hover:bg-white/20 focus:bg-white/20"
        >
          <Image size={ICON} />
        </button>
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
