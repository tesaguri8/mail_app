import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { AccountSyncStatus } from '@bindings/AccountSyncStatus';
import { accountSyncStatuses, SYNC_STATUS_EVENT } from '../services/mail';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * アカウントごとのメールの同期の最後の結果（アカウント選択の接続状況の印）。
 * 開いたときに今の状態をまとめて取り、以後は "sync:status" イベントで 1 件ずつ差し替える。
 * まだ一度も同期していないアカウントは入っていない（印は「まだ同期していない」）。
 */
export function useAccountSyncStatus(): {
  /** アカウント id ごとの状態。 */
  statuses: Record<number, AccountSyncStatus>;
  /** 開いたときの状態を取り終えたか（それまでは「まだ同期の結果が無い」と決めつけない）。 */
  loaded: boolean;
} {
  const [byId, setById] = useState<Record<number, AccountSyncStatus>>({});
  const [loaded, setLoaded] = useState(!isTauri);
  useEffect(() => {
    if (!isTauri) return;
    let alive = true;
    let unlisten: (() => void) | null = null;
    const put = (s: AccountSyncStatus) => setById((cur) => ({ ...cur, [s.account_id]: s }));
    listen<AccountSyncStatus>(SYNC_STATUS_EVENT, (e) => put(e.payload)).then((u) => {
      if (alive) unlisten = u;
      else u();
    });
    accountSyncStatuses()
      .then((all) => {
        if (!alive) return;
        // イベントで先に届いたもの（より新しい）は残す。
        setById((cur) => {
          const next = { ...cur };
          for (const s of all) if (!next[s.account_id]) next[s.account_id] = s;
          return next;
        });
      })
      .catch(() => undefined)
      .finally(() => alive && setLoaded(true));
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  return { statuses: byId, loaded };
}
