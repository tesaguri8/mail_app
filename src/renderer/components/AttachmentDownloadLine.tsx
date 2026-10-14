import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, X } from 'lucide-react';
import {
  dismissDownload,
  downloadPercent,
  formatElapsed,
  isInFlight,
  useAttachmentDownload,
  useEmailDownloads,
  type DownloadState,
} from '../stores/attachmentDownloads';
import { attachmentErrorReason, ERR_IMAP_TIMEOUT } from '../utils/attachmentSave';

/** 経過時間を 1 秒ごとに数え直す（取得中だけ）。 */
function useElapsed(startedAt: number, running: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running) return;
    const h = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(h);
  }, [running]);
  return now - startedAt;
}

/** バイト数を KB / MB で短く出す。 */
function formatBytes(n: number): string {
  if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${Math.max(0, Math.round(n / 1024))} KB`;
}

/**
 * 添付 1 件の取得の状態を、その添付の行に出す（会話のバブルと全文表示で共通）。
 *
 * - 取得中: 段（準備・接続・取得・保存）と経過時間。大きさが分かればパーセントの棒、
 *   分からなければ動いている帯。「終わるまで開けない」ことが分かるようにする
 * - 失敗: 理由を行に残す（トーストに加えて）。時間切れなら「押し直すと続きから取る」と添える
 */
export function AttachmentDownloadLine({
  state,
  showName = false,
}: {
  state: DownloadState;
  /** ファイル名も出す（バブルのように、行が添付の名前を持たない場所で使う）。 */
  showName?: boolean;
}) {
  const { t } = useTranslation();
  const running = isInFlight(state);
  const elapsed = useElapsed(state.startedAt, running);

  if (!running) {
    const timeout = state.error === ERR_IMAP_TIMEOUT;
    return (
      <div className="mt-1.5 flex items-start gap-1.5 text-[11px] text-rose-200/90">
        <AlertTriangle size={12} className="mt-px shrink-0" />
        <span className="min-w-0 flex-1 break-words">
          {showName && <span className="mr-1 font-medium">{state.filename}</span>}
          {t('mailbox.attachmentFailedLine', { reason: attachmentErrorReason(state.error, t) })}
          {timeout && <span className="ml-1 text-white/55">{t('mailbox.attachmentResumeHint')}</span>}
        </span>
        <button
          onClick={() => dismissDownload(state.id)}
          title={t('mailbox.close')}
          aria-label={t('mailbox.close')}
          className="shrink-0 text-white/40 hover:text-white/80"
        >
          <X size={12} />
        </button>
      </div>
    );
  }

  const pct = downloadPercent(state);
  const amount =
    state.total > 0 && state.stage === 'fetching'
      ? `${formatBytes(state.received)} / ${formatBytes(state.total)}`
      : null;
  return (
    <div className="mt-1.5" role="status" aria-live="polite">
      <div className="flex items-baseline gap-2 text-[11px] text-sky-100/80">
        <span className="min-w-0 flex-1 truncate">
          {showName && <span className="mr-1 font-medium text-white/80">{state.filename}</span>}
          {t(`mailbox.attachmentStage.${state.stage}`)}
          <span className="ml-1 text-white/45">{t('mailbox.attachmentNotYet')}</span>
        </span>
        {amount && <span className="shrink-0 tabular-nums text-white/55">{amount}</span>}
        {pct !== null && <span className="shrink-0 tabular-nums">{pct}%</span>}
        <span className="shrink-0 tabular-nums text-white/55">{formatElapsed(elapsed)}</span>
      </div>
      <div className="mt-1 h-1 overflow-hidden rounded-full bg-white/10">
        {pct !== null ? (
          <div
            className="h-full rounded-full bg-sky-400 transition-[width] duration-300"
            style={{ width: `${pct}%` }}
          />
        ) : (
          <div className="att-band h-full w-1/3 rounded-full bg-sky-400/70" />
        )}
      </div>
    </div>
  );
}

/** 添付 1 件の取得の状態を購読して出す（状態が無ければ何も出さない）。 */
export function AttachmentDownloadSlot({ id, showName }: { id: number; showName?: boolean }) {
  const state = useAttachmentDownload(id);
  return state ? <AttachmentDownloadLine state={state} showName={showName} /> : null;
}

/** メール 1 通の、取得中・失敗した添付をまとめて出す（会話のバブルの下）。 */
export function EmailDownloads({ emailId }: { emailId: number }) {
  const items = useEmailDownloads(emailId);
  if (items.length === 0) return null;
  return (
    <div className="mt-1 space-y-1 px-1">
      {items.map((d) => (
        <AttachmentDownloadSlot key={d.id} id={d.id} showName />
      ))}
    </div>
  );
}
