import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import type { AccountSyncStatus } from '@bindings/AccountSyncStatus';

/**
 * 同期の時刻を短く出す（今日なら時刻だけ、ほかの日は月日つき）。`locale` はアプリの表示の言語
 * （WebView の既定の言語ではなく、i18n の言語に合わせる。無ければ WebView の既定）。
 */
export function formatSyncTime(iso: string, now: Date = new Date(), locale?: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const time = d.toLocaleTimeString(locale, { hour: 'numeric', minute: '2-digit' });
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  return sameDay
    ? time
    : `${d.toLocaleDateString(locale, { month: 'numeric', day: 'numeric' })} ${time}`;
}

/**
 * 接続状況の説明（ツールチップと、失敗のときの補足の行）。状態が無ければ「まだ同期していない」。
 * 失敗のときは種類と、最後に成功した時刻（この起動で成功していなければ、その旨）を出す。
 */
export function accountStatusLabel(
  t: TFunction,
  status: AccountSyncStatus | undefined,
  now?: Date,
  locale?: string
): string {
  if (!status || status.phase === 'never') return t('mailbox.syncStatus.never');
  if (status.phase === 'syncing') return t('mailbox.syncStatus.syncing');
  if (status.phase === 'ok') {
    // 軽い到達確認だけで、まだ同期・ログインしていない（つながることだけ分かっている）。
    if (!status.last_success_at) return t('mailbox.syncStatus.reachable');
    return t('mailbox.syncStatus.ok', {
      time: status.last_success_at ? formatSyncTime(status.last_success_at, now, locale) : '',
    });
  }
  const kind = t(`mailbox.syncStatus.${status.error_kind ?? 'other'}`);
  const since = status.last_success_at
    ? t('mailbox.syncStatus.lastSuccess', {
        time: formatSyncTime(status.last_success_at, now, locale),
      })
    : t('mailbox.syncStatus.noSuccess');
  return `${kind}${since}`;
}

/** 印の色（正常は緑・同期中は水色で点滅・失敗は赤・まだ同期していないは灰）。 */
const DOT_CLASS = {
  ok: 'bg-emerald-400',
  syncing: 'bg-sky-400 animate-pulse',
  failed: 'bg-rose-500',
  never: 'bg-white/30',
} as const;

/** 悪い順（いくつかのメールアカウントを 1 つの印にまとめるとき、いちばん悪いものを出す）。 */
const SEVERITY = { failed: 3, syncing: 2, never: 1, ok: 0 } as const;

/** いくつかのメールアカウントの状態のうち、いちばん悪いもの（設定のカードの見出し用）。 */
export function worstStatus(
  list: readonly (AccountSyncStatus | undefined)[]
): AccountSyncStatus | undefined {
  let worst: AccountSyncStatus | undefined;
  let rank = -1;
  for (const s of list) {
    const r = SEVERITY[s?.phase ?? 'never'];
    if (r > rank) {
      rank = r;
      worst = s;
    }
  }
  return worst;
}

/**
 * メールの接続状況の小さな印（サイドバーのアカウント選択と、設定のアカウントのカードの見出しで共有）。
 * ツールチップに、状態（失敗のときは種類と最後に成功した時刻）を出す。
 */
export function AccountStatusDot({ status }: { status: AccountSyncStatus | undefined }) {
  const { t, i18n } = useTranslation();
  const phase = status?.phase ?? 'never';
  const label = accountStatusLabel(t, status, undefined, i18n.language);
  return (
    <span
      data-sync-phase={phase}
      role="img"
      aria-label={label}
      title={label}
      className={`inline-block h-2 w-2 shrink-0 rounded-full ${DOT_CLASS[phase]}`}
    />
  );
}
