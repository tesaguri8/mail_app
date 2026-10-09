import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { SyncProgress } from '@bindings/SyncProgress';
import { mailSync } from '../services/mail';
import { googleAccounts, googleSync } from '../services/google';
import { debounce, LOCAL_CHANGE_DEBOUNCE_MS, LOCAL_CHANGE_EVENT } from '../utils/localChange';
import { getAutoSyncInterval, PREFS_EVENT } from '../config/prefs';
import { activityStart, activityStop, activityUpdate } from '../stores/activity';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 自動同期が 1 巡完了したら発火する（一覧・件数の再読み込み合図）。 */
export const MAIL_SYNCED_EVENT = 'rondine:mail-synced';

/** 自動同期で Google カレンダーに変更を取り込んだら発火する（カレンダー表示の再読み込み合図）。 */
export const CALENDAR_SYNCED_EVENT = 'rondine:calendar-synced';

/** 自動同期で Google の連絡先を取り込んで住所録が変わったら発火する（住所録の再読み込み合図）。 */
export const CONTACTS_SYNCED_EVENT = 'rondine:contacts-synced';

/**
 * 自動同期（利用者の判断 2026-10-09: どの画面にいても動く／連絡先も毎回／変更したら即送る）。
 * - Rondine が起動している間、設定（getAutoSyncInterval, 0=オフ）の間隔で全メールアカウントと
 *   Google（カレンダー・連絡先）を順に同期する。画面によって止めない（以前はホーム/メール/
 *   カレンダーにいる間だけで、連絡先の画面に居続けると送られなかった）。
 * - 連絡先・組織を変えたら（`utils/localChange` の合図）、まとめ待ちのあと Google だけを同期する
 *   （メールのサーバーは叩かないので、メールのクールダウン中でも送る）。
 * - 戻り値 syncNow で任意タイミングの即時同期も呼べる（ボタン押下時用）。
 * - 多重実行はガードし、巡回中の依頼は巡回が終わってからすぐ回し直す。1 巡完了ごとに
 *   MAIL_SYNCED_EVENT を発火する。
 */
/** 全アカウントの同期が失敗（接続不可等）した後、自動再試行を止める時間（ミリ秒）。
 *  遮断中のサーバーへ叩き続けて遮断を延長させないための安全弁。手動同期には効かない。 */
const AUTOSYNC_COOLDOWN_MS = 5 * 60 * 1000;

/** 同期の依頼。`mail: false` は Google だけ（ローカルの変更を送るとき）。 */
export type SyncOptions = { mail?: boolean };

