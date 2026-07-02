import { useCallback, useEffect, useRef, useState } from 'react';
import type { AccountSummary } from '@bindings/AccountSummary';
import { mailSync } from '../services/mail';
import { getAutoSyncInterval, PREFS_EVENT } from '../config/prefs';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 自動同期が 1 巡完了したら発火する（一覧・件数の再読み込み合図）。 */
export const MAIL_SYNCED_EVENT = 'rondine:mail-synced';

/**
 * 自動同期（docs 仕様: ホーム/メールボタン押下時＋ホーム・メールモード滞在中の定期同期）。
 * - active の間、設定（getAutoSyncInterval, 0=オフ）の間隔で全アカウントを順に同期する。
 * - 戻り値 syncNow で任意タイミングの即時同期も呼べる（ボタン押下時用）。
 * - 多重実行はガードし、1 巡完了ごとに MAIL_SYNCED_EVENT を発火する。
 */
export function useAutoSync(active: boolean, accounts: AccountSummary[]): () => void {
  const busy = useRef(false);
  // アカウント増減にだけ追従（unread_count 等の変化で作り直さない）。
  const idsKey = accounts.map((a) => a.id).join(',');

  const syncNow = useCallback(() => {
    if (!isTauri || busy.current) return;
    const ids = idsKey ? idsKey.split(',').map(Number) : [];
    if (ids.length === 0) return;
    busy.current = true;
    (async () => {
      let synced = false;
      for (const id of ids) {
        try {
          await mailSync(id);
          synced = true;
        } catch {
          /* アカウント単位の失敗は無視して次へ */
        }
      }
      busy.current = false;
      if (synced) window.dispatchEvent(new Event(MAIL_SYNCED_EVENT));
    })();
  }, [idsKey]);

  // 設定変更（間隔）に追従する。
  const [intervalSec, setIntervalSec] = useState(getAutoSyncInterval());
  useEffect(() => {
    const onPrefs = () => setIntervalSec(getAutoSyncInterval());
    window.addEventListener(PREFS_EVENT, onPrefs);
    return () => window.removeEventListener(PREFS_EVENT, onPrefs);
  }, []);

  // 対象モードに入った時に即同期（起動直後のホーム表示・ホーム↔メール遷移を含む）。
  useEffect(() => {
    if (active) syncNow();
  }, [active, syncNow]);

  // 滞在中は設定間隔で定期同期（0=オフ）。
  useEffect(() => {
    if (!active || intervalSec <= 0) return;
    const h = setInterval(syncNow, intervalSec * 1000);
    return () => clearInterval(h);
  }, [active, intervalSec, syncNow]);

  return syncNow;
}
