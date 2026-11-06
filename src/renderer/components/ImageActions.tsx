import type { SyntheticEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { Download, ExternalLink } from 'lucide-react';

/** 画像に対する操作（保存 / 既定アプリで開く）。 */
export type ImageAction = 'save' | 'open';

/**
 * 画像の右上に重ねる「保存」「開く」のボタン。右クリックのメニューだけでは気づけない
 * （利用者報告 2026-11-06「ダウンロードボタン等がありません」）ので、目に見える形で出す。
 *
 * - 親に `relative` と `group/img` を付けた枠の中に置く（ホバーで濃くする）。
 * - タッチや見落としを考えて、ホバーしていなくても薄く見えている。
 * - クリック・ダブルクリックは親（画像の拡大、バブルの展開、本文のリンク）へ渡さない。
 */
export function ImageActions({
  name,
  onAction,
}: {
  /** 画像のファイル名（読み上げ用のラベルに使う）。 */
  name: string;
  onAction: (action: ImageAction) => void;
}) {
  const { t } = useTranslation();
  const stop = (e: SyntheticEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };
  const buttons = [
    { action: 'save' as const, label: t('mailbox.imageSave'), Icon: Download },
    { action: 'open' as const, label: t('mailbox.imageOpen'), Icon: ExternalLink },
  ];
  return (
    <span
      onDoubleClick={stop}
      className="absolute right-1 top-1 flex gap-1 opacity-60 transition-opacity focus-within:opacity-100 group-hover/img:opacity-100"
    >
      {buttons.map(({ action, label, Icon }) => (
        <button
          key={action}
          type="button"
          onClick={(e) => {
            stop(e);
            onAction(action);
          }}
          title={label}
          aria-label={`${label}: ${name}`}
          data-image-action={action}
          className="flex h-6 w-6 items-center justify-center rounded-md bg-black/55 text-white/90 shadow hover:bg-black/75 hover:text-sky-300"
        >
          <Icon size={13} />
        </button>
      ))}
    </span>
  );
}
