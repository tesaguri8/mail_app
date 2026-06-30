import { useTranslation } from 'react-i18next';

/**
 * 全ビュー共通のボトムバー（常設）。今は背景の濃さ調整。
 * 将来はステータス・今日のメッセージ等を左側に置ける。
 */
export function BottomBar({
  dim,
  onDimChange,
}: {
  dim: number;
  onDimChange: (v: number) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex h-8 shrink-0 items-center gap-3 border-t border-white/10 px-4 text-xs text-white/55">
      {/* 左: 将来のステータス領域 */}
      <div className="flex-1" />
      {/* 右: 背景の濃さ */}
      <span>{t('mailbox.dim')}</span>
      <input
        type="range"
        min={0}
        max={0.45}
        step={0.01}
        value={dim}
        onChange={(e) => onDimChange(Number(e.target.value))}
        className="w-40 accent-sky-400"
        title={t('mailbox.dim')}
      />
      <span className="w-9 text-right tabular-nums text-white/40">{Math.round(dim * 100)}%</span>
    </div>
  );
}
