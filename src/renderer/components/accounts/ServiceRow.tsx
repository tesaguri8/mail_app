import type { ReactNode } from 'react';

/**
 * カードの中のサービス 1 行（メール・連絡先・カレンダー）。名前と認証の種類、右にスイッチ。
 * `children` はスイッチの下に出す中身（メールの詳細など）。
 */
export function ServiceRow({
  icon,
  label,
  hint,
  checked,
  disabled = false,
  busy = false,
  badge,
  onToggle,
  children,
}: {
  icon: ReactNode;
  label: string;
  /** 使う認証（「App 用パスワード / IMAP」など）や、押せない理由。 */
  hint: string;
  checked: boolean;
  disabled?: boolean;
  busy?: boolean;
  badge?: ReactNode;
  onToggle?: () => void;
  children?: ReactNode;
}) {
  return (
    <div className="py-2">
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2.5">
          <span className="shrink-0 text-white/55">{icon}</span>
          <span className="min-w-0">
            <span className="flex items-center gap-1.5 text-sm text-white/90">
              {label}
              {badge}
            </span>
            <span className="block truncate text-xs text-white/40">{hint}</span>
          </span>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={checked}
          aria-label={label}
          disabled={disabled || busy || !onToggle}
          onClick={onToggle}
          className={`relative h-5 w-9 shrink-0 rounded-full transition-colors disabled:opacity-40 ${
            checked ? 'bg-sky-500' : 'bg-white/20'
          } ${busy ? 'animate-pulse' : ''}`}
        >
          {/* left-0.5 を明示（button は text-align:center のため、無指定だと中央から translate されてはみ出す） */}
          <span
            className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-white transition-transform ${
              checked ? 'translate-x-4' : ''
            }`}
          />
        </button>
      </div>
      {children}
    </div>
  );
}
