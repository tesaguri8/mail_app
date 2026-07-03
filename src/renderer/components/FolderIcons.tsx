import { useTranslation } from 'react-i18next';
import { Ban, FilePen, Inbox, Send, Trash2, type LucideIcon } from 'lucide-react';

/** 標準フォルダ（並び順）。受信箱・下書き・送信済・ごみ箱・迷惑メール。 */
const STANDARD_FOLDERS = ['inbox', 'drafts', 'sent', 'trash', 'spam'] as const;

const FOLDER_ICON: Record<string, LucideIcon> = {
  inbox: Inbox,
  drafts: FilePen,
  sent: Send,
  trash: Trash2,
  spam: Ban,
};

/**
 * フォルダ選択をアイコンボタンだけで並べる（ドロップダウンをやめてワンタップ切替）。
 * ラベルは title/aria に入れ、見た目はアイコンのみ。フィルタのトグルと同じ意匠。
 */
export function FolderIcons({
  value,
  onChange,
}: {
  value: string;
  onChange: (key: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex shrink-0 items-center gap-0.5">
      {STANDARD_FOLDERS.map((k) => {
        const Icon = FOLDER_ICON[k];
        const on = value === k;
        return (
          <button
            key={k}
            onClick={() => onChange(k)}
            title={t(`mailbox.f_${k}`)}
            aria-label={t(`mailbox.f_${k}`)}
            aria-pressed={on}
            className={`flex h-7 w-7 items-center justify-center rounded-md ${
              on
                ? 'bg-sky-500/30 text-sky-200 ring-1 ring-sky-300/40'
                : 'text-white/55 hover:text-white/80'
            }`}
          >
            <Icon size={15} />
          </button>
        );
      })}
    </div>
  );
}
