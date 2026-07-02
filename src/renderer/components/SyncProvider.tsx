import {
  createContext,
  useCallback,
  useContext,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { CheckCircle2, RefreshCw, X, XCircle } from 'lucide-react';
import type { SyncProgress } from '@bindings/SyncProgress';
import { mailResync, mailSync, mailSyncCancel } from '../services/mail';

type SyncKind = 'sync' | 'resync';
type ActiveSync = { accountId: number; label: string; kind: SyncKind; progress: SyncProgress | null };
type ToastItem = { id: number; text: string; kind: 'ok' | 'error' };

type SyncApi = {
  /** 現在バックグラウンド実行中の同期（無ければ null）。 */
  active: ActiveSync | null;
  /** 同期/再取り込みをバックグラウンドで開始（1 件ずつ）。 */
  start: (accountId: number, label: string, kind: SyncKind) => void;
  /** 実行中の同期を中断する。 */
  cancel: () => void;
  /** 任意のトーストを表示する。 */
  toast: (text: string, kind?: 'ok' | 'error') => void;
};

const SyncCtx = createContext<SyncApi | null>(null);

export function useSync(): SyncApi {
  const c = useContext(SyncCtx);
  if (!c) throw new Error('useSync must be used within SyncProvider');
  return c;
}

let toastSeq = 0;

/**
 * 同期/再取り込みをアプリ全体で管理する。バックグラウンドで走らせつつ、どの画面でも
 * 進捗インジケータ（＋中断）を表示し、完了時にトーストで知らせる。
 */
export function SyncProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const [active, setActive] = useState<ActiveSync | null>(null);
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const activeRef = useRef<ActiveSync | null>(null);
  activeRef.current = active;
  const cancelledRef = useRef(false);

  const pushToast = useCallback((text: string, kind: 'ok' | 'error' = 'ok') => {
    const id = ++toastSeq;
    setToasts((ts) => [...ts, { id, text, kind }]);
    setTimeout(() => setToasts((ts) => ts.filter((x) => x.id !== id)), 5000);
  }, []);

  const start = useCallback(
    (accountId: number, label: string, kind: SyncKind) => {
      if (activeRef.current) return; // 同時実行は 1 件まで
      cancelledRef.current = false;
      setActive({ accountId, label, kind, progress: null });

      let unlisten: (() => void) | null = null;
      listen<SyncProgress>('sync:progress', (e) => {
        setActive((cur) => (cur ? { ...cur, progress: e.payload } : cur));
      }).then((u) => {
        unlisten = u;
      });

      const call = kind === 'resync' ? mailResync(accountId) : mailSync(accountId);
      call
        .then((r) => {
          if (cancelledRef.current) {
            pushToast(`${label}: ${t('sync.cancelled')}`, 'ok');
          } else {
            const key = kind === 'resync' ? 'storage.resynced' : 'mailbox.result';
            pushToast(`${label}: ${t(key, r as Record<string, number>)}`, 'ok');
          }
        })
        .catch((err) => {
          pushToast(`${label}: ${String(err)}`, 'error');
        })
        .finally(() => {
          unlisten?.();
          setActive(null);
        });
    },
    [pushToast, t],
  );

  const cancel = useCallback(() => {
    const a = activeRef.current;
    if (!a) return;
    cancelledRef.current = true;
    mailSyncCancel(a.accountId).catch(() => undefined);
  }, []);

  return (
    <SyncCtx.Provider value={{ active, start, cancel, toast: pushToast }}>
      {children}
      {active && <SyncIndicator active={active} onCancel={cancel} />}
      <ToastHost toasts={toasts} onDismiss={(id) => setToasts((ts) => ts.filter((x) => x.id !== id))} />
    </SyncCtx.Provider>
  );
}

/** 右下の同期インジケータ（フォルダ別 現在/全体＋中断）。全画面共通で表示。 */
function SyncIndicator({ active, onCancel }: { active: ActiveSync; onCancel: () => void }) {
  const { t } = useTranslation();
  const p = active.progress;
  const pct = p && p.total > 0 ? Math.min(100, Math.round((p.current / p.total) * 100)) : 0;
  return (
    <div className="fixed bottom-12 right-4 z-40 w-72 rounded-lg border border-white/15 bg-neutral-900/85 p-3 text-xs text-white shadow-2xl backdrop-blur">
      <div className="flex items-center gap-2">
        <RefreshCw size={14} className="shrink-0 animate-spin text-sky-300" />
        <span className="min-w-0 flex-1 truncate font-medium">{active.label}</span>
        <button
          onClick={onCancel}
          className="shrink-0 rounded px-1.5 py-0.5 text-[11px] text-white/60 hover:bg-white/10 hover:text-white"
        >
          {t('sync.cancel')}
        </button>
      </div>
      {p ? (
        <>
          <div className="mt-1 flex items-center justify-between text-[11px] text-white/55">
            <span>{t(`mailbox.f_${p.folder}`, p.folder)}</span>
            <span className="tabular-nums">
              {p.current}/{p.total}
            </span>
          </div>
          <div className="mt-1 h-1 w-full overflow-hidden rounded-full bg-white/10">
            <div
              className="h-full rounded-full bg-sky-400 transition-[width]"
              style={{ width: `${pct}%` }}
            />
          </div>
        </>
      ) : (
        <div className="mt-2 h-1 w-full overflow-hidden rounded-full bg-white/10">
          <div className="h-full w-1/3 animate-pulse rounded-full bg-sky-400" />
        </div>
      )}
    </div>
  );
}

/** 右上のトースト表示（自動で消える）。 */
function ToastHost({ toasts, onDismiss }: { toasts: ToastItem[]; onDismiss: (id: number) => void }) {
  return (
    <div className="fixed right-4 top-12 z-50 flex w-80 flex-col gap-2">
      {toasts.map((tst) => (
        <div
          key={tst.id}
          className="flex items-start gap-2 rounded-lg border border-white/15 bg-neutral-900/90 p-3 text-xs text-white shadow-2xl backdrop-blur"
        >
          {tst.kind === 'error' ? (
            <XCircle size={15} className="mt-0.5 shrink-0 text-rose-300" />
          ) : (
            <CheckCircle2 size={15} className="mt-0.5 shrink-0 text-emerald-300" />
          )}
          <span className="min-w-0 flex-1 break-words">{tst.text}</span>
          <button
            onClick={() => onDismiss(tst.id)}
            className="shrink-0 text-white/40 hover:text-white/80"
          >
            <X size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}

/** SyncProvider の外からトーストだけ使いたい場合の補助（任意）。 */
export function useToast() {
  return useSync().toast;
}
