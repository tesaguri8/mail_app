import { useTranslation } from 'react-i18next';

/**
 * 全ビュー共通のボトムバー（常設）。今は背景の濃さ調整。
 * 将来はステータス・今日のメッセージ等を左側に置ける。
 */
export function BottomBar({
  dim,
  onDimChange,
  mailTotal,
}: {
  dim: number;
  onDimChange: (v: number) => void;
  /** 表示中アカウントのメール総数（メールモード時のみ。他は null）。 */
  mailTotal?: number | null;
}) {
  const { t } = useTranslation();
  const MAX = 0.45;
  const pct = Math.round((dim / MAX) * 100);
  return (
    <div className="flex h-8 shrink-0 items-center gap-3 border-t border-white/10 px-4 text-xs text-white/55">
      {/* 左: 表示中アカウントのメール総数（メールモード時） */}
      <div className="flex-1">
        {mailTotal != null && (
          <span className="tabular-nums text-white/45">
            {t('mailbox.accountTotal', { total: mailTotal.toLocaleString() })}
          </span>
        )}
      </div>
      {/* 右: 背景の濃さ（ラベルなし。左側は進捗で塗る） */}
      <input
        type="range"
        min={0}
        max={MAX}
        step={0.01}
        value={dim}
        onChange={(e) => onDimChange(Number(e.target.value))}
        className="thin-range w-40"
        style={{
          background: `linear-gradient(to right, #7dd3fc 0%, #7dd3fc ${pct}%, rgba(255,255,255,0.25) ${pct}%, rgba(255,255,255,0.25) 100%)`,
        }}
        title={t('mailbox.dim')}
      />
      <span className="w-9 text-right tabular-nums text-white/40">{Math.round(dim * 100)}%</span>
    </div>
  );
}
