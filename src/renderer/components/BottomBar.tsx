import { useTranslation } from 'react-i18next';
import { Image, Type } from 'lucide-react';
import { BAR_MAX } from '../App';

/**
 * 全ビュー共通のボトムバー（常設）。右側の 1 本のスライダーで、背景の濃さ（image）と
 * 文字色の白→黒（type）を切り替えて調整する。アイコンでモードを選ぶ。
 * 将来はステータス・今日のメッセージ等を左側に置ける。
 */
export function BottomBar({
  dim,
  onDimChange,
  ink,
  onInkChange,
  mode,
  onModeChange,
  mailTotal,
}: {
  /** 背景のかぶせ（暗さ）。0〜BAR_MAX。 */
  dim: number;
  onDimChange: (v: number) => void;
  /** 文字色（白→黒）。0〜BAR_MAX。 */
  ink: number;
  onInkChange: (v: number) => void;
  /** スライダーの操作対象（背景の濃さ / 文字色）。 */
  mode: 'backdrop' | 'ink';
  onModeChange: (m: 'backdrop' | 'ink') => void;
  /** 表示中アカウントのメール総数（メールモード時のみ。他は null）。 */
  mailTotal?: number | null;
}) {
  const { t } = useTranslation();
  const value = mode === 'ink' ? ink : dim;
  const setValue = mode === 'ink' ? onInkChange : onDimChange;
  const pct = Math.round((value / BAR_MAX) * 100);

  const iconBtn = (active: boolean) =>
    `flex h-5 w-5 items-center justify-center rounded ${
      active ? 'bg-white/25 text-white/90' : 'text-white/45 hover:text-white/80'
    }`;

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
      {/* 右: モード切替アイコン（背景の濃さ / 文字色）＋兼用スライダー */}
      <button
        type="button"
        onClick={() => onModeChange('backdrop')}
        className={iconBtn(mode === 'backdrop')}
        title={t('bottombar.backdrop')}
        aria-pressed={mode === 'backdrop'}
      >
        <Image size={13} />
      </button>
      <button
        type="button"
        onClick={() => onModeChange('ink')}
        className={iconBtn(mode === 'ink')}
        title={t('bottombar.textColor')}
        aria-pressed={mode === 'ink'}
      >
        <Type size={13} />
      </button>
      <input
        type="range"
        min={0}
        max={BAR_MAX}
        step={0.01}
        value={value}
        onChange={(e) => setValue(Number(e.target.value))}
        className="thin-range w-40"
        style={{
          background: `linear-gradient(to right, #7dd3fc 0%, #7dd3fc ${pct}%, rgba(255,255,255,0.25) ${pct}%, rgba(255,255,255,0.25) 100%)`,
        }}
        title={mode === 'ink' ? t('bottombar.textColor') : t('bottombar.backdrop')}
      />
      <span className="w-9 text-right tabular-nums text-white/40">{Math.round(value * 100)}%</span>
    </div>
  );
}
