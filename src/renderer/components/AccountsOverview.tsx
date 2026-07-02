import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailSummary } from '@bindings/MailSummary';
import { mailList } from '../services/mail';
import { getHomeCountMode, PREFS_EVENT } from '../config/prefs';

/**
 * ホーム右カラム：アカウント別の新着（未読）数を“ゴースト”表示（背景なし・文字のみ）。
 * クリックでその場展開 → 最新件名＋3行プレビュー →「もっと読む」でメールモードへ。
 */
export function AccountsOverview({
  accounts,
  onOpenMail,
}: {
  accounts: AccountSummary[];
  onOpenMail: (accountId: number, mailId?: number) => void;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState<number | null>(null);
  const [previews, setPreviews] = useState<Record<number, MailSummary[]>>({});
  // バッジの件数表示（未読数/全数/非表示。設定で変更可）。
  const [countMode, setCountMode] = useState(getHomeCountMode());
  useEffect(() => {
    const onPrefs = () => setCountMode(getHomeCountMode());
    window.addEventListener(PREFS_EVENT, onPrefs);
    return () => window.removeEventListener(PREFS_EVENT, onPrefs);
  }, []);

  const toggle = (id: number) => {
    if (expanded === id) {
      setExpanded(null);
      return;
    }
    setExpanded(id);
    if (!previews[id]) {
      // 一覧は多めに取得（表示はバー間いっぱいまで伸ばしてスクロール）。ダッシュボードは受信箱。
      mailList(id, 'inbox', 100)
        .then((m) => setPreviews((p) => ({ ...p, [id]: m })))
        .catch(() => undefined);
    }
  };

  if (accounts.length === 0) {
    return <p className="text-sm text-white/70 drop-shadow">{t('mailbox.addInSettings')}</p>;
  }

  return (
    <div className="flex h-full min-h-0 flex-col justify-center gap-3 drop-shadow">
      {accounts.map((a) => (
        <div
          key={a.id}
          className={expanded === a.id ? 'flex min-h-0 flex-1 flex-col' : 'shrink-0'}
        >
          <button
            onClick={() => toggle(a.id)}
            className="flex w-full shrink-0 items-baseline justify-between gap-3 text-left text-white/85 hover:text-white"
          >
            <span className="truncate">{a.email}</span>
            {countMode !== 'hidden' && (
              <span className="shrink-0 tabular-nums">
                {countMode === 'total' ? a.total_count : a.unread_count}
              </span>
            )}
          </button>

          {expanded === a.id && (
            <div className="mt-1 flex min-h-0 flex-1 flex-col pl-1">
              {(previews[a.id] ?? []).length === 0 ? (
                <p className="text-xs text-white/45">{t('mailbox.syncHint')}</p>
              ) : (
                // バー間いっぱいまで伸ばし、超過分はスクロール
                <div className="min-h-0 flex-1 space-y-2 overflow-y-auto pr-1">
                  {(previews[a.id] ?? []).map((m) => (
                    <div
                      key={m.id}
                      className="cursor-pointer"
                      onClick={() => onOpenMail(a.id, m.id)}
                    >
                      <div className="truncate text-sm text-white/90">
                        {m.subject ?? '(no subject)'}
                      </div>
                      <div className="line-clamp-3 text-xs leading-snug text-white/50">
                        {m.preview}
                      </div>
                    </div>
                  ))}
                </div>
              )}
              <button
                onClick={() => onOpenMail(a.id)}
                className="mt-2 shrink-0 text-xs text-sky-200/90 hover:underline"
              >
                {t('mailbox.more')}
              </button>
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