export function useAutoSync(accounts: AccountSummary[]): () => void {
  const { t } = useTranslation();
  const busy = useRef(false);
  // 巡回中に来た同期の依頼。捨てると次の定期同期（既定 30 秒後）まで取りに行かないので、
  // 覚えておいて巡回が終わったらすぐ回し直す。
  // 起動直後がこれに当たる: アカウント一覧が届く前（空）の巡回がカレンダー同期で塞がっている間に、
  // 一覧が届いてからの即時同期が来て弾かれ、メールを 30 秒取りに行かなかった（利用者報告 2026-10-07）。
  // 巡回中に来た依頼（null＝無し）。メールも含む依頼が 1 つでもあれば、回し直しでメールも回す。
  const again = useRef<{ mail: boolean } | null>(null);
  // 回し直しは最新のアカウント一覧で行うため、最新の syncNow を指しておく。
  const syncNowRef = useRef<(opts?: SyncOptions) => void>(() => undefined);
  // 直近の一括失敗でクールダウン中なら、この時刻まで自動（定期）同期を止める。
  const cooldownUntil = useRef(0);
  // フッター表示のアカウント名解決用に最新の一覧を保持（syncNow を作り直さずに参照する）。
  const accountsRef = useRef(accounts);
  accountsRef.current = accounts;
  // アカウント増減にだけ追従（unread_count 等の変化で作り直さない）。
  const idsKey = accounts.map((a) => a.id).join(',');

  const syncNow = useCallback(
    (opts: SyncOptions = {}) => {
      if (!isTauri) return;
      const withMail = opts.mail ?? true;
      if (busy.current) {
        again.current = { mail: (again.current?.mail ?? false) || withMail };
        return;
      }
      const ids = withMail && idsKey ? idsKey.split(',').map(Number) : [];
      busy.current = true;
      (async () => {
        // フッターの作業表示は「実際に新着をダウンロードしている時だけ」出す。定期チェックで新着が
        // 無ければ何も出さない（確認のみの巡回で頻繁に点滅させない）。判定は sync:progress の
        // total>0（＝取得予定あり＝新着あり）で行い、増分同期が空振り（total=0）なら無表示。
        // アカウントは順に 1 件ずつ同期し、ダウンロード中はそのアカウント名を出す。
        let act: number | null = null; // ダウンロード表示中の作業 ID（無ければ未表示）。
        let currentLabel = ''; // いま同期中のアカウントの表示ラベル。
        const showProgress = (current: number, total: number) => {
          if (total <= 0) return; // 新着なし（確認のみ）は表示しない。
          if (act == null) act = activityStart(currentLabel);
          activityUpdate(act, { label: currentLabel, current, total });
        };
        const unlistenP =
          ids.length > 0
            ? listen<SyncProgress>('sync:progress', (e) =>
                showProgress(e.payload.current, e.payload.total)
              )
            : null;
        let synced = false;
        let failed = false;
        let stored = 0; // この巡回で新規保存されたメール総数（新着有無の判定に使う）。
        try {
          for (const id of ids) {
            // ダウンロードが始まった時に出す名前（表示名 → メール → id の順で解決）。
            const a = accountsRef.current.find((x) => x.id === id);
            currentLabel = t('activity.receiving', {
              name: a?.display_name?.trim() || a?.email || String(id),
            });
            try {
              const r = await mailSync(id);
              synced = true;
              stored += r?.stored ?? 0;
            } catch {
              // アカウント単位の失敗は無視して次へ（ただし全滅ならクールダウン）。
              failed = true;
            }
          }
          // Google も同じ間隔で同期する（メールの成否とは独立）。カレンダーも連絡先も毎回（連絡先の
          // 取り込みは同期トークンの増分で、送信待ちが溜まっていればここで続きを送る）。連絡先は
          // 取り込みに続けて住所録へ反映する（「今すぐ同期」と同じ）。変化があればカレンダー表示・
          // 住所録へ再読み込みを促す。
          try {
            let calChanged = false;
            let contactsChanged = false;
            for (const g of await googleAccounts()) {
              // 解除中のアカウントは同期しない（再接続で再開する）。
              if (g.disconnected_at != null) continue;
              if (!g.sync_calendar && !g.sync_contacts) continue;
              try {
                const r = await googleSync(g.id, g.sync_contacts);
                if (r.calendar && r.calendar.pulled + r.calendar.deleted_in > 0) calChanged = true;
                const m = r.matched;
                if (
                  (r.contacts && r.contacts.pulled + r.contacts.deleted_in > 0) ||
                  (m && m.created + m.linked > 0)
                ) {
                  contactsChanged = true;
                }
              } catch {
                // アカウント単位の失敗は無視して次へ。
              }
            }
            if (calChanged) window.dispatchEvent(new Event(CALENDAR_SYNCED_EVENT));
            if (contactsChanged) window.dispatchEvent(new Event(CONTACTS_SYNCED_EVENT));
          } catch {
            // 連携アカウント一覧の取得失敗は無視（未連携なら送受信するものは無い）。
          }
        } finally {
          if (unlistenP) (await unlistenP)();
          if (act != null) activityStop(act);
          busy.current = false;
        }
        // メールが1件も成功せず全滅＝接続不可の可能性大 → しばらく自動再試行を止める（手動は可）。
        // カレンダーの成否はクールダウンの判定には含めない。メールを回さなかった巡回（Google だけ）は
        // 判定を変えない。
        if (withMail)
          cooldownUntil.current = failed && !synced ? Date.now() + AUTOSYNC_COOLDOWN_MS : 0;
        // 新着件数を載せて通知（購読側は新着ゼロなら一覧の再取得を省ける）。
        if (synced)
          window.dispatchEvent(new CustomEvent(MAIL_SYNCED_EVENT, { detail: { stored } }));
        const next = again.current;
        if (next) {
          again.current = null;
          syncNowRef.current(next);
        }
      })();
    },
    [idsKey, t]
  );
  syncNowRef.current = syncNow;

  // 設定変更（間隔）に追従する。
  const [intervalSec, setIntervalSec] = useState(getAutoSyncInterval());
  useEffect(() => {
    const onPrefs = () => setIntervalSec(getAutoSyncInterval());
    window.addEventListener(PREFS_EVENT, onPrefs);
    return () => window.removeEventListener(PREFS_EVENT, onPrefs);
  }, []);

  // 起動直後（アカウント一覧が変わったときも）に即同期。クールダウン中（直近の接続失敗後）は
  // 自動では叩かない（手動同期は別途可）。
  useEffect(() => {
    if (Date.now() >= cooldownUntil.current) syncNow();
  }, [syncNow]);

  // 設定間隔で定期同期（0=オフ）。どの画面にいても動く。直近の一括失敗でクールダウン中は
  // 定期同期をスキップして、遮断中のサーバーを叩き続けないようにする（手動同期は別途可）。
  useEffect(() => {
    if (intervalSec <= 0) return;
    const h = setInterval(() => {
      if (Date.now() >= cooldownUntil.current) syncNow();
    }, intervalSec * 1000);
    return () => clearInterval(h);
  }, [intervalSec, syncNow]);

  // 連絡先・組織を変えたら、まとめ待ちのあと Google だけを同期する（変更したら即送る）。
  // 自動同期をオフ（0）にしていても送る（オフは定期の取りに行きを止める設定で、変更を手元に
  // 溜めておく設定ではない）。
  useEffect(() => {
    const d = debounce(() => syncNowRef.current({ mail: false }), LOCAL_CHANGE_DEBOUNCE_MS);
    window.addEventListener(LOCAL_CHANGE_EVENT, d.call);
    return () => {
      window.removeEventListener(LOCAL_CHANGE_EVENT, d.call);
      d.cancel();
    };
  }, []);

  return syncNow;
}
