import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { TitleBar } from './components/TitleBar';
import { APP } from './config/appIdentity';

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

  useEffect(() => {
    if (!isTauri) return;
    // ts-rs 由来の境界型を返す Rust コマンド（src-tauri/src/lib.rs の app_info）
    invoke<{ name: string; version: string; identifier: string }>('app_info')
      .then((info) => setAppInfo(`${info.name} v${info.version} (${info.identifier})`))
      .catch(() => undefined);
  }, []);

  const hh = String(now.getHours()).padStart(2, '0');
  const mm = String(now.getMinutes()).padStart(2, '0');
  const dateStr = now.toLocaleDateString();

  return (
    <div
      className="flex h-full flex-col bg-cover bg-center text-white"
      style={{
        // Phase 1: 背景画像の取り込みは後続。まずは美しいグラデーションで全面ビジュアル。
        backgroundImage:
          'linear-gradient(135deg, #1a1a2e 0%, #16213e 45%, #0f3460 100%)',
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

        <div className="mt-6 rounded-xl bg-white/10 px-6 py-4 backdrop-blur">
          <p className="text-white/80">{t('home.placeholder')}</p>
        </div>

        {appInfo && <p className="mt-4 text-xs text-white/40">{appInfo}</p>}
      </main>
    </div>
  );
}
