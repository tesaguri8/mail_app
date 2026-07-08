import { useEffect, useState } from 'react';

/**
 * lucide の contact（住所録）カード枠を流用し、中の人物（頭＋肩）を今日の日付（1〜31）に
 * 差し替えたカレンダーアイコン。枠が広いぶん数字を大きく描けて視認性が高い。
 * stroke=currentColor で色は親に追従（lucide アイコンと同じ扱い）。
 * day を渡さなければ端末の今日を表示し、日付が変わったら自動で更新する（毎分チェック）。
 */
export function CalendarDateIcon({
  size = 15,
  day,
  className,
}: {
  size?: number;
  /** 表示する日（省略時は今日）。 */
  day?: number;
  className?: string;
}) {
  const [today, setToday] = useState(() => new Date().getDate());
  useEffect(() => {
    if (day !== undefined) return;
    const id = setInterval(() => {
      const d = new Date().getDate();
      setToday((prev) => (prev === d ? prev : d));
    }, 60_000);
    return () => clearInterval(id);
  }, [day]);

  const n = day ?? today;
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      {/* contact のカード枠（上部タブ2つ＋角丸カード。元アイコンの人物＝頭の円と肩は描かない） */}
      <path d="M16 2v2" />
      <path d="M8 2v2" />
      <rect x="3" y="4" width="18" height="18" rx="2" />
      {/* 今日の日付（カード中央に大きく塗りで描く） */}
      <text
        x="12"
        y="13"
        textAnchor="middle"
        dominantBaseline="central"
        fontSize={n >= 10 ? 12.5 : 15.5}
        fontWeight={700}
        fontFamily="ui-sans-serif, system-ui, sans-serif"
        fill="currentColor"
        stroke="none"
      >
        {n}
      </text>
    </svg>
  );
}
