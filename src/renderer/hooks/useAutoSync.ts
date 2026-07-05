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
/** 全アカウントの同期が失敗（接続不可等）した後、自動再試行を止める時間（ミリ秒）。
 *  遮断中のサーバーへ叩き続けて遮断を延長させないための安全弁。手動同期には効かない。 */
const AUTOSYNC_COOLDOWN_MS = 5 * 60 * 1000;

export function useAutoSync(active: boolean, accounts: AccountSummary[]): () => void {
  const busy = useRef(false);
  // 直近の一括失敗でクールダウン中なら、この時刻まで自動（定期）同期を止める。
  const cooldownUntil = useRef(0);
  // アカウント増減にだけ追従（unread_count 等の変化で作り直さない）。
  const idsKey = accounts.map((a) => a.id).join(',');

  const syncNow = useCallback(() => {
    if (!isTauri || busy.current) return;
    const ids = idsKey ? idsKey.split(',').map(Number) : [];
    if (ids.length === 0) return;
    busy.current = true;
    (async () => {
      let synced = false;
      let failed = false;
      let stored = 0; // この巡回で新規保存されたメール総数（新着有無の判定に使う）。
      for (const id of ids) {
        try {
          const r = await mailSync(id);
          synced = true;
          stored += r?.stored ?? 0;
        } catch {
          // アカウント単位の失敗は無視して次へ（ただし全滅ならクールダウン）。
          failed = true;
        }
      }
      busy.current = false;
      // 1件も成功せず全滅＝接続不可の可能性大 → しばらく自動再試行を止める（手動は可）。
      cooldownUntil.current = failed && !synced ? Date.now() + AUTOSYNC_COOLDOWN_MS : 0;
      // 新着件数を載せて通知（購読側は新着ゼロなら一覧の再取得を省ける）。
      if (synced) window.dispatchEvent(new CustomEvent(MAIL_SYNCED_EVENT, { detail: { stored } }));
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
  // クールダウン中（直近の接続失敗後）は自動では叩かない（手動同期は別途可）。
  useEffect(() => {
    if (active && Date.now() >= cooldownUntil.current) syncNow();
  }, [active, syncNow]);

  // 滞在中は設定間隔で定期同期（0=オフ）。直近の一括失敗でクールダウン中は
  // 定期同期をスキップして、遮断中のサーバーを叩き続けないようにする（手動同期は別途可）。
  useEffect(() => {
    if (!active || intervalSec <= 0) return;
    const h = setInterval(() => {
      if (Date.now() >= cooldownUntil.current) syncNow();
    }, intervalSec * 1000);
    return () => clearInterval(h);
  }, [active, intervalSec, syncNow]);

  return syncNow;
}
