import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountSummary } from '@bindings/AccountSummary';
import { AccountsOverview } from './AccountsOverview';
import { HomeSchedule } from './HomeSchedule';

/** 時間帯に応じたあいさつキー（将来は各国の今日のメッセージ等に拡張）。 */
function greetingKey(hour: number): string {
  if (hour >= 5 && hour < 11) return 'greeting.morning';
  if (hour >= 11 && hour < 18) return 'greeting.afternoon';
  if (hour >= 18 && hour < 23) return 'greeting.evening';
  return 'greeting.night';
}

function useClock() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 1000 * 30);
    return () => clearInterval(id);
  }, []);
  return now;
}

/**
 * ホーム: 3カラム（左=空白 / 中央=日時 / 右=アカウント別新着メール・ゴースト表示）。
 */
export function Home({
  accounts,
  onOpenMail,
  onOpenCalendar,
}: {
  accounts: AccountSummary[];
  onOpenMail: (accountId: number) => void;
  onOpenCalendar?: () => void;
}) {
  const { t, i18n } = useTranslation();
  const now = useClock();
  const hh = String(now.getHours()).padStart(2, '0');
  const mm = String(now.getMinutes()).padStart(2, '0');
  const dateStr = new Intl.DateTimeFormat(i18n.language, {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    weekday: 'long',
  }).format(now);

  return (
    <div className="grid h-full grid-cols-3 gap-6 px-8 py-6 text-white">
      {/* 左: 空白 */}
      <div />

      {/* 中央: 日時 */}
      <div className="flex flex-col items-center justify-center text-center">
        <div className="text-7xl font-light tabular-nums tracking-tight drop-shadow">
          {hh}:{mm}
        </div>
        <div className="mt-1 text-sm text-white/70">{dateStr}</div>
        <p className="mt-8 text-lg text-white/85 drop-shadow">{t(greetingKey(now.getHours()))}</p>
      </div>

      {/* 右: 最新メール（ゴースト）＋本日の日程。メールは展開時にバー間いっぱいに伸びる */}
      <div className="flex min-h-0 flex-col gap-4 py-6">
        <div className="flex min-h-0 flex-1 flex-col">
          <AccountsOverview accounts={accounts} onOpenMail={onOpenMail} />
        </div>
        <HomeSchedule onOpenCalendar={onOpenCalendar} />
      </div>
    </div>
  );
}
