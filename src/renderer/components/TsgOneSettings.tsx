import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { LogIn, LogOut, RefreshCw, ExternalLink } from 'lucide-react';
import type { TsgOneProvider } from '@bindings/TsgOneProvider';
import type { TsgOneStatus } from '@bindings/TsgOneStatus';
import {
  TSG_ONE_CHANGED_EVENT,
  tsgOneCancelLogin,
  tsgOneLogin,
  tsgOneLogout,
  tsgOneOpenMemberSite,
  tsgOneRecheckDevice,
  tsgOneStatus,
} from '../services/tsgOne';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * ブラウザでのログインを待つ長さ。tsg-core がログインの途中を持つ時間（10 分）と同じ
 * （src-tauri/src/services/tsg_one.rs の LOGIN_TTL）。過ぎたら待つのをやめる。
 */
const LOGIN_WAIT_MS = 10 * 60 * 1000;

const PROVIDERS: TsgOneProvider[] = ['google', 'github', 'apple'];

const btnCls =
  'inline-flex items-center gap-1.5 rounded-md bg-white/15 px-3 py-2 text-sm hover:bg-white/25 disabled:opacity-40';

/**
 * 設定の「TSG One」: TSG One のアカウントでのログイン・ログアウト・ログイン中のアカウント。
 * **メールの Google の接続（設定の「アカウント」）とは別のもの。**利用権の確認はまだ行わない。
 */
export function TsgOneSettings() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<TsgOneStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri) return;
    let alive = true;
    let unlisten: (() => void) | null = null;
    listen<TsgOneStatus>(TSG_ONE_CHANGED_EVENT, (e) => setStatus(e.payload)).then((u) => {
      if (alive) unlisten = u;
      else u();
    });
    tsgOneStatus()
      .then((s) => alive && setStatus(s))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  // ブラウザでのログインを待ちすぎたら、待つのをやめる（戻りは受けなくなる）。
  const pending = status?.pending ?? false;
  useEffect(() => {
    if (!pending) return;
    const timer = window.setTimeout(() => {
      tsgOneCancelLogin()
        .then(setStatus)
        .catch(() => undefined);
    }, LOGIN_WAIT_MS);
    return () => window.clearTimeout(timer);
  }, [pending]);

  const run = async (action: () => Promise<TsgOneStatus>) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      setStatus(await action());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const user = status?.user ?? null;
  const problem = status?.problem ?? null;

  return (
    <div className="max-w-xl space-y-4 text-sm">
      <div>
        <h2 className="text-base font-semibold text-white">{t('tsgOne.title')}</h2>
        <p className="mt-1 text-white/60">{t('tsgOne.lead')}</p>
      </div>

      {user ? (
        <div className="space-y-3 rounded-lg bg-black/40 p-4 backdrop-blur-sm" data-tsg-signed-in>
          <div className="text-xs text-white/50">{t('tsgOne.signedInAs')}</div>
          <div>
            <div className="text-white">{user.display_name || user.email || user.id}</div>
            {user.display_name && user.email && (
              <div className="text-xs text-white/60">{user.email}</div>
            )}
            {user.provider && (
              <div className="mt-1 text-xs text-white/40">
                {t('tsgOne.via', { provider: providerLabel(user.provider) })}
              </div>
            )}
          </div>

          {status?.over_cap && (
            <div
              className="space-y-2 rounded-md bg-amber-500/15 p-3 text-amber-100"
              role="alert"
              data-tsg-over-cap
            >
              <div>{t('tsgOne.overCap')}</div>
              <div className="flex flex-wrap gap-2">
                <button
                  className={btnCls}
                  onClick={() => tsgOneOpenMemberSite().catch((e) => setError(String(e)))}
                >
                  <ExternalLink size={14} />
                  {t('tsgOne.openMemberSite')}
                </button>
                <button className={btnCls} disabled={busy} onClick={() => run(tsgOneRecheckDevice)}>
                  <RefreshCw size={14} />
                  {t('tsgOne.recheck')}
                </button>
              </div>
            </div>
          )}

          <button className={btnCls} disabled={busy} onClick={() => run(tsgOneLogout)}>
            <LogOut size={14} />
            {t('tsgOne.logout')}
          </button>
        </div>
      ) : (
        <div className="space-y-3 rounded-lg bg-black/40 p-4 backdrop-blur-sm" data-tsg-signed-out>
          <div className="text-white/70">{t('tsgOne.notSignedIn')}</div>
          <div className="flex flex-wrap gap-2">
            {PROVIDERS.map((p) => (
              <button
                key={p}
                className={btnCls}
                disabled={busy || pending || !isTauri}
                onClick={() => run(() => tsgOneLogin(p))}
              >
                <LogIn size={14} />
                {t('tsgOne.loginWith', { provider: providerLabel(p) })}
              </button>
            ))}
          </div>
          {pending && (
            <div className="flex items-center gap-3 text-white/70" data-tsg-pending>
              <span>{t('tsgOne.waiting')}</span>
              <button className={btnCls} onClick={() => run(tsgOneCancelLogin)}>
                {t('tsgOne.cancel')}
              </button>
            </div>
          )}
        </div>
      )}

      {problem && (
        <div className="rounded-md bg-red-500/15 p-3 text-red-100" role="alert" data-tsg-problem>
          <div>{t(`tsgOne.problem.${problem}`)}</div>
          {status?.problem_detail && (
            <div className="mt-1 text-xs text-red-100/60">{status.problem_detail}</div>
          )}
        </div>
      )}
      {error && <div className="text-xs text-red-300">{error}</div>}
    </div>
  );
}

/** 提供元の表示名（固有名なので翻訳しない）。 */
function providerLabel(provider: string): string {
  switch (provider) {
    case 'google':
      return 'Google';
    case 'github':
      return 'GitHub';
    case 'apple':
      return 'Apple';
    default:
      return provider;
  }
}
