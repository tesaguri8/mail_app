import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { MailSummary } from '@bindings/MailSummary';
import { mailList } from '../services/mail';
import { getHomeCountMode, getHomeCountShow, PREFS_EVENT } from '../config/prefs';
import { MAIL_FILTERS, matchesFilters } from './mailFilters';

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
  // 展開中のプレビューに掛ける絞り込み（メール画面と共通のトグル）。
  const [filters, setFilters] = useState<Set<string>>(new Set());
  const toggleFilter = (key: string) =>
    setFilters((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  // バッジの件数表示（表示トグル＋未読数/全数。設定で変更可）。
  const [countShow, setCountShow] = useState(getHomeCountShow());
  const [countMode, setCountMode] = useState(getHomeCountMode());
  useEffect(() => {
    const onPrefs = () => {
      setCountShow(getHomeCountShow());
      setCountMode(getHomeCountMode());
    };
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

  // 展開中は他アカウントを隠し、選択したアカウントだけ表示する。
  const shown = expanded == null ? accounts : accounts.filter((a) => a.id === expanded);

  return (
    <div className="flex h-full min-h-0 flex-col justify-center gap-3 drop-shadow">
      {shown.map((a) => (
        <div
          key={a.id}
          className={expanded === a.id ? 'flex min-h-0 flex-1 flex-col' : 'shrink-0'}
        >
          <button
            onClick={() => toggle(a.id)}
            className="flex w-full shrink-0 items-baseline justify-between gap-3 text-left text-white/85 hover:text-white"
          >
            <span className="truncate">{a.email}</span>
            {countShow && (
              <span className="shrink-0 tabular-nums">
                {countMode === 'total' ? a.total_count : a.unread_count}
              </span>
            )}
          </button>

          {expanded === a.id && (
            <div className="mt-1 flex min-h-0 flex-1 flex-col pl-1">
              {/* 絞り込みアイコン（メール画面と共通のトグル） */}
              <div className="mb-1 flex shrink-0 flex-wrap items-center gap-1">
                {MAIL_FILTERS.map(({ key, Icon }) => {
                  const on = filters.has(key);
                  return (
                    <button
                      key={key}
                      onClick={() => toggleFilter(key)}
                      title={t(`filter.${key}`)}
                      aria-label={t(`filter.${key}`)}
                      aria-pressed={on}
                      className={`flex h-7 w-7 items-center justify-center rounded-md ${
                        on
                          ? 'bg-sky-500/40 text-white ring-1 ring-sky-200/50'
                          : 'text-white/60 hover:bg-white/10 hover:text-white/90'
                      }`}
                    >
                      <Icon size={14} />
                    </button>
                  );
                })}
              </div>
              {(() => {
                const list = (previews[a.id] ?? []).filter((m) => matchesFilters(m, filters));
                return (previews[a.id] ?? []).length === 0 ? (
                  <p className="text-xs text-white/45">{t('mailbox.syncHint')}</p>
                ) : list.length === 0 ? (
                  <p className="text-xs text-white/45">{t('mailbox.noMatch')}</p>
                ) : (
                  // バー間いっぱいまで伸ばし、超過分はスクロール
                  <div className="min-h-0 flex-1 space-y-2 overflow-y-auto pr-1">
                    {list.map((m) => (
                    <div
                      key={m.id}
                      className="cursor-pointer"
                      onClick={() => onOpenMail(a.id, m.id)}
                    >
                      <div className="truncate text-sm text-white/90">
                        {/* 未読は先頭に印を表示 */}
                        {!m.is_read && (
                          <span className="mr-1 align-middle text-[10px] text-sky-300">●</span>
                        )}
                        {m.subject ?? '(no subject)'}
                      </div>
                      <div className="line-clamp-3 text-xs leading-snug text-white/50">
                        {m.preview}
                      </div>
                    </div>
                    ))}
                  </div>
                );
              })()}
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
