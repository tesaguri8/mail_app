import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailSummary } from '@bindings/MailSummary';
import { mailList } from '../services/mail';

/**
 * ホーム右カラム：アカウント別の新着（未読）数を“ゴースト”表示（背景なし・文字のみ）。
 * クリックでその場展開 → 最新件名＋3行プレビュー →「もっと読む」でメールモードへ。
 */
export function AccountsOverview({
  accounts,
  onOpenMail,
}: {
  accounts: AccountSummary[];
  onOpenMail: (accountId: number) => void;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState<number | null>(null);
  const [previews, setPreviews] = useState<Record<number, MailSummary[]>>({});

  const toggle = (id: number) => {
    if (expanded === id) {
      setExpanded(null);
      return;
    }
    setExpanded(id);
    if (!previews[id]) {
      mailList(id, 3)
        .then((m) => setPreviews((p) => ({ ...p, [id]: m })))
        .catch(() => undefined);
    }
  };

  if (accounts.length === 0) {
    return <p className="text-sm text-white/70 drop-shadow">{t('mailbox.addInSettings')}</p>;
  }

  return (
    <div className="space-y-3 drop-shadow">
      {accounts.map((a) => (
        <div key={a.id}>
          <button
            onClick={() => toggle(a.id)}
            className="flex w-full items-baseline justify-between gap-3 text-left text-white/85 hover:text-white"
          >
            <span className="truncate">{a.email}</span>
            <span className="shrink-0 tabular-nums">{a.unread_count}件</span>
          </button>

          {expanded === a.id && (
            <div className="mt-1 space-y-2 pl-1">
              {(previews[a.id] ?? []).length === 0 ? (
                <p className="text-xs text-white/45">{t('mailbox.syncHint')}</p>
              ) : (
                (previews[a.id] ?? []).map((m) => (
                  <div
                    key={m.id}
                    className="cursor-pointer"
                    onClick={() => onOpenMail(a.id)}
                  >
                    <div className="truncate text-sm text-white/90">
                      {m.subject ?? '(no subject)'}
                    </div>
                    <div className="line-clamp-3 text-xs leading-snug text-white/50">
                      {m.preview}
                    </div>
                  </div>
                ))
              )}
              <button
                onClick={() => onOpenMail(a.id)}
                className="text-xs text-sky-200/90 hover:underline"
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
