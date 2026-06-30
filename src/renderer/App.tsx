import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { TitleBar } from './components/TitleBar';
import { AccountSetup } from './components/AccountSetup';
import { APP } from './config/appIdentity';
import type { AppInfo } from '@bindings/AppInfo';
import type { DbInfo } from '@bindings/DbInfo';
// Phase 1: アプリ同梱の背景画像（プレースホルダ。docs/UI_UX_DESIGN.md 背景写真システム）
import backgroundUrl from './assets/background.jpg';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

function useClock() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 1000 * 30);
    return () => clearInterval(id);
  }, []);
  return now;
}

export default function App() {
  const { t } = useTranslation();
  const now = useClock();
  const [appInfo, setAppInfo] = useState<string>('');
  const [dbInfo, setDbInfo] = useState<string>('');

  useEffect(() => {
    if (!isTauri) return;
    // ts-rs 由来の境界型を返す Rust コマンド
    invoke<AppInfo>('app_info')
      .then((info) => setAppInfo(`${info.name} v${info.version} (${info.identifier})`))
      .catch(() => undefined);
    invoke<DbInfo>('db_info')
      .then((info) => setDbInfo(`DB schema v${info.schema_version}`))
      .catch(() => undefined);
  }, []);

  const hh = String(now.getHours()).padStart(2, '0');
  const mm = String(now.getMinutes()).padStart(2, '0');
  const dateStr = now.toLocaleDateString();

  return (
    <div
      className="flex h-full flex-col bg-cover bg-center text-white"
      style={{
        // 全面ビジュアル背景。可読性のため上から時間帯風グラデーションを重ねる。
        backgroundImage: `linear-gradient(160deg, rgba(20,20,40,0.45) 0%, rgba(10,15,35,0.65) 100%), url(${backgroundUrl})`,
      }}
    >
      <TitleBar />

      <main className="flex flex-1 flex-col items-center justify-center gap-4 px-6 text-center">
        <div className="text-6xl font-light tabular-nums tracking-tight drop-shadow">
          {hh}:{mm}
        </div>
        <div className="text-sm text-white/70">{dateStr}</div>

        <h1 className="mt-4 text-2xl font-semibold drop-shadow">
          {APP.productName}
        </h1>
        <p className="text-white/70">{t('app.tagline')}</p>

        <div className="mt-6">
          <AccountSetup />
        </div>

        {(appInfo || dbInfo) && (
          <p className="mt-4 text-xs text-white/40">
            {[appInfo, dbInfo].filter(Boolean).join('  ·  ')}
          </p>
        )}
      </main>
    </div>
  );
}
